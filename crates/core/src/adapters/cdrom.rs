//! EDC/ECC de setor de CD-ROM (ECMA-130), transcrito do algoritmo de
//! referencia da cena (ECM, Neill Corlett, dominio publico; mesma base do
//! cdrdao/mkpsxiso): EDC = CRC-32 com polinomio 0xD8018001; ECC = Reed-
//! Solomon P (86x24) e Q (52x43) sobre GF(2^8) com polinomio 0x11D.
//! No Mode 2 o bloco ECC usa o header ZERADO (so o Mode 1 inclui o real).

use std::sync::OnceLock;

use super::iso9660::SECTOR_RAW;
use crate::error::{CoreError, Result};

struct Luts {
    f: [u8; 256],
    b: [u8; 256],
    edc: [u32; 256],
}

fn luts() -> &'static Luts {
    static LUTS: OnceLock<Luts> = OnceLock::new();
    LUTS.get_or_init(|| {
        let mut l = Luts {
            f: [0; 256],
            b: [0; 256],
            edc: [0; 256],
        };
        for i in 0..256usize {
            let j = ((i << 1) ^ (if i & 0x80 != 0 { 0x11D } else { 0 })) & 0xFF;
            l.f[i] = j as u8;
            l.b[i ^ j] = i as u8;
            let mut edc = i as u32;
            for _ in 0..8 {
                edc = (edc >> 1) ^ (if edc & 1 != 0 { 0xD801_8001 } else { 0 });
            }
            l.edc[i] = edc;
        }
        l
    })
}

/// EDC (init 0) sobre `data`; armazenado little-endian no setor.
pub fn edc_compute(data: &[u8]) -> u32 {
    let l = luts();
    let mut edc = 0u32;
    for &b in data {
        edc = (edc >> 8) ^ l.edc[((edc ^ b as u32) & 0xFF) as usize];
    }
    edc
}

/// Escreve um bloco P ou Q. `address` = 4 bytes do header (zeros no Mode 2);
/// `data` comeca em sector[0x10] — no passo Q inclui a paridade P ja escrita.
fn ecc_writepq(
    address: &[u8; 4],
    data: &[u8],
    major_count: usize,
    minor_count: usize,
    major_mult: usize,
    minor_inc: usize,
    ecc: &mut [u8],
) {
    let l = luts();
    let size = major_count * minor_count;
    for major in 0..major_count {
        let mut index = (major >> 1) * major_mult + (major & 1);
        let mut ecc_a = 0u8;
        let mut ecc_b = 0u8;
        for _ in 0..minor_count {
            let temp = if index < 4 {
                address[index]
            } else {
                data[index - 4]
            };
            index += minor_inc;
            if index >= size {
                index -= size;
            }
            ecc_a ^= temp;
            ecc_b ^= temp;
            ecc_a = l.f[ecc_a as usize];
        }
        ecc_a = l.b[(l.f[ecc_a as usize] ^ ecc_b) as usize];
        ecc[major] = ecc_a;
        ecc[major + major_count] = ecc_a ^ ecc_b;
    }
}

/// Preenche sector[0x81C..0x930] (P depois Q). No original os ponteiros
/// `data` e `ecc` se sobrepoem: o passo Q le a paridade P recem-escrita
/// (indices ate 0x8C8) — por isso dois passes com slices distintos.
fn write_ecc(sector: &mut [u8], address: &[u8; 4]) {
    let mut p = [0u8; 172];
    ecc_writepq(address, &sector[0x10..0x81C], 86, 24, 2, 86, &mut p);
    sector[0x81C..0x8C8].copy_from_slice(&p);
    let mut q = [0u8; 104];
    ecc_writepq(address, &sector[0x10..0x8C8], 52, 43, 86, 88, &mut q);
    sector[0x8C8..0x930].copy_from_slice(&q);
}

/// Recalcula EDC (e ECC quando o modo tem) de um setor raw de 2352 bytes,
/// apos os dados terem sido modificados.
pub fn regenerate_sector(sector: &mut [u8]) -> Result<()> {
    if sector.len() != SECTOR_RAW {
        return Err(CoreError::Project(
            "cdrom: setor nao tem 2352 bytes".to_string(),
        ));
    }
    match sector[15] {
        0 => Ok(()), // Mode 0: setor vazio, nada a recalcular
        1 => {
            let edc = edc_compute(&sector[0..0x810]);
            sector[0x810..0x814].copy_from_slice(&edc.to_le_bytes());
            let address: [u8; 4] = sector[12..16].try_into().unwrap();
            write_ecc(sector, &address);
            Ok(())
        }
        2 => {
            // Submode (byte 2 do subheader): bit 0x20 = Form 2.
            if sector[18] & 0x20 != 0 {
                // Form 2: EDC opcional em 0x92C; recalcula so se ja era usado.
                if sector[0x92C..0x930] != [0, 0, 0, 0] {
                    let edc = edc_compute(&sector[0x10..0x92C]);
                    sector[0x92C..0x930].copy_from_slice(&edc.to_le_bytes());
                }
                return Ok(());
            }
            // Form 1: EDC sobre subheader+user (0x10..0x818), ECC com header zerado.
            let edc = edc_compute(&sector[0x10..0x818]);
            sector[0x818..0x81C].copy_from_slice(&edc.to_le_bytes());
            write_ecc(sector, &[0, 0, 0, 0]);
            Ok(())
        }
        mode => Err(CoreError::Project(format!(
            "cdrom: modo de setor desconhecido ({mode})"
        ))),
    }
}

/// Confere o EDC de um setor raw. `None` = nao aplicavel (Mode 0, ou Form 2
/// sem EDC); `Some(bool)` = valido/invalido.
pub fn sector_edc_ok(sector: &[u8]) -> Option<bool> {
    if sector.len() != SECTOR_RAW {
        return Some(false);
    }
    match sector[15] {
        0 => None,
        1 => {
            let stored = u32::from_le_bytes(sector[0x810..0x814].try_into().unwrap());
            Some(edc_compute(&sector[0..0x810]) == stored)
        }
        2 => {
            if sector[18] & 0x20 != 0 {
                let stored = u32::from_le_bytes(sector[0x92C..0x930].try_into().unwrap());
                if stored == 0 {
                    return None;
                }
                return Some(edc_compute(&sector[0x10..0x92C]) == stored);
            }
            let stored = u32::from_le_bytes(sector[0x818..0x81C].try_into().unwrap());
            Some(edc_compute(&sector[0x10..0x818]) == stored)
        }
        _ => Some(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_data_yields_zero_edc_and_zero_ecc() {
        // Propriedade estrutural (algoritmo linear, init 0): setor Mode 2
        // Form 1 todo zero regenera pra EDC 0 e ECC 0 (o mode byte fica fora
        // do EDC e o ECC de Mode 2 usa header zerado).
        assert_eq!(edc_compute(&[0u8; 2064]), 0);
        let mut sector = vec![0u8; SECTOR_RAW];
        sector[15] = 2;
        regenerate_sector(&mut sector).unwrap();
        assert!(sector[16..].iter().all(|&b| b == 0));
    }

    fn pattern_sector() -> Vec<u8> {
        (0..SECTOR_RAW)
            .map(|i| ((i * 7 + 3) & 0xFF) as u8)
            .collect()
    }

    /// Valores gerados pelo ecm.c de referencia (Neill Corlett) compilado
    /// nesta maquina sobre o mesmo setor deterministico — trava a
    /// transcricao do algoritmo contra regressao.
    #[test]
    fn golden_values_match_reference_implementation() {
        // Mode 1
        let mut s = pattern_sector();
        s[15] = 0x01;
        regenerate_sector(&mut s).unwrap();
        assert_eq!(
            u32::from_le_bytes(s[0x810..0x814].try_into().unwrap()),
            0x33AB_2810
        );
        assert_eq!((s[0x81C], s[0x81C + 171]), (0x2A, 0x87));
        assert_eq!((s[0x8C8], s[0x8C8 + 103]), (0xA3, 0x6D));
        assert_eq!(
            s[0x810..0x930].iter().map(|&b| b as u32).sum::<u32>(),
            37733
        );
        assert_eq!(sector_edc_ok(&s), Some(true));

        // Mode 2 Form 1 (byte 18 do pattern = 0x81, bit 0x20 limpo)
        let mut s = pattern_sector();
        s[15] = 0x02;
        regenerate_sector(&mut s).unwrap();
        assert_eq!(
            u32::from_le_bytes(s[0x818..0x81C].try_into().unwrap()),
            0x1F4D_AB0E
        );
        assert_eq!((s[0x81C], s[0x81C + 171]), (0xBB, 0x24));
        assert_eq!((s[0x8C8], s[0x8C8 + 103]), (0x34, 0x9F));
        assert_eq!(
            s[0x810..0x930].iter().map(|&b| b as u32).sum::<u32>(),
            36302
        );
        assert_eq!(sector_edc_ok(&s), Some(true));

        // Mode 2 Form 2 (EDC opcional presente no pattern -> recalcula)
        let mut s = pattern_sector();
        s[15] = 0x02;
        s[18] |= 0x20;
        regenerate_sector(&mut s).unwrap();
        assert_eq!(
            u32::from_le_bytes(s[0x92C..0x930].try_into().unwrap()),
            0x66C1_A87D
        );
        assert_eq!(sector_edc_ok(&s), Some(true));
    }

    #[test]
    fn regenerate_is_idempotent_and_detected_by_edc_check() {
        let mut sector = vec![0u8; SECTOR_RAW];
        sector[15] = 2; // Mode 2 Form 1 (submode 0)
        sector[100] = 0x41;
        regenerate_sector(&mut sector).unwrap();
        assert_eq!(sector_edc_ok(&sector), Some(true));

        let snapshot = sector.clone();
        regenerate_sector(&mut sector).unwrap();
        assert_eq!(sector, snapshot, "idempotente");

        // Corromper dados sem regenerar: o EDC acusa.
        sector[200] ^= 0xFF;
        assert_eq!(sector_edc_ok(&sector), Some(false));
    }
}

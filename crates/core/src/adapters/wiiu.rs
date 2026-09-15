//! Probe de Wii U — formatos VERIFICADOS em fonte primaria:
//! - WUX (imagem de disco comprimida, WudCompress/Cemu): magic "WUX0" +
//!   0x1099D02E, sectorSize e uncompressedSize no header;
//! - RPX/RPL (executavel Cafe OS, decaf-emu loader): ELF 32-bit big-endian
//!   com e_ident[OSABI]=0xCA e abiVersion=0xFE ("CAFE"), machine PowerPC,
//!   secoes comprimidas (SHF_DEFLATED).
//!
//! Deteccao apenas: conteudo de disco Wii U e cifrado (keys nao entram neste
//! projeto) e secoes RPX sao deflated — extracao fica para uma fase futura.
//! WUD bruto ficou de fora: sem magic documentado confiavel.

use crate::adapter::{GameAdapter, GameInput};
use crate::types::{AdapterCapabilities, Platform, ProbeResult};

pub struct WiiUAdapter;

pub const WUX_MAGIC0: &[u8; 4] = b"WUX0";
pub const WUX_MAGIC1: u32 = 0x1099_D02E;
pub const CAFE_OSABI: u8 = 0xCA;
pub const CAFE_ABI_VERSION: u8 = 0xFE;
const EM_PPC: u16 = 20;

fn probe_wux(head: &[u8]) -> Option<(f32, Vec<String>)> {
    if head.len() < 0x18 || &head[0..4] != WUX_MAGIC0 {
        return None;
    }
    if u32::from_le_bytes(head[4..8].try_into().unwrap()) != WUX_MAGIC1 {
        return None;
    }
    let sector_size = u32::from_le_bytes(head[8..12].try_into().unwrap());
    let uncompressed = u64::from_le_bytes(head[0x0C..0x14].try_into().unwrap());
    let mut evidence = vec![
        "imagem de disco Wii U comprimida (WUX, WudCompress): magic dupla valida".to_string(),
        format!(
            "setor de {} KiB, imagem original de {:.1} GiB",
            sector_size / 1024,
            uncompressed as f64 / (1024.0 * 1024.0 * 1024.0)
        ),
        "conteudo de disco cifrado: extracao exigiria keys (fora do projeto)".to_string(),
    ];
    let mut confidence = 0.95;
    if (0x1000..=0x10_0000).contains(&sector_size) {
        confidence += 0.03;
    } else {
        evidence.push("sectorSize fora do usual (header corrompido?)".to_string());
    }
    Some((confidence, evidence))
}

fn probe_rpx(head: &[u8]) -> Option<(f32, Vec<String>)> {
    if head.len() < 0x34 || &head[0..4] != b"\x7FELF" {
        return None;
    }
    // ELFCLASS32 + big-endian + OSABI/abiVersion "CA FE" (decaf: EABI_CAFE).
    if head[4] != 1 || head[5] != 2 || head[7] != CAFE_OSABI || head[8] != CAFE_ABI_VERSION {
        return None;
    }
    let machine = u16::from_be_bytes([head[0x12], head[0x13]]);
    if machine != EM_PPC {
        return None;
    }
    Some((
        0.97,
        vec![
            "executavel Cafe OS (RPX/RPL): ELF 32-bit big-endian PowerPC".to_string(),
            "e_ident OSABI 0xCA + versao 0xFE (\"CAFE\")".to_string(),
            "secoes tipicamente comprimidas (SHF_DEFLATED): extracao em fase futura".to_string(),
        ],
    ))
}

impl GameAdapter for WiiUAdapter {
    fn id(&self) -> &'static str {
        "wiiu.probe"
    }

    fn display_name(&self) -> &'static str {
        "Wii U (WUX / RPX)"
    }

    fn platform(&self) -> Platform {
        Platform::WiiU
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::detect_only()
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        let head = &input.head;
        let Some((confidence, evidence)) = probe_wux(head).or_else(|| probe_rpx(head)) else {
            return ProbeResult::no_match(self.id(), self.platform());
        };
        ProbeResult {
            confidence: confidence.min(0.99),
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;
    use crate::types::Platform;

    fn probe(bytes: &[u8]) -> ProbeResult {
        WiiUAdapter.probe(&GameInput::from_bytes("t.bin", bytes))
    }

    #[test]
    fn detects_wux_and_rpx_fixtures() {
        let wux = probe(&synth::make_wux_header());
        assert!(wux.confidence >= 0.95, "{}", wux.confidence);
        assert_eq!(wux.platform, Platform::WiiU);
        assert!(wux.evidence.iter().any(|e| e.contains("WUX")));
        assert!(wux.evidence.iter().any(|e| e.contains("cifrado")));

        let rpx = probe(&synth::make_rpx_header());
        assert!(rpx.confidence >= 0.95, "{}", rpx.confidence);
        assert!(rpx.evidence.iter().any(|e| e.contains("CAFE")));
    }

    #[test]
    fn rejects_plain_elf_random_and_truncated() {
        // ELF comum (Linux, OSABI 0) NAO e RPX.
        let mut elf = synth::make_rpx_header();
        elf[7] = 0x00;
        elf[8] = 0x00;
        assert_eq!(probe(&elf).confidence, 0.0);

        // WUX com a segunda magic errada.
        let mut wux = synth::make_wux_header();
        wux[4] ^= 0xFF;
        assert_eq!(probe(&wux).confidence, 0.0);

        assert_eq!(probe(&synth::make_random(4096, 9)).confidence, 0.0);
        assert_eq!(probe(&[]).confidence, 0.0);
        let full = synth::make_wux_header();
        for len in 0..full.len().min(0x40) {
            probe(&full[..len]);
        }
    }
}

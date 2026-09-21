//! Tabelas de ponteiros absolutos de 32 bits (little-endian) e relocacao de
//! strings — a "Camada C" da spec, que proibe busca/troca global de bytes.
//! Por isso um ponteiro so e reconhecido dentro de uma TABELA: MIN_RUN ou
//! mais palavras alinhadas consecutivas que apontam, cada uma, pro inicio
//! exato de uma string conhecida. Palavra isolada com o mesmo valor e
//! ignorada: pode ser coincidencia (no GBA, a instrucao Thumb LSR comeca com
//! 0x08, igual ao byte alto de um ponteiro de ROM).

use std::collections::{HashMap, HashSet};

use crate::error::{CoreError, Result};

/// Dois ponteiros seguidos pra inicios de string ja e estrutura (menu
/// sim/nao, struct {nome, descricao}); coincidencia dupla e desprezivel.
const MIN_RUN: usize = 2;

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("ponteiros: {}", msg.into()))
}

/// Offset de cada string -> offsets dos ponteiros (em tabelas) pra ela.
/// `base` = endereco que o jogo usa pro offset 0 do arquivo.
pub fn find_pointer_tables(
    data: &[u8],
    base: u32,
    string_starts: &HashSet<usize>,
) -> HashMap<usize, Vec<usize>> {
    let mut tables: HashMap<usize, Vec<usize>> = HashMap::new();
    // (onde o ponteiro esta, pra onde aponta)
    let mut run: Vec<(usize, usize)> = Vec::new();
    let mut flush = |run: &mut Vec<(usize, usize)>| {
        if run.len() >= MIN_RUN {
            for &(at, target) in run.iter() {
                tables.entry(target).or_default().push(at);
            }
        }
        run.clear();
    };
    for (i, word) in data.as_chunks::<4>().0.iter().enumerate() {
        let target = u32::from_le_bytes(*word)
            .checked_sub(base)
            .map(|t| t as usize)
            .filter(|t| string_starts.contains(t));
        match target {
            Some(t) => run.push((i * 4, t)),
            None => flush(&mut run),
        }
    }
    flush(&mut run);
    tables
}

/// Traducao que nao cabe no lugar original mas tem ponteiros em tabela.
#[derive(Debug, Clone)]
pub struct Relocation {
    pub entry_id: String,
    pub original_offset: usize,
    /// Traducao codificada + terminador.
    pub bytes: Vec<u8>,
    pub pointers: Vec<usize>,
}

/// Anexa cada string no fim de `out` e reaponta os ponteiros. Alinhada em 4
/// porque ha engine que copia texto com LDM/STM, e no ARM7 leitura
/// desalinhada rotaciona a palavra. O original fica intacto: uma referencia
/// que a deteccao nao viu continua mostrando o texto antigo, nao lixo.
/// `max_len` = teto enderecavel da plataforma.
pub fn relocate(
    out: &mut Vec<u8>,
    relocations: &[Relocation],
    base: u32,
    max_len: usize,
) -> Result<()> {
    for r in relocations {
        let expected = pointer_value(base, r.original_offset)?;
        for &p in &r.pointers {
            if read_u32(out, p) != Some(expected) {
                return Err(err(format!(
                    "entry {}: o ponteiro em 0x{p:X} nao aponta mais pra string original — \
                     arquivo diferente do extraido? Re-extraia antes de reinserir",
                    r.entry_id
                )));
            }
        }
        let at = out.len().next_multiple_of(4);
        if at + r.bytes.len() > max_len {
            return Err(err(format!(
                "entry {}: sem espaco enderecavel pra realocar (a imagem passaria de \
                 {max_len} bytes) — encurte as traducoes",
                r.entry_id
            )));
        }
        out.resize(at, 0);
        out.extend_from_slice(&r.bytes);
        let new_value = pointer_value(base, at)?.to_le_bytes();
        for &p in &r.pointers {
            out[p..p + 4].copy_from_slice(&new_value);
        }
    }
    out.resize(out.len().next_multiple_of(4), 0);

    // Auto-checagem: todo ponteiro reapontado resolve pros bytes novos.
    for r in relocations {
        for &p in &r.pointers {
            let target = read_u32(out, p)
                .and_then(|v| v.checked_sub(base))
                .map(|t| t as usize);
            let resolved = target.and_then(|t| out.get(t..t.checked_add(r.bytes.len())?));
            if resolved != Some(&r.bytes[..]) {
                return Err(err(format!(
                    "auto-checagem da relocacao falhou na entry {} (bug) — nada foi gravado",
                    r.entry_id
                )));
            }
        }
    }
    Ok(())
}

fn pointer_value(base: u32, offset: usize) -> Result<u32> {
    u32::try_from(offset)
        .ok()
        .and_then(|o| base.checked_add(o))
        .ok_or_else(|| {
            err(format!(
                "offset 0x{offset:X} nao cabe num ponteiro de 32 bits"
            ))
        })
}

fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u32 = 0x0800_0000;

    fn put_ptr(buf: &mut [u8], at: usize, target: usize) {
        buf[at..at + 4].copy_from_slice(&(BASE + target as u32).to_le_bytes());
    }

    #[test]
    fn finds_tables_but_ignores_isolated_and_mid_string_pointers() {
        let mut buf = vec![0u8; 0x80];
        let starts: HashSet<usize> = [0x40, 0x48, 0x50].into_iter().collect();
        put_ptr(&mut buf, 0x10, 0x40); // tabela de 2
        put_ptr(&mut buf, 0x14, 0x48);
        put_ptr(&mut buf, 0x20, 0x50); // isolado: coincidencia possivel
        put_ptr(&mut buf, 0x30, 0x41); // meio de string + inicio: run nao fecha
        put_ptr(&mut buf, 0x34, 0x48);

        let tables = find_pointer_tables(&buf, BASE, &starts);
        assert_eq!(tables.get(&0x40), Some(&vec![0x10]));
        assert_eq!(tables.get(&0x48), Some(&vec![0x14]), "0x34 nao conta");
        assert!(!tables.contains_key(&0x50), "ponteiro isolado nao e tabela");

        // Entrada truncada/vazia/lixo nunca panica.
        for len in 0..buf.len() {
            let _ = find_pointer_tables(&buf[..len], BASE, &starts);
        }
        assert!(find_pointer_tables(&[0xFF; 7], BASE, &starts).is_empty());
    }

    #[test]
    fn relocates_repoints_and_refuses_drift_and_overflow() {
        let mut data = vec![0u8; 0x40];
        data[0x20..0x24].copy_from_slice(b"OLD\0");
        put_ptr(&mut data, 0x00, 0x20);
        put_ptr(&mut data, 0x08, 0x20);
        let reloc = Relocation {
            entry_id: "e".into(),
            original_offset: 0x20,
            bytes: b"TEXTO NOVO\0".to_vec(),
            pointers: vec![0x00, 0x08],
        };

        let mut out = data.clone();
        relocate(&mut out, std::slice::from_ref(&reloc), BASE, 1 << 20).unwrap();
        assert_eq!(&out[0x40..0x4B], b"TEXTO NOVO\0", "anexado alinhado no fim");
        assert_eq!(out.len() % 4, 0);
        assert_eq!(read_u32(&out, 0x00), Some(BASE + 0x40));
        assert_eq!(read_u32(&out, 0x08), Some(BASE + 0x40));
        assert_eq!(&out[0x20..0x24], b"OLD\0", "original intacto");

        // Ponteiro que mudou desde a extracao: recusa.
        let mut drifted = data.clone();
        put_ptr(&mut drifted, 0x08, 0x24);
        let e = relocate(&mut drifted, std::slice::from_ref(&reloc), BASE, 1 << 20).unwrap_err();
        assert!(e.to_string().contains("0x8"), "{e}");

        // Sem espaco enderecavel: recusa em vez de gravar ponteiro invalido.
        let mut full = data.clone();
        let e = relocate(&mut full, std::slice::from_ref(&reloc), BASE, 0x44).unwrap_err();
        assert!(e.to_string().contains("espaco enderecavel"), "{e}");
    }
}

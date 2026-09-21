//! Tabelas de ponteiros absolutos de 32 bits (little-endian) e relocacao de
//! strings — a "Camada C" da spec, que proibe busca/troca global de bytes.
//! Por isso um ponteiro so e reconhecido dentro de uma TABELA: MIN_RUN ou
//! mais palavras alinhadas consecutivas que apontam, cada uma, pro inicio
//! exato de uma string conhecida. Palavra isolada com o mesmo valor e
//! ignorada: pode ser coincidencia (no GBA, a instrucao Thumb LSR comeca com
//! 0x08, igual ao byte alto de um ponteiro de ROM).

use std::collections::{HashMap, HashSet};

use crate::error::{CoreError, Result};
use crate::types::TextEntry;

/// Ponteiro absoluto de ROM (GBA: 0x08xxxxxx) quase nunca aparece por acaso:
/// dois seguidos pra inicios de string ja sao estrutura (menu sim/nao,
/// struct {nome, descricao}).
pub const MIN_RUN_ABSOLUTE: usize = 2;
/// Offset relativo ao inicio de um arquivo e numero PEQUENO, comum em dado
/// binario (tamanho, contagem, coordenada). Com metade das palavras pequenas
/// e 1 inicio de string por KB, um run falso de 2 sai ~1 a cada 15 arquivos
/// de 1 MB; de 3, ~1 a cada 30 mil. Por isso 3.
pub const MIN_RUN_RELATIVE: usize = 3;

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("ponteiros: {}", msg.into()))
}

/// Offset de cada string -> offsets dos ponteiros (em tabelas) pra ela.
/// `base` = endereco que o jogo usa pro offset 0 dos dados (0 = relativo).
pub fn find_pointer_tables(
    data: &[u8],
    base: u32,
    string_starts: &HashSet<usize>,
    min_run: usize,
) -> HashMap<usize, Vec<usize>> {
    let mut tables: HashMap<usize, Vec<usize>> = HashMap::new();
    // (onde o ponteiro esta, pra onde aponta)
    let mut run: Vec<(usize, usize)> = Vec::new();
    let mut flush = |run: &mut Vec<(usize, usize)>| {
        if run.len() >= min_run {
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
                "entry {}: sem espaco pra realocar — as strings realocadas passariam do \
                 limite de {max_len} bytes; encurte as traducoes",
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

/// So string terminada pode crescer: quem le por tamanho fixo nao aceita.
pub fn is_terminated(entry: &TextEntry) -> bool {
    entry.metadata.get("terminated").and_then(|v| v.as_bool()) == Some(true)
}

/// Tabelas dentro de UM arquivo cujos ponteiros sao offsets relativos ao
/// inicio dele (NDS, PS1). `file` = bytes logicos do arquivo; as funcoes
/// traduzem enderecos da imagem <-> do arquivo. Devolve (indice da entry,
/// ponteiros em enderecos absolutos da imagem).
pub fn file_relative_tables(
    file: &[u8],
    entries: &[TextEntry],
    abs_to_rel: impl Fn(usize) -> Option<usize>,
    rel_to_abs: impl Fn(usize) -> Option<usize>,
) -> Vec<(usize, Vec<usize>)> {
    let rel_of = |e: &TextEntry| {
        e.offset
            .and_then(|o| abs_to_rel(o as usize))
            .filter(|&r| r < file.len())
    };
    let starts: HashSet<usize> = entries
        .iter()
        .filter(|e| is_terminated(e))
        .filter_map(rel_of)
        .collect();
    if starts.is_empty() {
        return Vec::new();
    }
    let tables = find_pointer_tables(file, 0, &starts, MIN_RUN_RELATIVE);
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let pointers = tables.get(&rel_of(e)?)?;
            let absolute: Option<Vec<usize>> = pointers.iter().map(|&p| rel_to_abs(p)).collect();
            Some((i, absolute?))
        })
        .collect()
}

/// Grava os ponteiros no metadata (a entry vira realocavel). Falso se o
/// metadata nao e objeto — ai a entry segue so in-place.
pub fn mark_relocatable(entry: &mut TextEntry, pointers: &[usize]) -> bool {
    match entry.metadata.as_object_mut() {
        Some(meta) => {
            meta.insert("pointers".to_string(), serde_json::json!(pointers));
            true
        }
        None => false,
    }
}

/// Nenhuma escrita in-place pode ter pisado num ponteiro de tabela: isso
/// corromperia outra string em silencio (ex.: campo de nome sem terminador
/// colado no ponteiro).
pub fn ensure_pointers_untouched(original: &[u8], out: &[u8], entries: &[TextEntry]) -> Result<()> {
    for entry in entries {
        for p in entry.pointer_offsets() {
            let end = p
                .checked_add(4)
                .ok_or_else(|| err(format!("ponteiro invalido na entry {}", entry.id)))?;
            if out.get(p..end) != original.get(p..end) {
                return Err(err(format!(
                    "uma traducao in-place sobrescreveria o ponteiro em 0x{p:X} (que aponta \
                     pra entry {}) — encurte o texto logo antes desse endereco",
                    entry.id
                )));
            }
        }
    }
    Ok(())
}

/// Converte relocacoes em enderecos absolutos (da imagem) pra relativos ao
/// arquivo que as contem — quando o ponteiro e offset dentro do arquivo.
pub fn to_file_relative(
    relocations: &[Relocation],
    abs_to_rel: impl Fn(usize) -> Option<usize>,
) -> Result<Vec<Relocation>> {
    relocations
        .iter()
        .map(|r| {
            let map = |abs: usize| {
                abs_to_rel(abs).ok_or_else(|| {
                    err(format!(
                        "entry {}: 0x{abs:X} fora do arquivo da string",
                        r.entry_id
                    ))
                })
            };
            Ok(Relocation {
                entry_id: r.entry_id.clone(),
                original_offset: map(r.original_offset)?,
                bytes: r.bytes.clone(),
                pointers: r.pointers.iter().map(|&p| map(p)).collect::<Result<_>>()?,
            })
        })
        .collect()
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

        let tables = find_pointer_tables(&buf, BASE, &starts, MIN_RUN_ABSOLUTE);
        assert_eq!(tables.get(&0x40), Some(&vec![0x10]));
        assert_eq!(tables.get(&0x48), Some(&vec![0x14]), "0x34 nao conta");
        assert!(!tables.contains_key(&0x50), "ponteiro isolado nao e tabela");

        // Offset relativo (numero pequeno) exige run de 3: a tabela de 2 cai.
        let strict = find_pointer_tables(&buf, BASE, &starts, MIN_RUN_RELATIVE);
        assert!(strict.is_empty(), "{strict:?}");

        // Entrada truncada/vazia/lixo nunca panica.
        for len in 0..buf.len() {
            let _ = find_pointer_tables(&buf[..len], BASE, &starts, MIN_RUN_ABSOLUTE);
        }
        assert!(find_pointer_tables(&[0xFF; 7], BASE, &starts, MIN_RUN_ABSOLUTE).is_empty());
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
        assert!(e.to_string().contains("sem espaco pra realocar"), "{e}");
    }
}

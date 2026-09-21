//! Tabelas de ponteiros little-endian e relocacao de strings — a "Camada C"
//! da spec, que proibe busca/troca global de bytes. Por isso um ponteiro so
//! e reconhecido dentro de uma TABELA: palavras alinhadas consecutivas que
//! apontam, cada uma, pro inicio exato de uma string conhecida. Palavra
//! isolada com o mesmo valor e ignorada: pode ser coincidencia (no GBA, a
//! instrucao Thumb LSR comeca com 0x08, igual ao byte alto de um ponteiro de
//! ROM).

use std::collections::{HashMap, HashSet};

use crate::error::{CoreError, Result};
use crate::types::{Pointer, TextEntry};

/// Como os ponteiros de uma tabela sao gravados.
#[derive(Debug, Clone, Copy)]
pub struct PointerFormat {
    /// 4 (u32) ou 2 (u16), little-endian, alinhado na propria largura.
    pub width: usize,
    /// Valor do ponteiro = base + offset nos dados.
    pub base: u32,
    pub min_run: usize,
    /// Offset relativo (numero pequeno, comum em dado binario): a tabela tem
    /// que ser ESTRITAMENTE CRESCENTE e alvo 0 nao conta. Sem isso, padding
    /// de zeros vira "tabela" apontando pra string que abre o arquivo.
    pub relative: bool,
}

/// Offset u32 relativo ao arquivo. Com metade das palavras pequenas e 1
/// inicio de string por KB, um run falso de 2 sai ~1 a cada 15 arquivos de
/// 1 MB; de 3, ~1 a cada 30 mil — isso antes de exigir ordem crescente.
pub const FILE_U32: PointerFormat = PointerFormat {
    width: 4,
    base: 0,
    min_run: 3,
    relative: true,
};

/// Offset u16: num arquivo de texto pequeno quase todo valor de 16 bits cai
/// dentro dele. Pior caso (4 KB, uma string a cada 40 bytes): run crescente
/// de 3 falso ~1 a cada 200 arquivos; de 4, ~1 a cada 35 mil. Por isso 4.
pub const FILE_U16: PointerFormat = PointerFormat {
    width: 2,
    base: 0,
    min_run: 4,
    relative: true,
};

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("ponteiros: {}", msg.into()))
}

/// Offset de cada string -> ponteiros (em tabelas) pra ela.
pub fn find_pointer_tables(
    data: &[u8],
    format: &PointerFormat,
    string_starts: &HashSet<usize>,
) -> HashMap<usize, Vec<Pointer>> {
    let mut tables: HashMap<usize, Vec<Pointer>> = HashMap::new();
    // (onde o ponteiro esta, pra onde aponta)
    let mut run: Vec<(usize, usize)> = Vec::new();
    let mut flush = |run: &mut Vec<(usize, usize)>| {
        if run.len() >= format.min_run {
            for &(at, target) in run.iter() {
                tables.entry(target).or_default().push(Pointer {
                    at,
                    width: format.width,
                });
            }
        }
        run.clear();
    };
    let values: Box<dyn Iterator<Item = u32> + '_> = match format.width {
        2 => Box::new(
            data.as_chunks::<2>()
                .0
                .iter()
                .map(|w| u16::from_le_bytes(*w) as u32),
        ),
        _ => Box::new(
            data.as_chunks::<4>()
                .0
                .iter()
                .map(|w| u32::from_le_bytes(*w)),
        ),
    };
    for (i, value) in values.enumerate() {
        let target = value
            .checked_sub(format.base)
            .map(|t| t as usize)
            .filter(|t| string_starts.contains(t))
            .filter(|&t| !(format.relative && t == 0));
        let Some(target) = target else {
            flush(&mut run);
            continue;
        };
        // Fora de ordem: fecha o run atual; este pode abrir outro.
        if format.relative && run.last().is_some_and(|&(_, prev)| target <= prev) {
            flush(&mut run);
        }
        run.push((i * format.width, target));
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
    pub pointers: Vec<Pointer>,
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
            if read_pointer(out, p) != Some(expected) {
                return Err(err(format!(
                    "entry {}: o ponteiro em 0x{:X} nao aponta mais pra string original — \
                     arquivo diferente do extraido? Re-extraia antes de reinserir",
                    r.entry_id, p.at
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
        let new_value = pointer_value(base, at)?;
        if new_value > u16::MAX as u32 && r.pointers.iter().any(|p| p.width == 2) {
            return Err(err(format!(
                "entry {}: a tabela usa ponteiro de 16 bits, que nao alcanca 0x{at:X} (o \
                 arquivo passaria de 64 KB) — encurte as traducoes",
                r.entry_id
            )));
        }
        out.resize(at, 0);
        out.extend_from_slice(&r.bytes);
        for &p in &r.pointers {
            write_pointer(out, p, new_value);
        }
    }
    out.resize(out.len().next_multiple_of(4), 0);

    // Auto-checagem: todo ponteiro reapontado resolve pros bytes novos.
    for r in relocations {
        for &p in &r.pointers {
            let target = read_pointer(out, p)
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
/// inicio dele (NDS, PS1), em u32 e u16. `file` = bytes logicos do arquivo;
/// as funcoes traduzem enderecos da imagem <-> do arquivo. Devolve (indice
/// da entry, ponteiros em enderecos absolutos da imagem).
pub fn file_relative_tables(
    file: &[u8],
    entries: &[TextEntry],
    abs_to_rel: impl Fn(usize) -> Option<usize>,
    rel_to_abs: impl Fn(usize) -> Option<usize>,
) -> Vec<(usize, Vec<Pointer>)> {
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
    let mut tables = find_pointer_tables(file, &FILE_U32, &starts);
    // u16 so alcanca os primeiros 64 KiB: se nem a primeira string anexada
    // (no fim do arquivo, alinhada em 4) caberia ali, nem procura.
    if file.len().next_multiple_of(4) <= u16::MAX as usize {
        for (target, pointers) in find_pointer_tables(file, &FILE_U16, &starts) {
            tables.entry(target).or_default().extend(pointers);
        }
    }
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let pointers = tables.get(&rel_of(e)?)?;
            let absolute: Option<Vec<Pointer>> = pointers
                .iter()
                .map(|p| {
                    Some(Pointer {
                        at: rel_to_abs(p.at)?,
                        width: p.width,
                    })
                })
                .collect();
            Some((i, absolute?))
        })
        .collect()
}

/// Grava os ponteiros no metadata (a entry vira realocavel): u32 em
/// `pointers`, u16 em `pointers16`. Falso se o metadata nao e objeto — ai a
/// entry segue so in-place.
pub fn mark_relocatable(entry: &mut TextEntry, pointers: &[Pointer]) -> bool {
    let Some(meta) = entry.metadata.as_object_mut() else {
        return false;
    };
    for (key, width) in [("pointers", 4), ("pointers16", 2)] {
        let at: Vec<usize> = pointers
            .iter()
            .filter(|p| p.width == width)
            .map(|p| p.at)
            .collect();
        if !at.is_empty() {
            meta.insert(key.to_string(), serde_json::json!(at));
        }
    }
    true
}

/// Nenhuma escrita in-place pode ter pisado num ponteiro de tabela: isso
/// corromperia outra string em silencio (ex.: campo de nome sem terminador
/// colado no ponteiro).
pub fn ensure_pointers_untouched(original: &[u8], out: &[u8], entries: &[TextEntry]) -> Result<()> {
    for entry in entries {
        for p in entry.pointers() {
            let end =
                p.at.checked_add(p.width)
                    .ok_or_else(|| err(format!("ponteiro invalido na entry {}", entry.id)))?;
            if out.get(p.at..end) != original.get(p.at..end) {
                return Err(err(format!(
                    "uma traducao in-place sobrescreveria o ponteiro em 0x{:X} (que aponta \
                     pra entry {}) — encurte o texto logo antes desse endereco",
                    p.at, entry.id
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
                pointers: r
                    .pointers
                    .iter()
                    .map(|p| {
                        Ok(Pointer {
                            at: map(p.at)?,
                            width: p.width,
                        })
                    })
                    .collect::<Result<_>>()?,
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

fn read_pointer(data: &[u8], p: Pointer) -> Option<u32> {
    match *data.get(p.at..p.at.checked_add(p.width)?)? {
        [a, b] => Some(u16::from_le_bytes([a, b]) as u32),
        [a, b, c, d] => Some(u32::from_le_bytes([a, b, c, d])),
        _ => None,
    }
}

/// So depois de `read_pointer` validar a posicao e do valor caber na largura.
fn write_pointer(out: &mut [u8], p: Pointer, value: u32) {
    out[p.at..p.at + p.width].copy_from_slice(&value.to_le_bytes()[..p.width]);
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u32 = 0x0800_0000;
    const ABSOLUTE: PointerFormat = PointerFormat {
        width: 4,
        base: BASE,
        min_run: 2,
        relative: false,
    };

    fn put32(buf: &mut [u8], at: usize, value: u32) {
        buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put16(buf: &mut [u8], at: usize, value: u16) {
        buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn ptr(at: usize, width: usize) -> Pointer {
        Pointer { at, width }
    }

    #[test]
    fn absolute_tables_ignore_isolated_and_mid_string_pointers() {
        let mut buf = vec![0u8; 0x80];
        let starts: HashSet<usize> = [0x40, 0x48, 0x50].into_iter().collect();
        put32(&mut buf, 0x10, BASE + 0x40); // tabela de 2
        put32(&mut buf, 0x14, BASE + 0x48);
        put32(&mut buf, 0x20, BASE + 0x50); // isolado: coincidencia possivel
        put32(&mut buf, 0x30, BASE + 0x41); // meio de string + inicio: run nao fecha
        put32(&mut buf, 0x34, BASE + 0x48);

        let tables = find_pointer_tables(&buf, &ABSOLUTE, &starts);
        assert_eq!(tables.get(&0x40), Some(&vec![ptr(0x10, 4)]));
        assert_eq!(
            tables.get(&0x48),
            Some(&vec![ptr(0x14, 4)]),
            "0x34 nao conta"
        );
        assert!(!tables.contains_key(&0x50), "ponteiro isolado nao e tabela");

        // Entrada truncada/vazia/lixo nunca panica.
        for len in 0..buf.len() {
            let _ = find_pointer_tables(&buf[..len], &ABSOLUTE, &starts);
            let _ = find_pointer_tables(&buf[..len], &FILE_U16, &starts);
        }
        assert!(find_pointer_tables(&[0xFF; 7], &ABSOLUTE, &starts).is_empty());
    }

    #[test]
    fn relative_tables_need_increasing_run_and_ignore_target_zero() {
        // String abrindo o arquivo (offset 0) + padding de zeros: sem a regra,
        // cada palavra zerada viraria ponteiro pra ela.
        let starts: HashSet<usize> = [0x00, 0x40, 0x48, 0x50, 0x58].into_iter().collect();
        let zeros = vec![0u8; 0x80];
        assert!(find_pointer_tables(&zeros, &FILE_U32, &starts).is_empty());
        assert!(find_pointer_tables(&zeros, &FILE_U16, &starts).is_empty());

        // Crescente de 3 conta; fora de ordem quebra o run.
        let mut buf = vec![0u8; 0x80];
        for (i, target) in [0x40, 0x48, 0x50].into_iter().enumerate() {
            put32(&mut buf, 0x10 + i * 4, target);
        }
        put32(&mut buf, 0x20, 0x50); // 0x50, 0x48, 0x58: nao e crescente
        put32(&mut buf, 0x24, 0x48);
        put32(&mut buf, 0x28, 0x58);
        let tables = find_pointer_tables(&buf, &FILE_U32, &starts);
        assert_eq!(tables.get(&0x40), Some(&vec![ptr(0x10, 4)]));
        assert!(
            !tables.contains_key(&0x58),
            "run fora de ordem nao e tabela"
        );

        // u16: crescente de 4 conta, de 3 nao.
        let mut buf = vec![0u8; 0x80];
        for (i, target) in [0x40u16, 0x48, 0x50, 0x58].into_iter().enumerate() {
            put16(&mut buf, 0x10 + i * 2, target);
        }
        for (i, target) in [0x40u16, 0x48, 0x50].into_iter().enumerate() {
            put16(&mut buf, 0x30 + i * 2, target);
        }
        let tables = find_pointer_tables(&buf, &FILE_U16, &starts);
        assert_eq!(tables.get(&0x40), Some(&vec![ptr(0x10, 2)]), "so a de 4");
        assert_eq!(tables.get(&0x58), Some(&vec![ptr(0x16, 2)]));
    }

    #[test]
    fn relocates_repoints_and_refuses_drift_and_overflow() {
        let mut data = vec![0u8; 0x40];
        data[0x20..0x24].copy_from_slice(b"OLD\0");
        put32(&mut data, 0x00, BASE + 0x20);
        put32(&mut data, 0x08, BASE + 0x20);
        let reloc = Relocation {
            entry_id: "e".into(),
            original_offset: 0x20,
            bytes: b"TEXTO NOVO\0".to_vec(),
            pointers: vec![ptr(0x00, 4), ptr(0x08, 4)],
        };

        let mut out = data.clone();
        relocate(&mut out, std::slice::from_ref(&reloc), BASE, 1 << 20).unwrap();
        assert_eq!(&out[0x40..0x4B], b"TEXTO NOVO\0", "anexado alinhado no fim");
        assert_eq!(out.len() % 4, 0);
        assert_eq!(read_pointer(&out, ptr(0x00, 4)), Some(BASE + 0x40));
        assert_eq!(read_pointer(&out, ptr(0x08, 4)), Some(BASE + 0x40));
        assert_eq!(&out[0x20..0x24], b"OLD\0", "original intacto");

        // Ponteiro que mudou desde a extracao: recusa.
        let mut drifted = data.clone();
        put32(&mut drifted, 0x08, BASE + 0x24);
        let e = relocate(&mut drifted, std::slice::from_ref(&reloc), BASE, 1 << 20).unwrap_err();
        assert!(e.to_string().contains("0x8"), "{e}");

        // Sem espaco enderecavel: recusa em vez de gravar ponteiro invalido.
        let mut full = data.clone();
        let e = relocate(&mut full, std::slice::from_ref(&reloc), BASE, 0x44).unwrap_err();
        assert!(e.to_string().contains("sem espaco pra realocar"), "{e}");
    }

    #[test]
    fn u16_pointers_relocate_and_refuse_past_64k() {
        let mut data = vec![0u8; 0x20];
        data[0x10..0x14].copy_from_slice(b"OLD\0");
        put16(&mut data, 0x02, 0x10);
        let reloc = Relocation {
            entry_id: "e".into(),
            original_offset: 0x10,
            bytes: b"NOVO\0".to_vec(),
            pointers: vec![ptr(0x02, 2)],
        };
        let mut out = data.clone();
        relocate(&mut out, std::slice::from_ref(&reloc), 0, 1 << 20).unwrap();
        assert_eq!(read_pointer(&out, ptr(0x02, 2)), Some(0x20));
        assert_eq!(&out[0x20..0x25], b"NOVO\0");

        // Arquivo ja passando de 64 KB: o u16 nao alcanca o fim — recusa.
        let mut big = data.clone();
        big.resize(0x1_0000, 0);
        let e = relocate(&mut big, std::slice::from_ref(&reloc), 0, 1 << 20).unwrap_err();
        assert!(e.to_string().contains("16 bits"), "{e}");
    }
}

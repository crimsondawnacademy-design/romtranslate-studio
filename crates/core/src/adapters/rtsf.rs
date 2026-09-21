//! Adapter da fixture sintetica RTSF — o primeiro com o ciclo completo da
//! Camada B (spec §25 Sprint 5): strings FIXAS (slots com limite), strings
//! RELOCAVEIS (blob + tabela de ponteiros atualizada na reinsercao) e
//! checksum recalculado. Nenhum byte de jogo real: formato proprio.
//!
//! Layout (little-endian):
//! ```text
//! 0x00  magic "RTSF"        0x04 version u8 (=1)
//! 0x05  fixed_count u8      0x06 fixed_slot_size u8   0x07 reserved u8
//! 0x08  fixed_offset u32    0x0C ptr_table_offset u32
//! 0x10  ptr_count u32       0x14 blob_offset u32
//! 0x18  blob_capacity u32   0x1C checksum u32 (soma wrapping, campo zerado)
//! ```
//! Slots fixos: ASCII null-terminated, zero-padded. Ponteiros: offsets
//! absolutos para strings null-terminated dentro do blob.

use crate::adapter::{AppliedImage, ApplyReport, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::types::{
    AdapterCapabilities, Platform, ProbeResult, SupportLevel, TextEncoding, TextEntry,
    TranslationStatus,
};

pub struct RtsfAdapter;

pub const MAGIC: &[u8; 4] = b"RTSF";
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 0x20;
const CHECKSUM_OFFSET: usize = 0x1C;

#[derive(Debug, Clone, Copy)]
struct Layout {
    fixed_count: usize,
    slot_size: usize,
    fixed_offset: usize,
    ptr_offset: usize,
    ptr_count: usize,
    blob_offset: usize,
    blob_capacity: usize,
    checksum: u32,
}

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("rtsf: {}", msg.into()))
}

fn read_u32(data: &[u8], off: usize) -> Result<u32> {
    let bytes = data
        .get(off..off + 4)
        .ok_or_else(|| err(format!("header truncado em 0x{off:X}")))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

/// Regiao [off, off+len) dentro do arquivo, com aritmetica checada.
fn check_region(data: &[u8], off: usize, len: usize, what: &str) -> Result<()> {
    let end = off
        .checked_add(len)
        .ok_or_else(|| err(format!("{what}: overflow de offset")))?;
    if end > data.len() {
        return Err(err(format!(
            "{what}: regiao 0x{off:X}+{len} passa do fim do arquivo ({})",
            data.len()
        )));
    }
    Ok(())
}

fn parse_layout(data: &[u8]) -> Result<Layout> {
    if data.len() < HEADER_LEN || &data[0..4] != MAGIC {
        return Err(err("magic RTSF ausente"));
    }
    if data[4] != VERSION {
        return Err(err(format!("versao {} nao suportada", data[4])));
    }
    let layout = Layout {
        fixed_count: data[5] as usize,
        slot_size: data[6] as usize,
        fixed_offset: read_u32(data, 0x08)? as usize,
        ptr_offset: read_u32(data, 0x0C)? as usize,
        ptr_count: read_u32(data, 0x10)? as usize,
        blob_offset: read_u32(data, 0x14)? as usize,
        blob_capacity: read_u32(data, 0x18)? as usize,
        checksum: read_u32(data, CHECKSUM_OFFSET)?,
    };
    if layout.fixed_count > 0 && layout.slot_size < 2 {
        return Err(err("slot fixo menor que 2 bytes"));
    }
    if layout.ptr_count > 10_000 {
        return Err(err("ptr_count absurdo (header corrompido?)"));
    }
    let fixed_len = layout
        .fixed_count
        .checked_mul(layout.slot_size)
        .ok_or_else(|| err("fixed_count*slot_size overflow"))?;
    check_region(data, layout.fixed_offset, fixed_len, "secao fixa")?;
    let ptr_len = layout
        .ptr_count
        .checked_mul(4)
        .ok_or_else(|| err("ptr_count overflow"))?;
    check_region(data, layout.ptr_offset, ptr_len, "tabela de ponteiros")?;
    check_region(data, layout.blob_offset, layout.blob_capacity, "blob")?;
    Ok(layout)
}

/// Soma wrapping de todos os bytes com o campo de checksum zerado.
pub fn compute_checksum(data: &[u8]) -> u32 {
    data.iter().enumerate().fold(0u32, |acc, (i, &b)| {
        if (CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4).contains(&i) {
            acc
        } else {
            acc.wrapping_add(b as u32)
        }
    })
}

/// String ASCII null-terminated a partir de `off`, limitada a `max`.
fn read_cstr(data: &[u8], off: usize, max: usize) -> Result<String> {
    let region = data
        .get(off..(off + max).min(data.len()))
        .ok_or_else(|| err(format!("string fora do arquivo em 0x{off:X}")))?;
    let end = region.iter().position(|&b| b == 0).unwrap_or(region.len());
    let bytes = &region[..end];
    if !bytes.iter().all(|&b| (0x20..0x7F).contains(&b)) {
        return Err(err(format!("bytes nao-ASCII na string em 0x{off:X}")));
    }
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn entry_kind(entry: &TextEntry) -> Option<(&str, u64)> {
    let kind = entry.metadata.get("kind")?.as_str()?;
    let index = entry.metadata.get("index")?.as_u64()?;
    Some((kind, index))
}

fn encode_ascii(entry_id: &str, text: &str, max: usize) -> Result<Vec<u8>> {
    if !text.is_ascii() {
        return Err(err(format!(
            "entry {entry_id}: traducao tem caracteres fora de ASCII"
        )));
    }
    let bytes = text.as_bytes();
    if bytes.len() > max {
        return Err(err(format!(
            "entry {entry_id}: traducao ocupa {} bytes; maximo {max} — encurte o texto",
            bytes.len()
        )));
    }
    Ok(bytes.to_vec())
}

impl GameAdapter for RtsfAdapter {
    fn id(&self) -> &'static str {
        "synthetic.rtsf"
    }

    fn display_name(&self) -> &'static str {
        "RTSF Synthetic Fixture"
    }

    fn platform(&self) -> Platform {
        Platform::Synthetic
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            detect: true,
            extract: true,
            reinsert: true,
            patch: true,
            compression: false,
            pointer_relocation: true,
            font_table: false,
            experimental: false,
            support_level: SupportLevel::Full,
        }
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        let head = &input.head;
        if head.len() < HEADER_LEN || &head[0..4] != MAGIC {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let mut evidence = vec!["magic RTSF presente".to_string()];
        let mut confidence = 0.7;
        match parse_layout(head) {
            Ok(layout) => {
                confidence = 0.95;
                evidence.push(format!(
                    "estrutura valida: {} strings fixas, {} relocaveis",
                    layout.fixed_count, layout.ptr_count
                ));
                // Checksum cobre o arquivo inteiro; so da pra conferir com ele todo no head.
                if input.size as usize <= head.len() {
                    if compute_checksum(&head[..input.size as usize]) == layout.checksum {
                        confidence = 0.99;
                        evidence.push("checksum do arquivo valido".to_string());
                    } else {
                        evidence.push("checksum NAO bate (arquivo modificado?)".to_string());
                    }
                }
            }
            Err(e) => evidence.push(format!("estrutura invalida: {e}")),
        }
        ProbeResult {
            confidence,
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }

    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        let layout = parse_layout(data)?;
        let mut entries = Vec::new();

        for i in 0..layout.fixed_count {
            let off = layout.fixed_offset + i * layout.slot_size;
            let text = read_cstr(data, off, layout.slot_size)?;
            entries.push(TextEntry {
                id: format!("fixed-{i}"),
                resource_path: None,
                offset: Some(off as u64),
                original_bytes: text.as_bytes().to_vec(),
                source_text: text,
                translated_text: None,
                context: Some("campo de tamanho fixo".to_string()),
                // 1 byte reservado para o terminador.
                max_bytes: Some(layout.slot_size - 1),
                encoding: TextEncoding::Ascii,
                status: TranslationStatus::Untranslated,
                metadata: serde_json::json!({"kind": "fixed", "index": i}),
            });
        }

        for i in 0..layout.ptr_count {
            let ptr = read_u32(data, layout.ptr_offset + i * 4)? as usize;
            if ptr < layout.blob_offset || ptr >= layout.blob_offset + layout.blob_capacity {
                return Err(err(format!("ponteiro {i} aponta fora do blob (0x{ptr:X})")));
            }
            let max = layout.blob_offset + layout.blob_capacity - ptr;
            let text = read_cstr(data, ptr, max)?;
            entries.push(TextEntry {
                id: format!("reloc-{i}"),
                resource_path: None,
                offset: Some(ptr as u64),
                original_bytes: text.as_bytes().to_vec(),
                source_text: text,
                translated_text: None,
                context: Some("string relocavel (ponteiro atualizado na reinsercao)".to_string()),
                max_bytes: None, // relocavel: limite e a capacidade total do blob
                encoding: TextEncoding::Ascii,
                status: TranslationStatus::Untranslated,
                metadata: serde_json::json!({"kind": "relocatable", "index": i}),
            });
        }
        Ok(entries)
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        let layout = parse_layout(data)?;
        let mut out = data.to_vec();
        let mut report = ApplyReport::default();

        // Textos finais das relocaveis: comeca com o conteudo atual do arquivo.
        let current = self.extract_structured(data)?;
        let mut reloc_texts: Vec<String> = current
            .iter()
            .filter(|e| entry_kind(e).is_some_and(|(k, _)| k == "relocatable"))
            .map(|e| e.source_text.clone())
            .collect();

        for entry in entries {
            let Some((kind, index)) = entry_kind(entry) else {
                report.ignored_generic += 1;
                continue;
            };
            let index = index as usize;
            let Some(translation) = entry.translated_text.as_deref() else {
                report.kept_original += 1;
                continue;
            };
            match kind {
                "fixed" => {
                    if index >= layout.fixed_count {
                        return Err(err(format!("entry {}: slot {index} nao existe", entry.id)));
                    }
                    let bytes = encode_ascii(&entry.id, translation, layout.slot_size - 1)?;
                    let off = layout.fixed_offset + index * layout.slot_size;
                    out[off..off + layout.slot_size].fill(0);
                    out[off..off + bytes.len()].copy_from_slice(&bytes);
                    report.applied += 1;
                }
                "relocatable" => {
                    if index >= reloc_texts.len() {
                        return Err(err(format!(
                            "entry {}: ponteiro {index} nao existe",
                            entry.id
                        )));
                    }
                    if !translation.is_ascii() {
                        return Err(err(format!(
                            "entry {}: traducao tem caracteres fora de ASCII",
                            entry.id
                        )));
                    }
                    reloc_texts[index] = translation.to_string();
                    report.applied += 1;
                    report.relocated += 1;
                }
                other => {
                    return Err(err(format!(
                        "entry {}: kind desconhecido \"{other}\"",
                        entry.id
                    )));
                }
            }
        }

        // Reescreve o blob inteiro e a tabela de ponteiros.
        let needed: usize = reloc_texts.iter().map(|t| t.len() + 1).sum();
        if needed > layout.blob_capacity {
            return Err(err(format!(
                "textos relocaveis ocupam {needed} bytes; capacidade do blob e {} — encurte as traducoes",
                layout.blob_capacity
            )));
        }
        out[layout.blob_offset..layout.blob_offset + layout.blob_capacity].fill(0);
        let mut cursor = layout.blob_offset;
        for (i, text) in reloc_texts.iter().enumerate() {
            out[cursor..cursor + text.len()].copy_from_slice(text.as_bytes());
            let ptr_pos = layout.ptr_offset + i * 4;
            out[ptr_pos..ptr_pos + 4].copy_from_slice(&(cursor as u32).to_le_bytes());
            cursor += text.len() + 1; // terminador ja e zero pelo fill
        }

        // Checksum do arquivo novo.
        let checksum = compute_checksum(&out);
        out[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4].copy_from_slice(&checksum.to_le_bytes());

        Ok(AppliedImage { bytes: out, report })
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        let mut checks = Vec::new();
        let mut problems = Vec::new();

        match parse_layout(data) {
            Ok(layout) => {
                checks.push("header e regioes dentro dos limites".to_string());
                let checksum = compute_checksum(data);
                if checksum == layout.checksum {
                    checks.push("checksum valido".to_string());
                } else {
                    problems.push(format!(
                        "checksum invalido: header 0x{:08X}, calculado 0x{checksum:08X}",
                        layout.checksum
                    ));
                }
                match self.extract_structured(data) {
                    Ok(entries) => checks.push(format!(
                        "{} strings re-extraidas com sucesso",
                        entries.len()
                    )),
                    Err(e) => problems.push(format!("re-extracao falhou: {e}")),
                }
            }
            Err(e) => problems.push(format!("estrutura invalida: {e}")),
        }

        Ok(VerificationReport {
            ok: problems.is_empty(),
            checks,
            problems,
        })
    }
}

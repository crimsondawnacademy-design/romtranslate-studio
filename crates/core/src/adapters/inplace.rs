//! Reinsercao IN-PLACE compartilhada (padrao do GBA, reusado pelo NDS):
//! cada traducao ocupa exatamente o espaco da string original (mesmo tamanho
//! ou menor), sem relocacao. All-or-nothing; `finalize` recalcula o que o
//! formato exigir (checksums de header) depois de todas as escritas.

use crate::adapter::{AppliedImage, ApplyReport};
use crate::error::{CoreError, Result};
use crate::types::{TextEncoding, TextEntry};

/// Serializa a traducao no encoding da entry. `None` = encoding sem encoder
/// in-place (tabela custom, Shift-JIS) — o caller decide errar ou ignorar.
fn encode(entry_id: &str, text: &str, encoding: &TextEncoding) -> Result<Option<Vec<u8>>> {
    match encoding {
        TextEncoding::Ascii => {
            if !text.is_ascii() {
                return Err(CoreError::Project(format!(
                    "entry {entry_id}: traducao tem caracteres fora de ASCII"
                )));
            }
            Ok(Some(text.as_bytes().to_vec()))
        }
        TextEncoding::Utf8 => Ok(Some(text.as_bytes().to_vec())),
        TextEncoding::Utf16Le => Ok(Some(
            text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect(),
        )),
        TextEncoding::Utf16Be => Ok(Some(
            text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect(),
        )),
        TextEncoding::ShiftJis | TextEncoding::Table(_) => Ok(None),
    }
}

/// Plano de escrita in-place: pares (offset, bytes ja com padding) validados
/// contra a imagem original — quem aplica decide se e num Vec em memoria
/// (`apply_in_place`) ou direto num arquivo (reinsercao streaming).
pub struct InPlacePlan {
    pub writes: Vec<(usize, Vec<u8>)>,
    pub report: ApplyReport,
}

/// Valida e monta o plano de escritas. Regras:
/// - sanity anti-drift: os bytes atuais precisam ser identicos a `original_bytes`;
/// - traducao serializada tem que caber no espaco original;
/// - sobra preenchida com terminador (0x00) se o run original era terminado,
///   senao espaco (fixed-width) — em ASCII/UTF-8; UTF-16 sempre 0x00 (par).
pub fn plan_in_place(data: &[u8], entries: &[TextEntry]) -> Result<InPlacePlan> {
    let mut writes = Vec::new();
    let mut report = ApplyReport {
        applied: 0,
        kept_original: 0,
        ignored_generic: 0,
    };

    for entry in entries {
        let Some(offset) = entry.offset.map(|o| o as usize) else {
            report.ignored_generic += 1;
            continue;
        };
        let slot = entry.original_bytes.len();
        let Some(translation) = entry.translated_text.as_deref() else {
            report.kept_original += 1;
            continue;
        };
        let end = offset
            .checked_add(slot)
            .filter(|&e| e <= data.len())
            .ok_or_else(|| {
                CoreError::Project(format!(
                    "entry {} aponta para fora do arquivo (0x{offset:X})",
                    entry.id
                ))
            })?;
        if data[offset..end] != entry.original_bytes[..] {
            return Err(CoreError::Project(format!(
                "bytes em 0x{offset:X} nao batem com a entry {} — arquivo diferente do que \
                 foi extraido? Re-extraia antes de reinserir",
                entry.id
            )));
        }
        let Some(bytes) = encode(&entry.id, translation, &entry.encoding)? else {
            report.ignored_generic += 1;
            continue;
        };
        if bytes.len() > slot {
            return Err(CoreError::Project(format!(
                "entry {}: traducao ocupa {} bytes; o espaco original tem {slot} — encurte \
                 o texto (reinsercao conservadora nao realoca)",
                entry.id,
                bytes.len()
            )));
        }
        let terminated = entry
            .metadata
            .get("terminated")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let is_utf16 = matches!(
            entry.encoding,
            TextEncoding::Utf16Le | TextEncoding::Utf16Be
        );
        let pad = if terminated || is_utf16 { 0x00 } else { 0x20 };
        let mut patch = vec![pad; slot];
        patch[..bytes.len()].copy_from_slice(&bytes);
        writes.push((offset, patch));
        report.applied += 1;
    }

    Ok(InPlacePlan { writes, report })
}

/// Aplica o plano numa copia em memoria; `finalize` roda por ultimo
/// (recalculo de checksums do formato).
pub fn apply_in_place(
    data: &[u8],
    entries: &[TextEntry],
    finalize: impl FnOnce(&mut [u8]),
) -> Result<AppliedImage> {
    let plan = plan_in_place(data, entries)?;
    let mut out = data.to_vec();
    for (offset, patch) in &plan.writes {
        out[*offset..offset + patch.len()].copy_from_slice(patch);
    }
    finalize(&mut out);
    Ok(AppliedImage {
        bytes: out,
        report: plan.report,
    })
}

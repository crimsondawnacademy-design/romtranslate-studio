//! Export de entries extraidas (spec §25 Sprint 2: JSON/CSV).

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::error::{CoreError, Result};
use crate::types::{TextEncoding, TextEntry};

pub fn export_json(entries: &[TextEntry], path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(entries)?;
    write_atomic(path, json.as_bytes())
}

pub fn export_csv(entries: &[TextEntry], path: &Path) -> Result<()> {
    let mut out = String::from("id,offset,encoding,byte_length,text\n");
    for e in entries {
        let offset = e.offset.map(|o| format!("0x{o:08X}")).unwrap_or_default();
        let _ = writeln!(
            out,
            "{},{},{},{},{}",
            csv_field(&e.id),
            offset,
            csv_field(&encoding_label(&e.encoding)),
            e.original_bytes.len(),
            csv_field(&e.source_text),
        );
    }
    write_atomic(path, out.as_bytes())
}

pub fn encoding_label(encoding: &TextEncoding) -> String {
    match encoding {
        TextEncoding::Ascii => "ascii".to_string(),
        TextEncoding::Utf8 => "utf8".to_string(),
        TextEncoding::Utf16Le => "utf16le".to_string(),
        TextEncoding::Utf16Be => "utf16be".to_string(),
        TextEncoding::ShiftJis => "shift_jis".to_string(),
        TextEncoding::Table(id) => format!("table:{id}"),
        TextEncoding::Image => "image".to_string(),
    }
}

/// Escaping RFC 4180: aspas dobradas; campo com virgula/aspas/quebra vai entre aspas.
fn csv_field(s: &str) -> String {
    if s.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| CoreError::io(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| CoreError::io(path, e))?;
    Ok(())
}

/// Grava as traducoes de um arquivo TSV (`id<TAB>traducao`, uma por linha),
/// como traducao de maquina — o caminho de quem traduziu fora do app. O
/// arquivo inteiro e conferido antes de gravar a primeira linha: TSV quebrado
/// nao deixa o projeto traduzido pela metade. Devolve quantas gravou.
pub fn import_tsv(db: &mut crate::db::ProjectDb, path: &Path) -> Result<usize> {
    let content = fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
    let mut rows = Vec::new();
    for (index, line) in content.trim_start_matches('\u{feff}').lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let at = format!("{}, linha {}", path.display(), index + 1);
        // ponytail: uma linha, uma frase, sem escape — traducao com quebra de
        // linha de verdade nao cabe neste formato. Se um jogo pedir, o caminho
        // e importar o JSON do export.
        let (id, translation) = line
            .split_once('\t')
            .ok_or_else(|| CoreError::Project(format!("{at}: esperado id<TAB>traducao")))?;
        if db.get_entry(id)?.is_none() {
            return Err(CoreError::Project(format!(
                "{at}: o projeto nao tem a frase {id}"
            )));
        }
        rows.push((id, translation));
    }
    for (id, translation) in &rows {
        db.update_translation(id, translation, crate::types::TranslationStatus::Machine)?;
    }
    Ok(rows.len())
}

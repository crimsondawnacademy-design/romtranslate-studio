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

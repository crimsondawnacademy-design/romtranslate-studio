//! Camada A da extracao (spec §10): scanner generico de strings.
//! Serve para DESCOBERTA e debug — nao garante reinsercao segura (isso e papel
//! dos adapters estruturados, Camada B+). Deterministico: mesmo arquivo + mesma
//! config => mesmas entries, na mesma ordem.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::{CoreError, Result};
use crate::tbl::TblTable;
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

// ponytail: scan carrega o arquivo inteiro em memoria; 64 MiB cobre qualquer
// cartucho (GBA max 32 MiB). Scan streaming entra com plataformas de disco.
pub const MAX_SCAN_FILE_SIZE: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanEncoding {
    Ascii,
    Utf8,
    Utf16Le,
    Utf16Be,
    Table,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanConfig {
    pub encoding: ScanEncoding,
    /// Obrigatorio quando `encoding == Table`.
    pub tbl_path: Option<PathBuf>,
    /// Minimo de CARACTERES (nao bytes) para virar entry.
    pub min_chars: usize,
    /// Regiao de busca em bytes; None = arquivo inteiro.
    pub region_start: Option<u64>,
    pub region_end: Option<u64>,
    /// Corte de seguranca para a UI nao afogar.
    pub max_entries: usize,
}

impl Default for ScanConfig {
    fn default() -> Self {
        ScanConfig {
            encoding: ScanEncoding::Ascii,
            tbl_path: None,
            min_chars: 4,
            region_start: None,
            region_end: None,
            max_entries: 20_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub entries: Vec<TextEntry>,
    /// true = parou em max_entries; ha mais strings alem das retornadas.
    pub truncated: bool,
    pub scanned_bytes: u64,
}

pub fn scan_file(path: &Path, config: &ScanConfig) -> Result<ScanOutcome> {
    let size = fs::metadata(path)
        .map_err(|e| CoreError::io(path, e))?
        .len();
    if size > MAX_SCAN_FILE_SIZE {
        return Err(CoreError::FileTooLarge {
            size,
            limit: MAX_SCAN_FILE_SIZE,
        });
    }
    let data = fs::read(path).map_err(|e| CoreError::io(path, e))?;
    let outcome = scan_bytes(&data, config)?;
    info!(
        path = %path.display(),
        encoding = ?config.encoding,
        entries = outcome.entries.len(),
        truncated = outcome.truncated,
        "scan concluido"
    );
    Ok(outcome)
}

pub fn scan_bytes(data: &[u8], config: &ScanConfig) -> Result<ScanOutcome> {
    let len = data.len() as u64;
    let start = config.region_start.unwrap_or(0).min(len) as usize;
    let end = config.region_end.unwrap_or(len).min(len) as usize;
    if start > end {
        return Err(CoreError::Project(format!(
            "regiao invalida: inicio 0x{start:X} > fim 0x{end:X}"
        )));
    }
    let window = &data[start..end];

    let table;
    let runs = match config.encoding {
        ScanEncoding::Ascii => ascii_runs(window),
        ScanEncoding::Utf8 => utf8_runs(window),
        ScanEncoding::Utf16Le => utf16_runs(window, true),
        ScanEncoding::Utf16Be => utf16_runs(window, false),
        ScanEncoding::Table => {
            let tbl_path = config.tbl_path.as_deref().ok_or_else(|| {
                CoreError::Tbl("encoding 'table' exige o caminho de um arquivo .tbl".to_string())
            })?;
            table = TblTable::load(tbl_path)?;
            table_runs(window, &table)
        }
    };

    let text_encoding = |cfg: &ScanConfig| match cfg.encoding {
        ScanEncoding::Ascii => TextEncoding::Ascii,
        ScanEncoding::Utf8 => TextEncoding::Utf8,
        ScanEncoding::Utf16Le => TextEncoding::Utf16Le,
        ScanEncoding::Utf16Be => TextEncoding::Utf16Be,
        ScanEncoding::Table => TextEncoding::Table(
            cfg.tbl_path
                .as_deref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "tbl".to_string()),
        ),
    };

    let mut entries = Vec::new();
    let mut truncated = false;
    for run in runs {
        if run.chars < config.min_chars {
            continue;
        }
        if entries.len() >= config.max_entries {
            truncated = true;
            break;
        }
        let abs = start as u64 + run.offset as u64;
        entries.push(TextEntry {
            id: format!("scan-{abs:08x}"),
            resource_path: None,
            offset: Some(abs),
            original_bytes: window[run.offset..run.offset + run.byte_len].to_vec(),
            source_text: run.text,
            translated_text: None,
            context: None,
            // Scanner generico nao garante limite de reinsercao (Camada B decide).
            max_bytes: None,
            encoding: text_encoding(config),
            status: TranslationStatus::Untranslated,
            metadata: serde_json::json!({
                "scanner": config.encoding,
                "chars": run.chars,
                "terminated": run.terminated,
            }),
        });
    }

    Ok(ScanOutcome {
        entries,
        truncated,
        scanned_bytes: (end - start) as u64,
    })
}

/// Run bruto achado por um scanner, relativo ao inicio da janela.
struct Run {
    offset: usize,
    byte_len: usize,
    chars: usize,
    text: String,
    /// true se o byte seguinte ao run e 0x00 (terminador classico).
    terminated: bool,
}

fn is_ascii_printable(b: u8) -> bool {
    (0x20..0x7F).contains(&b)
}

/// Texto "plausivel" para scanners unicode: nada de controle, nada de
/// private-use (lixo comum em falso positivo), nada de replacement char e
/// nenhum noncharacter (U+FDD0..=U+FDEF e os xxFFFE/xxFFFF de cada plano —
/// padding 0xFFFF de ROM cai aqui).
fn is_text_char(c: char) -> bool {
    let cp = c as u32;
    !c.is_control()
        && c != '\u{FFFD}'
        && !('\u{E000}'..='\u{F8FF}').contains(&c)
        && !(0xFDD0..=0xFDEF).contains(&cp)
        && (cp & 0xFFFE) != 0xFFFE
}

fn ascii_runs(data: &[u8]) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if !is_ascii_printable(data[i]) {
            i += 1;
            continue;
        }
        let begin = i;
        while i < data.len() && is_ascii_printable(data[i]) {
            i += 1;
        }
        let bytes = &data[begin..i];
        runs.push(Run {
            offset: begin,
            byte_len: bytes.len(),
            chars: bytes.len(),
            text: String::from_utf8_lossy(bytes).into_owned(),
            terminated: data.get(i) == Some(&0),
        });
    }
    runs
}

/// Decodifica um char UTF-8 valido em `data[i..]` (from_utf8 rejeita overlong
/// e surrogates por nos).
fn decode_utf8_char(data: &[u8], i: usize) -> Option<(char, usize)> {
    let first = *data.get(i)?;
    let len = match first {
        0x20..=0x7E => 1, // ASCII printable direto; controles nao comecam run
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return None,
    };
    let slice = data.get(i..i + len)?;
    let c = std::str::from_utf8(slice).ok()?.chars().next()?;
    is_text_char(c).then_some((c, len))
}

fn utf8_runs(data: &[u8]) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let Some((c, clen)) = decode_utf8_char(data, i) else {
            i += 1;
            continue;
        };
        let begin = i;
        let mut text = String::new();
        let mut chars = 0;
        let mut cur = (c, clen);
        loop {
            text.push(cur.0);
            chars += 1;
            i += cur.1;
            match decode_utf8_char(data, i) {
                Some(next) => cur = next,
                None => break,
            }
        }
        runs.push(Run {
            offset: begin,
            byte_len: i - begin,
            chars,
            text,
            terminated: data.get(i) == Some(&0),
        });
    }
    runs
}

fn utf16_runs(data: &[u8], le: bool) -> Vec<Run> {
    let unit = |i: usize| -> Option<u16> {
        let a = *data.get(i)?;
        let b = *data.get(i + 1)?;
        Some(if le {
            u16::from_le_bytes([a, b])
        } else {
            u16::from_be_bytes([a, b])
        })
    };
    // Decodifica um char (com par surrogate) em unidades de 2 bytes.
    let decode = |i: usize| -> Option<(char, usize)> {
        let u = unit(i)?;
        match u {
            0xD800..=0xDBFF => {
                let lo = unit(i + 2)?;
                if !(0xDC00..=0xDFFF).contains(&lo) {
                    return None;
                }
                let cp = 0x10000 + (((u as u32 - 0xD800) << 10) | (lo as u32 - 0xDC00));
                char::from_u32(cp)
                    .filter(|c| is_text_char(*c))
                    .map(|c| (c, 4))
            }
            0xDC00..=0xDFFF => None, // low surrogate solto
            _ => char::from_u32(u as u32)
                .filter(|c| is_text_char(*c))
                .map(|c| (c, 2)),
        }
    };

    let mut runs = Vec::new();
    let mut i = 0;
    // Alinhamento fixo em offsets pares — strings UTF-16 em ROMs sao alinhadas.
    while i + 1 < data.len() {
        let Some((c, clen)) = decode(i) else {
            i += 2;
            continue;
        };
        let begin = i;
        let mut text = String::new();
        let mut chars = 0;
        let mut cur = (c, clen);
        loop {
            text.push(cur.0);
            chars += 1;
            i += cur.1;
            match decode(i) {
                Some(next) => cur = next,
                None => break,
            }
        }
        runs.push(Run {
            offset: begin,
            byte_len: i - begin,
            chars,
            text,
            terminated: unit(i) == Some(0),
        });
    }
    runs
}

fn table_runs(data: &[u8], table: &TblTable) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let Some((klen, piece)) = table.lookup(&data[i..]) else {
            i += 1;
            continue;
        };
        let begin = i;
        let mut text = String::new();
        let mut cur = (klen, piece);
        loop {
            text.push_str(cur.1);
            i += cur.0;
            match table.lookup(&data[i..]) {
                Some(next) => cur = next,
                None => break,
            }
        }
        runs.push(Run {
            offset: begin,
            byte_len: i - begin,
            chars: text.chars().count(),
            text,
            terminated: data.get(i) == Some(&0),
        });
    }
    runs
}

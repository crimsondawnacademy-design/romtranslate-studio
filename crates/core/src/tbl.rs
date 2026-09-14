//! Loader de tabelas de caracteres `.tbl` (formato classico de romhacking):
//! uma entrada por linha, `HEX=texto`, com chaves de 1 a 4 bytes.
//! Linhas vazias e comentarios (`#` ou `;`) sao ignorados.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::{CoreError, Result};

#[derive(Debug, Clone)]
pub struct TblTable {
    entries: HashMap<Vec<u8>, String>,
    max_key_len: usize,
}

impl TblTable {
    pub fn parse(text: &str) -> Result<Self> {
        let mut entries = HashMap::new();
        let mut max_key_len = 0;
        for (idx, raw) in text.lines().enumerate() {
            let line = raw.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let n = idx + 1;
            let Some((hex, value)) = line.split_once('=') else {
                return Err(CoreError::Tbl(format!("linha {n}: falta '=' ({line})")));
            };
            let key = decode_hex(hex).ok_or_else(|| {
                CoreError::Tbl(format!(
                    "linha {n}: chave hex invalida \"{hex}\" (esperado 2-8 digitos hex)"
                ))
            })?;
            max_key_len = max_key_len.max(key.len());
            // Chave duplicada: a ultima vence (comum em .tbl reais).
            entries.insert(key, value.to_string());
        }
        if entries.is_empty() {
            return Err(CoreError::Tbl("tabela vazia".to_string()));
        }
        Ok(TblTable {
            entries,
            max_key_len,
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
        Self::parse(&text)
    }

    /// Match mais longo no inicio de `bytes` (greedy longest-match).
    pub fn lookup(&self, bytes: &[u8]) -> Option<(usize, &str)> {
        let cap = self.max_key_len.min(bytes.len());
        for len in (1..=cap).rev() {
            if let Some(text) = self.entries.get(&bytes[..len]) {
                return Some((len, text.as_str()));
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    let hex = hex.trim();
    if hex.is_empty() || !hex.len().is_multiple_of(2) || hex.len() > 8 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

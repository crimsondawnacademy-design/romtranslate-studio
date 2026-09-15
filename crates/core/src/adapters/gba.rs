//! Adapter de Game Boy Advance (GBATEK: cartridge header em 0x00-0xBF).
//!
//! Deteccao sem embutir o logo Nintendo (material de terceiros): byte fixo
//! 0x96 em 0xB2, header checksum em 0xBD, pistas fracas (branch ARM, titulo).
//!
//! Reinsercao CONSERVADORA (Experimental): cada string ASCII descoberta e
//! traduzida in-place no espaco que ja ocupa (mesmo tamanho ou menor), sem
//! mexer em ponteiros — cobre menus/textos curtos de muitos jogos. O header
//! checksum e recalculado (traduzir o titulo em 0xA0 o afetaria).

use crate::adapter::{AppliedImage, ApplyReport, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::scan::{scan_bytes, ScanConfig, ScanEncoding};
use crate::types::{AdapterCapabilities, Platform, ProbeResult, SupportLevel, TextEntry};

pub struct GbaAdapter;

const HEADER_LEN: usize = 0xC0;
const FIXED_VALUE_OFFSET: usize = 0xB2;
const CHECKSUM_OFFSET: usize = 0xBD;
const TITLE_RANGE: std::ops::Range<usize> = 0xA0..0xAC;

/// Checksum do header GBA: soma negativa de 0xA0..=0xBC menos 0x19 (mod 256).
pub fn header_checksum(header: &[u8]) -> u8 {
    let mut chk: u8 = 0;
    for &b in &header[0xA0..=0xBC] {
        chk = chk.wrapping_sub(b);
    }
    chk.wrapping_sub(0x19)
}

impl GameAdapter for GbaAdapter {
    fn id(&self) -> &'static str {
        "gba.generic"
    }

    fn display_name(&self) -> &'static str {
        "GBA Generic"
    }

    fn platform(&self) -> Platform {
        Platform::Gba
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            detect: true,
            extract: true,
            reinsert: true, // conservadora: in-place, sem relocacao
            patch: true,
            compression: false,
            pointer_relocation: false,
            font_table: false,
            experimental: true,
            support_level: SupportLevel::Experimental,
        }
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        let head = &input.head;
        if head.len() < HEADER_LEN {
            return ProbeResult::no_match(self.id(), self.platform());
        }

        let mut evidence = Vec::new();
        let fixed_ok = head[FIXED_VALUE_OFFSET] == 0x96;
        let checksum_ok = header_checksum(head) == head[CHECKSUM_OFFSET];

        let mut confidence: f32 = match (fixed_ok, checksum_ok) {
            (true, true) => 0.9,
            (true, false) => 0.5,
            (false, true) => 0.2, // checksum bater por acaso sem o byte fixo e suspeito
            (false, false) => 0.0,
        };

        if fixed_ok {
            evidence.push("byte fixo 0x96 presente em 0xB2".to_string());
        }
        if checksum_ok {
            evidence.push("header checksum (0xBD) valido".to_string());
        } else if fixed_ok {
            evidence.push("header checksum invalido".to_string());
        }

        if confidence > 0.0 {
            if head[3] == 0xEA {
                confidence += 0.05;
                evidence.push("entry point e branch ARM (0xEA)".to_string());
            }
            let title = &head[TITLE_RANGE];
            let printable = title.iter().all(|&b| b == 0 || (0x20..0x7F).contains(&b));
            if printable {
                confidence += 0.04;
                evidence.push("titulo do cartucho em ASCII valido".to_string());
            }
        }

        ProbeResult {
            confidence: confidence.min(1.0),
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }

    /// Extracao conservadora: scan ASCII com `max_bytes` = espaco que a string
    /// ja ocupa. Ids identicos aos do scanner generico ("scan-<offset>"), entao
    /// rodar os dois nao duplica entries no projeto.
    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        if data.len() < HEADER_LEN {
            return Err(CoreError::Project(
                "gba: arquivo menor que o header do cartucho".to_string(),
            ));
        }
        let outcome = scan_bytes(
            data,
            &ScanConfig {
                encoding: ScanEncoding::Ascii,
                ..ScanConfig::default()
            },
        )?;
        let mut entries = outcome.entries;
        for e in entries.iter_mut() {
            e.max_bytes = Some(e.original_bytes.len());
            e.context = Some("in-place: traducao limitada ao espaco original".to_string());
        }
        Ok(entries)
    }

    /// Escreve cada traducao EXATAMENTE sobre os bytes da string original
    /// (sanity check anti-drift), preenchendo a sobra com o padding adequado,
    /// e recalcula o header checksum. All-or-nothing.
    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        if data.len() < HEADER_LEN {
            return Err(CoreError::Project(
                "gba: arquivo menor que o header do cartucho".to_string(),
            ));
        }
        let mut out = data.to_vec();
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
                .filter(|&e| e <= out.len())
                .ok_or_else(|| {
                    CoreError::Project(format!(
                        "gba: entry {} aponta para fora do arquivo (0x{offset:X})",
                        entry.id
                    ))
                })?;
            if out[offset..end] != entry.original_bytes[..] {
                return Err(CoreError::Project(format!(
                    "gba: bytes em 0x{offset:X} nao batem com a entry {} — arquivo diferente \
                     do que foi extraido? Re-extraia antes de reinserir",
                    entry.id
                )));
            }
            if !translation.is_ascii() {
                return Err(CoreError::Project(format!(
                    "gba: entry {}: traducao tem caracteres fora de ASCII",
                    entry.id
                )));
            }
            let bytes = translation.as_bytes();
            if bytes.len() > slot {
                return Err(CoreError::Project(format!(
                    "gba: entry {}: traducao ocupa {} bytes; o espaco original tem {slot} — \
                     encurte o texto (reinsercao conservadora nao realoca)",
                    entry.id,
                    bytes.len()
                )));
            }
            // Sobra do slot: 0x00 se a string original era null-terminated
            // (leitores por terminador param antes); espaco se era fixed-width.
            let terminated = entry
                .metadata
                .get("terminated")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            out[offset..end].fill(if terminated { 0x00 } else { 0x20 });
            out[offset..offset + bytes.len()].copy_from_slice(bytes);
            report.applied += 1;
        }

        // Traducao do titulo (0xA0..0xAC) muda o header checksum: recalcula sempre.
        out[CHECKSUM_OFFSET] = header_checksum(&out);

        Ok(AppliedImage { bytes: out, report })
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        let mut checks = Vec::new();
        let mut problems = Vec::new();

        if data.len() < HEADER_LEN {
            problems.push("arquivo menor que o header GBA".to_string());
        } else {
            if data[FIXED_VALUE_OFFSET] == 0x96 {
                checks.push("byte fixo 0x96 preservado".to_string());
            } else {
                problems.push("byte fixo 0x96 corrompido".to_string());
            }
            if header_checksum(data) == data[CHECKSUM_OFFSET] {
                checks.push("header checksum valido".to_string());
            } else {
                problems.push("header checksum invalido".to_string());
            }
            match self.extract_structured(data) {
                Ok(entries) => checks.push(format!("{} strings re-extraidas", entries.len())),
                Err(e) => problems.push(format!("re-extracao falhou: {e}")),
            }
        }

        Ok(VerificationReport {
            ok: problems.is_empty(),
            checks,
            problems,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    fn probe(bytes: &[u8]) -> ProbeResult {
        GbaAdapter.probe(&GameInput::from_bytes("test.gba", bytes))
    }

    #[test]
    fn detects_synthetic_gba() {
        let rom = synth::make_gba_rom("TESTGAME");
        let result = probe(&rom);
        assert!(
            result.confidence >= 0.9,
            "confidence: {}",
            result.confidence
        );
        assert_eq!(result.platform, Platform::Gba);
    }

    #[test]
    fn bad_checksum_lowers_confidence() {
        let mut rom = synth::make_gba_rom("TESTGAME");
        rom[CHECKSUM_OFFSET] ^= 0xFF;
        let result = probe(&rom);
        assert!(result.confidence < 0.9);
        assert!(result.confidence > 0.0, "byte fixo ainda presente");
    }

    #[test]
    fn rejects_random_and_short_input() {
        assert_eq!(probe(&synth::make_random(4096, 42)).confidence, 0.0);
        assert_eq!(probe(&[]).confidence, 0.0);
        assert_eq!(probe(&[0u8; 0x50]).confidence, 0.0);
    }
}

//! Adapter de NES (formato iNES / NES 2.0: magic "NES\x1A" + tamanhos de PRG/CHR).
//!
//! Reinsercao CONSERVADORA (Experimental): strings ASCII traduzidas in-place no
//! espaco original. Muitos jogos NES usam tabelas de tiles proprias — para esses,
//! use o scanner com tabela `.tbl`; o in-place cobre os que guardam texto ASCII.
//! iNES nao tem checksum de header: nada a recalcular.

use crate::adapter::{AppliedImage, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::scan::{scan_bytes, ScanConfig, ScanEncoding};
use crate::types::{AdapterCapabilities, Platform, ProbeResult, SupportLevel, TextEntry};

pub struct NesAdapter;

const MAGIC: &[u8; 4] = b"NES\x1a";
const HEADER_LEN: usize = 16;

impl GameAdapter for NesAdapter {
    fn id(&self) -> &'static str {
        "nes.ines"
    }

    fn display_name(&self) -> &'static str {
        "NES (iNES)"
    }

    fn platform(&self) -> Platform {
        Platform::Nes
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
        if head.len() < HEADER_LEN || &head[0..4] != MAGIC {
            return ProbeResult::no_match(self.id(), self.platform());
        }

        let mut evidence = vec!["magic iNES (NES\\x1A) presente".to_string()];

        let prg = head[4] as u64 * 16 * 1024;
        let chr = head[5] as u64 * 8 * 1024;
        let trainer = if head[6] & 0x04 != 0 { 512 } else { 0 };
        let expected = HEADER_LEN as u64 + trainer + prg + chr;

        if head[7] & 0x0C == 0x08 {
            evidence.push("header NES 2.0".to_string());
        }

        let confidence = if input.size == expected {
            evidence.push(format!(
                "tamanho do arquivo bate com o header (PRG {prg} + CHR {chr} bytes)"
            ));
            0.99
        } else if input.size > expected {
            evidence.push("arquivo maior que o declarado no header (extra data?)".to_string());
            0.9
        } else {
            evidence.push("arquivo MENOR que o declarado no header (truncado?)".to_string());
            0.6
        };

        ProbeResult {
            confidence,
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }

    /// Extracao conservadora: scan ASCII com `max_bytes` = espaco original.
    /// Ids identicos aos do scanner generico — sem duplicatas no projeto.
    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        if data.len() < HEADER_LEN || &data[0..4] != MAGIC {
            return Err(CoreError::Project(
                "nes: arquivo sem header iNES valido".to_string(),
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

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        if data.len() < HEADER_LEN || &data[0..4] != MAGIC {
            return Err(CoreError::Project(
                "nes: arquivo sem header iNES valido".to_string(),
            ));
        }
        // iNES nao tem checksum: nada a finalizar.
        super::inplace::apply_in_place(data, entries, |_| {})
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        let mut checks = Vec::new();
        let mut problems = Vec::new();

        if data.len() < HEADER_LEN || &data[0..4] != MAGIC {
            problems.push("magic iNES ausente".to_string());
        } else {
            checks.push("magic iNES preservado".to_string());
            let prg = data[4] as usize * 16 * 1024;
            let chr = data[5] as usize * 8 * 1024;
            let trainer = if data[6] & 0x04 != 0 { 512 } else { 0 };
            let expected = HEADER_LEN + trainer + prg + chr;
            if data.len() >= expected {
                checks.push("tamanho consistente com o header".to_string());
            } else {
                problems.push("arquivo menor que o declarado no header".to_string());
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
        NesAdapter.probe(&GameInput::from_bytes("test.nes", bytes))
    }

    #[test]
    fn detects_synthetic_nes() {
        let result = probe(&synth::make_nes_rom());
        assert!(
            result.confidence >= 0.99,
            "confidence: {}",
            result.confidence
        );
        assert_eq!(result.platform, Platform::Nes);
    }

    #[test]
    fn truncated_nes_flags_lower_confidence() {
        let rom = synth::make_nes_rom();
        let result = probe(&rom[..1024]);
        assert!(result.confidence > 0.0 && result.confidence < 0.9);
    }

    #[test]
    fn rejects_non_nes_input() {
        assert_eq!(probe(&synth::make_random(4096, 7)).confidence, 0.0);
        assert_eq!(probe(&[]).confidence, 0.0);
        assert_eq!(probe(b"NES").confidence, 0.0);
    }
}

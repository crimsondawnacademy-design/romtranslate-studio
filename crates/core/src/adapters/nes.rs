//! Probe de NES (formato iNES / NES 2.0: magic "NES\x1A" + tamanhos de PRG/CHR).

use crate::adapter::{GameAdapter, GameInput};
use crate::types::{AdapterCapabilities, Platform, ProbeResult};

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
        AdapterCapabilities::detect_only()
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

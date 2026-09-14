//! Probe de Game Boy Advance (GBATEK: cartridge header em 0x00-0xBF).
//!
//! Evidencias usadas (sem embutir o logo Nintendo, que e material de terceiros):
//! - byte fixo 0x96 em 0xB2 (obrigatorio no header);
//! - header checksum em 0xBD sobre 0xA0..=0xBC;
//! - entry point como branch ARM (byte 3 == 0xEA) e titulo ASCII como pistas fracas.

use crate::adapter::{GameAdapter, GameInput};
use crate::types::{AdapterCapabilities, Platform, ProbeResult};

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
        AdapterCapabilities::detect_only()
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

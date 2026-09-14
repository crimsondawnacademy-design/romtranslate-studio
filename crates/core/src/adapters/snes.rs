//! Probe de SNES. Nao ha magic number: a deteccao pontua candidatos de header
//! interno em 0x7FC0 (LoROM) e 0xFFC0 (HiROM), com suporte a copier header de
//! 512 bytes (.smc). Evidencias: par checksum/complement, titulo ASCII e map mode.

use crate::adapter::{GameAdapter, GameInput};
use crate::types::{AdapterCapabilities, Platform, ProbeResult};

pub struct SnesAdapter;

pub const LOROM_HEADER: usize = 0x7FC0;
pub const HIROM_HEADER: usize = 0xFFC0;
const TITLE_LEN: usize = 21;
const MAP_MODE: usize = 0x15;
const COMPLEMENT: usize = 0x1C;
const CHECKSUM: usize = 0x1E;

struct Candidate {
    mapping: &'static str,
    score: f32,
    evidence: Vec<String>,
}

fn read_u16_le(buf: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*buf.get(off)?, *buf.get(off + 1)?]))
}

fn score_candidate(
    head: &[u8],
    base: usize,
    mapping: &'static str,
    hirom: bool,
) -> Option<Candidate> {
    // Header interno ocupa base..base+0x20; sem esses bytes nao ha candidato.
    if base + 0x20 > head.len() {
        return None;
    }
    let header = &head[base..base + 0x20];
    let mut score = 0.0f32;
    let mut evidence = Vec::new();

    let complement = read_u16_le(header, COMPLEMENT)?;
    let checksum = read_u16_le(header, CHECKSUM)?;
    if complement ^ checksum == 0xFFFF {
        score += 0.55;
        evidence.push(format!("par checksum/complement consistente ({mapping})"));
    }

    let title = &header[..TITLE_LEN];
    let printable = title.iter().filter(|&&b| (0x20..0x7F).contains(&b)).count();
    if printable >= TITLE_LEN - 2 {
        score += 0.25;
        let text: String = title
            .iter()
            .map(|&b| {
                if (0x20..0x7F).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        evidence.push(format!("titulo interno ASCII: \"{}\"", text.trim_end()));
    }

    let map_mode = header[MAP_MODE];
    if map_mode & 0x20 != 0 {
        score += 0.08;
        evidence.push(format!("map mode plausivel (0x{map_mode:02X})"));
        if (map_mode & 0x01 != 0) == hirom {
            score += 0.07;
            evidence.push(format!("map mode coerente com offset {mapping}"));
        }
    }

    Some(Candidate {
        mapping,
        score,
        evidence,
    })
}

impl GameAdapter for SnesAdapter {
    fn id(&self) -> &'static str {
        "snes.generic"
    }

    fn display_name(&self) -> &'static str {
        "SNES Generic"
    }

    fn platform(&self) -> Platform {
        Platform::Snes
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::detect_only()
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        // Copier header (.smc): 512 bytes extras no inicio.
        let copier = if input.size % 1024 == 512 {
            512usize
        } else {
            0
        };

        let mut best: Option<Candidate> = None;
        for (offset, mapping, hirom) in [
            (LOROM_HEADER, "LoROM", false),
            (HIROM_HEADER, "HiROM", true),
        ] {
            if let Some(c) = score_candidate(&input.head, copier + offset, mapping, hirom) {
                if best.as_ref().is_none_or(|b| c.score > b.score) {
                    best = Some(c);
                }
            }
        }

        let Some(mut cand) = best.filter(|c| c.score > 0.2) else {
            return ProbeResult::no_match(self.id(), self.platform());
        };

        if copier != 0 {
            cand.evidence
                .push("copier header de 512 bytes detectado (.smc)".to_string());
        }
        cand.evidence
            .insert(0, format!("mapeamento provavel: {}", cand.mapping));

        ProbeResult {
            confidence: cand.score.min(1.0),
            evidence: cand.evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    fn probe(bytes: &[u8]) -> ProbeResult {
        SnesAdapter.probe(&GameInput::from_bytes("test.sfc", bytes))
    }

    #[test]
    fn detects_synthetic_lorom() {
        let result = probe(&synth::make_snes_lorom("SYNTHETIC QUEST"));
        assert!(
            result.confidence >= 0.9,
            "confidence: {}",
            result.confidence
        );
        assert_eq!(result.platform, Platform::Snes);
        assert!(result.evidence.iter().any(|e| e.contains("LoROM")));
    }

    #[test]
    fn detects_copier_headered_smc() {
        let result = probe(&synth::make_snes_headered("SYNTHETIC QUEST"));
        assert!(
            result.confidence >= 0.9,
            "confidence: {}",
            result.confidence
        );
        assert!(result.evidence.iter().any(|e| e.contains("copier header")));
    }

    #[test]
    fn rejects_random_short_and_empty() {
        assert_eq!(probe(&synth::make_random(0x8000, 3)).confidence, 0.0);
        assert_eq!(probe(&[]).confidence, 0.0);
        assert_eq!(probe(&[0u8; 0x4000]).confidence, 0.0);
    }
}

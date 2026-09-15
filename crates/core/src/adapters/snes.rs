//! Adapter de SNES. Nao ha magic number: a deteccao pontua candidatos de header
//! interno em 0x7FC0 (LoROM) e 0xFFC0 (HiROM), com suporte a copier header de
//! 512 bytes (.smc). Evidencias: par checksum/complement, titulo ASCII e map mode.
//!
//! Reinsercao CONSERVADORA (Experimental): strings ASCII in-place no espaco
//! original, com o par checksum/complement do header interno recalculado pela
//! SOMA CANONICA do corpo (copier header fora; resto espelhado quando o
//! tamanho nao e potencia de 2).

use crate::adapter::{AppliedImage, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::scan::{scan_bytes, ScanConfig, ScanEncoding};
use crate::types::{AdapterCapabilities, Platform, ProbeResult, SupportLevel, TextEntry};

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

/// Copier header (.smc): 512 bytes extras no inicio do arquivo.
fn copier_len(total: usize) -> usize {
    if total % 1024 == 512 {
        512
    } else {
        0
    }
}

/// Melhor candidato de header interno no arquivo: (offset absoluto, mapping, copier).
pub fn find_header(data: &[u8]) -> Option<(usize, &'static str, usize)> {
    let copier = copier_len(data.len());
    let mut best: Option<(Candidate, usize)> = None;
    for (offset, mapping, hirom) in [
        (LOROM_HEADER, "LoROM", false),
        (HIROM_HEADER, "HiROM", true),
    ] {
        if let Some(c) = score_candidate(data, copier + offset, mapping, hirom) {
            if best.as_ref().is_none_or(|(b, _)| c.score > b.score) {
                best = Some((c, copier + offset));
            }
        }
    }
    best.filter(|(c, _)| c.score > 0.2)
        .map(|(c, abs)| (abs, c.mapping, copier))
}

/// Soma canonica SNES (u16 wrapping) do corpo da ROM. Tamanho potencia de 2:
/// soma direta; senao, o resto e espelhado ate preencher a parte baixa (regra
/// dos dumps A+B); layout mais exotico cai em soma simples — o verify usa o
/// MESMO algoritmo, entao o fluxo fica consistente.
pub fn snes_sum(body: &[u8]) -> u16 {
    fn sum(data: &[u8]) -> u32 {
        data.iter().fold(0u32, |a, &b| a.wrapping_add(b as u32))
    }
    let len = body.len();
    if len == 0 {
        return 0;
    }
    if len.is_power_of_two() {
        return sum(body) as u16;
    }
    let half = 1usize << (usize::BITS - 1 - len.leading_zeros());
    let rest = &body[half..];
    if rest.len().is_power_of_two() && half.is_multiple_of(rest.len()) {
        let mult = (half / rest.len()) as u32;
        sum(&body[..half]).wrapping_add(sum(rest).wrapping_mul(mult)) as u16
    } else {
        sum(body) as u16
    }
}

/// Recalcula o par complement/checksum no header interno: campos zerados
/// durante a soma + 0x1FE (a contribuicao fixa de qualquer par valido).
fn recalc_internal_checksum(out: &mut [u8]) {
    let Some((header_abs, _, copier)) = find_header(out) else {
        return;
    };
    out[header_abs + COMPLEMENT..header_abs + COMPLEMENT + 4].fill(0);
    let checksum = snes_sum(&out[copier..]).wrapping_add(0x1FE);
    let complement = checksum ^ 0xFFFF;
    out[header_abs + COMPLEMENT..header_abs + COMPLEMENT + 2]
        .copy_from_slice(&complement.to_le_bytes());
    out[header_abs + CHECKSUM..header_abs + CHECKSUM + 2].copy_from_slice(&checksum.to_le_bytes());
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

    /// Extracao conservadora: scan ASCII com `max_bytes` = espaco original.
    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        if find_header(data).is_none() {
            return Err(CoreError::Project(
                "snes: header interno nao encontrado (LoROM/HiROM)".to_string(),
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

    /// In-place + recalculo do par checksum/complement do header interno.
    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        if find_header(data).is_none() {
            return Err(CoreError::Project(
                "snes: header interno nao encontrado (LoROM/HiROM)".to_string(),
            ));
        }
        super::inplace::apply_in_place(data, entries, recalc_internal_checksum)
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        let mut checks = Vec::new();
        let mut problems = Vec::new();

        match find_header(data) {
            None => problems.push("header interno nao encontrado".to_string()),
            Some((header_abs, mapping, copier)) => {
                checks.push(format!("header interno {mapping} em 0x{header_abs:X}"));
                let complement = u16::from_le_bytes([
                    data[header_abs + COMPLEMENT],
                    data[header_abs + COMPLEMENT + 1],
                ]);
                let checksum = u16::from_le_bytes([
                    data[header_abs + CHECKSUM],
                    data[header_abs + CHECKSUM + 1],
                ]);
                // Soma real com os 4 bytes do par zerados + contribuicao fixa 0x1FE.
                let mut body = data[copier..].to_vec();
                let rel = header_abs - copier;
                body[rel + COMPLEMENT..rel + COMPLEMENT + 4].fill(0);
                let expected = snes_sum(&body).wrapping_add(0x1FE);
                if checksum == expected && complement == (checksum ^ 0xFFFF) {
                    checks.push("checksum interno bate com a soma real do arquivo".to_string());
                } else {
                    problems.push(format!(
                        "checksum interno invalido: header 0x{checksum:04X}, soma real 0x{expected:04X}"
                    ));
                }
                match self.extract_structured(data) {
                    Ok(entries) => checks.push(format!("{} strings re-extraidas", entries.len())),
                    Err(e) => problems.push(format!("re-extracao falhou: {e}")),
                }
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

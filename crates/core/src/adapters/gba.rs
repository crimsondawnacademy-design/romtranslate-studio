//! Adapter de Game Boy Advance (GBATEK: cartridge header em 0x00-0xBF).
//!
//! Deteccao sem embutir o logo Nintendo (material de terceiros): byte fixo
//! 0x96 em 0xB2, header checksum em 0xBD, pistas fracas (branch ARM, titulo).
//!
//! Reinsercao (Experimental): traducao que cabe vai in-place no espaco da
//! original. A que nao cabe so passa se a string tiver ponteiros numa TABELA
//! detectada (`adapters::pointers`: 2+ ponteiros de ROM consecutivos pra
//! inicios de string) — ai vai pro fim do ROM e a tabela e reapontada.
//! Ponteiro isolado (literal pool) nao conta, e ponteiro pros espelhos de
//! wait-state (0x0A/0x0C000000) nao e reconhecido. O header checksum e
//! recalculado (traduzir o titulo em 0xA0 o afetaria).

use std::collections::HashSet;

use super::pointers::{
    ensure_pointers_untouched, find_pointer_tables, is_terminated, mark_relocatable, relocate,
    PointerFormat,
};
use crate::adapter::{AppliedImage, Extraction, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::scan::{keep_text, scan_bytes, ScanConfig, ScanEncoding, MAX_ENTRIES};
use crate::types::{AdapterCapabilities, Platform, ProbeResult, SupportLevel, TextEntry};

pub struct GbaAdapter;

const HEADER_LEN: usize = 0xC0;
const FIXED_VALUE_OFFSET: usize = 0xB2;
const CHECKSUM_OFFSET: usize = 0xBD;
const TITLE_RANGE: std::ops::Range<usize> = 0xA0..0xAC;
/// ROM mapeada em 0x08000000 (GBATEK): ponteiro pra texto = base + offset,
/// u32. Ponteiro de ROM quase nunca aparece por acaso — 2 seguidos ja sao
/// estrutura. Sem exigir ordem: tabela de struct {nome, descricao} intercala
/// dois blocos de strings.
const ROM_POINTERS: PointerFormat = PointerFormat {
    width: 4,
    base: 0x0800_0000,
    min_run: 2,
    relative: false,
};
/// Janela de ROM do cartucho: 32 MiB (0x08000000-0x09FFFFFF).
const MAX_ROM_LEN: usize = 32 * 1024 * 1024;

/// Checksum do header GBA: soma negativa de 0xA0..=0xBC menos 0x19 (mod 256).
pub fn header_checksum(header: &[u8]) -> u8 {
    let mut chk: u8 = 0;
    for &b in &header[0xA0..=0xBC] {
        chk = chk.wrapping_sub(b);
    }
    chk.wrapping_sub(0x19)
}

/// A extracao, com o teto injetavel (o teste usa um pequeno).
fn extract_capped(data: &[u8], cap: usize) -> Result<Extraction> {
    if data.len() < HEADER_LEN {
        return Err(CoreError::Project(
            "gba: arquivo menor que o header do cartucho".to_string(),
        ));
    }
    // ponytail: uma TextEntry por sequencia crua, ~0,5 KB cada — uns
    // 100 MB de pico num cartucho de 16 MiB (187 mil sequencias), o dobro
    // no de 32. Se pesar, filtrar no scanner com os offsets das tabelas
    // achados numa passada anterior.
    let outcome = scan_bytes(
        data,
        &ScanConfig {
            encoding: ScanEncoding::Ascii,
            max_entries: usize::MAX,
            ..ScanConfig::default()
        },
    )?;
    let mut entries = outcome.entries;
    let starts: HashSet<usize> = entries
        .iter()
        .filter(|e| is_terminated(e))
        .filter_map(|e| e.offset.map(|o| o as usize))
        .collect();
    let tables = find_pointer_tables(data, &ROM_POINTERS, &starts);

    for e in entries.iter_mut() {
        let pointers = e.offset.and_then(|o| tables.get(&(o as usize)));
        if let Some(pointers) = pointers.filter(|p| mark_relocatable(e, p)) {
            e.max_bytes = None;
            e.context = Some(format!(
                "realocavel: {} ponteiro(s) em tabela — nao precisa caber no espaco original",
                pointers.len()
            ));
        } else {
            e.max_bytes = Some(e.original_bytes.len());
            e.context = Some("in-place: traducao limitada ao espaco original".to_string());
        }
    }
    let discarded = keep_text(&mut entries);
    let truncated = entries.len() > cap;
    entries.truncate(cap);
    Ok(Extraction {
        entries,
        discarded,
        truncated,
    })
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
            reinsert: true, // in-place; relocacao so com ponteiros em tabela
            patch: true,
            compression: false,
            pointer_relocation: true,
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

    /// Scan ASCII + deteccao de tabelas de ponteiros. String com ponteiro em
    /// tabela fica sem `max_bytes` (pode crescer: sera realocada); as demais
    /// ficam limitadas ao espaco que ja ocupam. Ids identicos aos do scanner
    /// generico ("scan-<offset>"), entao rodar os dois nao duplica entries.
    ///
    /// Ordem: varre tudo, acha os ponteiros, descarta o que nao parece texto
    /// (string em tabela fica sempre) e so entao corta no teto. Com o teto
    /// antes, o lixo do comeco do cartucho escondia o texto do fim: no jogo
    /// medido em 12/10/2026, 16,4 mil das 20 mil primeiras eram lixo.
    fn extract(&self, data: &[u8]) -> Result<Extraction> {
        extract_capped(data, MAX_ENTRIES)
    }

    /// In-place do que cabe; o que nao cabe e tem ponteiros em tabela vai pro
    /// fim do ROM com a tabela reapontada. Header checksum recalculado no fim.
    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        if data.len() < HEADER_LEN {
            return Err(CoreError::Project(
                "gba: arquivo menor que o header do cartucho".to_string(),
            ));
        }
        let plan = super::inplace::plan_in_place(data, entries, true)?;
        let mut out = data.to_vec();
        for (offset, patch) in &plan.writes {
            out[*offset..offset + patch.len()].copy_from_slice(patch);
        }
        ensure_pointers_untouched(data, &out, entries)?;
        relocate(&mut out, &plan.relocations, ROM_POINTERS.base, MAX_ROM_LEN)?;
        out[CHECKSUM_OFFSET] = header_checksum(&out);
        Ok(AppliedImage {
            bytes: out,
            report: plan.report,
        })
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
    fn cap_counts_text_not_junk() {
        // Centenas de sequencias de lixo antes do texto e um teto de 40: com
        // o teto antes do filtro, nenhuma frase do fim do cartucho apareceria.
        let rom = synth::make_gba_rom_with_junk();
        let raw = scan_bytes(&rom, &ScanConfig::default()).unwrap().entries;
        let first_phrase = raw
            .iter()
            .position(|e| e.source_text == synth::JUNK_ROM_PHRASES[0])
            .unwrap();
        assert!(first_phrase > 40, "lixo antes do texto: {first_phrase}");

        let out = extract_capped(&rom, 40).unwrap();
        assert!(!out.truncated, "{} entries", out.entries.len());
        let texts: Vec<&str> = out.entries.iter().map(|e| e.source_text.as_str()).collect();
        for phrase in synth::JUNK_ROM_PHRASES {
            assert!(texts.contains(&phrase), "{phrase}: {texts:?}");
        }
        assert_eq!(out.discarded + out.entries.len(), raw.len());

        // Teto menor que o texto: corta, e avisa.
        let cut = extract_capped(&rom, 3).unwrap();
        assert_eq!((cut.entries.len(), cut.truncated), (3, true));
    }

    #[test]
    fn rejects_random_and_short_input() {
        assert_eq!(probe(&synth::make_random(4096, 42)).confidence, 0.0);
        assert_eq!(probe(&[]).confidence, 0.0);
        assert_eq!(probe(&[0u8; 0x50]).confidence, 0.0);
    }
}

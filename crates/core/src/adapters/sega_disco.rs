//! Discos da Sega: Saturn e Mega-CD. ISO 9660 normal, em imagem de 2048 ou
//! raw 2352 (setor Mode 1, EDC/ECC refeito pelo modulo `cdrom`), com a
//! assinatura do console no setor 0 da trilha de dados: "SEGA SEGASATURN " no
//! Saturn, "SEGADISCSYSTEM  " no Mega-CD. Extracao por arquivo, em ASCII e
//! Shift-JIS (o japones dos dois), e reinsercao no espaco da original — o
//! mesmo caminho do PS1, sem a sobra de setor.
//!
//! Lista de pedidos do Data Crystal (20/10/2026): Saturn 109 jogos, Mega-CD
//! 37, quase todos japoneses.

use super::iso9660::{
    self, apply_iso, detect_map, extract_ascii_by_file, parse_pvd, read_sector, walk, SectorMap,
};
use crate::adapter::{AppliedImage, Extraction, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::types::{
    AdapterCapabilities, Platform, ProbeResult, ResourceDescriptor, SupportLevel, TextEntry,
};

pub struct Disc {
    id: &'static str,
    name: &'static str,
    platform: Platform,
    /// O comeco do setor 0 da trilha de dados.
    tag: &'static [u8],
}

pub static DISCS: [Disc; 2] = [
    Disc {
        id: "saturn.cd",
        name: "Sega Saturn",
        platform: Platform::Saturn,
        tag: b"SEGA SEGASATURN ",
    },
    Disc {
        id: "megacd.cd",
        name: "Mega-CD / Sega CD",
        platform: Platform::MegaCd,
        tag: b"SEGADISCSYSTEM  ",
    },
];

pub struct SegaDisc(pub &'static Disc);

impl SegaDisc {
    fn signed(&self, data: &[u8], map: SectorMap) -> bool {
        read_sector(data, map, 0).is_ok_and(|s| s.starts_with(self.0.tag))
    }

    fn check(&self, data: &[u8]) -> Result<SectorMap> {
        let map = detect_map(data);
        if self.signed(data, map) && parse_pvd(data, map).is_ok() {
            Ok(map)
        } else {
            Err(CoreError::Project(format!(
                "{}: imagem sem a assinatura do console no setor 0 ou sem ISO 9660",
                self.0.name
            )))
        }
    }
}

impl GameAdapter for SegaDisc {
    fn id(&self) -> &'static str {
        self.0.id
    }

    fn display_name(&self) -> &'static str {
        self.0.name
    }

    fn platform(&self) -> Platform {
        self.0.platform
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            detect: true,
            extract: true,
            reinsert: true,
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
        let map = detect_map(head);
        if !self.signed(head, map) {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let mut evidence = vec![format!(
            "assinatura {:?} no setor 0",
            String::from_utf8_lossy(self.0.tag).trim()
        )];
        let mut confidence = 0.85;
        if let Ok(info) = parse_pvd(head, map) {
            confidence += 0.1;
            evidence.push(format!("ISO 9660 (volume \"{}\")", info.volume_id));
        }
        if map == SectorMap::Raw2352 {
            evidence.push("imagem raw de CD (2352 bytes/setor, BIN)".to_string());
        }
        ProbeResult {
            confidence,
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }

    fn list_resources(&self, data: &[u8]) -> Result<Vec<ResourceDescriptor>> {
        let map = self.check(data)?;
        let files = walk(data, map)?;
        Ok(iso9660::to_resources(data, map, &files))
    }

    fn extract(&self, data: &[u8]) -> Result<Extraction> {
        let map = self.check(data)?;
        extract_ascii_by_file(data, map, false)
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        let map = self.check(data)?;
        apply_iso(data, map, entries, false)
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        super::ps1::verify_iso(self, data, |map, d| self.signed(d, map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;
    use crate::types::TextEncoding;

    fn adapter(id: &str) -> SegaDisc {
        SegaDisc(DISCS.iter().find(|d| d.id == id).unwrap())
    }

    #[test]
    fn saturn_and_mega_cd_are_told_apart_and_from_playstation() {
        let saturn = synth::make_saturn_bin();
        let mega = synth::make_mega_cd_iso();
        let probe = |id: &str, name: &str, rom: &[u8]| {
            adapter(id)
                .probe(&GameInput::from_bytes(name, rom))
                .confidence
        };
        assert!(probe("saturn.cd", "a.bin", &saturn) >= 0.9);
        assert_eq!(probe("megacd.cd", "a.bin", &saturn), 0.0);
        assert!(probe("megacd.cd", "a.iso", &mega) >= 0.9);
        assert_eq!(probe("saturn.cd", "a.iso", &mega), 0.0);
        for id in ["saturn.cd", "megacd.cd"] {
            assert_eq!(probe(id, "a.bin", &synth::make_ps1_bin()), 0.0);
            assert_eq!(probe(id, "a.bin", &synth::make_random(1 << 16, 9)), 0.0);
            assert_eq!(probe(id, "a.bin", &[]), 0.0);
        }
    }

    #[test]
    fn a_translation_goes_in_place_with_the_sector_redone() {
        let disc = synth::make_saturn_bin();
        let mut entries = adapter("saturn.cd").extract(&disc).unwrap().entries;
        let japanese = entries
            .iter()
            .position(|e| e.source_text.contains("ぼうけん"))
            .expect("o japones em Shift-JIS sai");
        let english = entries
            .iter()
            .position(|e| e.source_text == "PRESS START BUTTON")
            .expect("o ASCII sai");
        entries[english].translated_text = Some("APERTE START".into());
        entries[japanese].translated_text = Some("Aventura!".into());
        assert_eq!(entries[japanese].encoding, TextEncoding::ShiftJis);
        let out = adapter("saturn.cd").apply_text(&disc, &entries).unwrap();
        assert_eq!(out.report.applied, 2);
        let report = adapter("saturn.cd").verify(&out.bytes).unwrap();
        assert!(report.ok, "{:?}", report.problems);
        let again = adapter("saturn.cd").extract(&out.bytes).unwrap().entries;
        assert!(again
            .iter()
            .any(|e| e.source_text.starts_with("APERTE START")));
    }
}

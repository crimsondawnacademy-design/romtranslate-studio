//! Adapter de PlayStation 2: ISO 9660 (DVD, 2048/setor; CD raw tambem
//! detectado) com SYSTEM.CNF contendo `BOOT2 =` — o discriminador canonico
//! contra PS1. In-place em 2048. DVD acima do teto em memoria (2 GiB):
//! extracao/verify por mmap (`fileio::read_view`) e reinsercao streaming
//! (`reinsert::stream_apply_iso`) — o dual layer de 8.5 GiB passa inteiro.

use super::iso9660::{
    self, apply_iso, detect_map, extract_ascii_by_file, identify_playstation, parse_pvd, walk,
    PsKind, SectorMap,
};
use super::ps1::verify_iso;
use crate::adapter::{AppliedImage, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::types::{
    AdapterCapabilities, Platform, ProbeResult, ResourceDescriptor, SupportLevel, TextEntry,
};

pub struct Ps2Adapter;

impl GameAdapter for Ps2Adapter {
    fn id(&self) -> &'static str {
        "ps2.generic"
    }

    fn display_name(&self) -> &'static str {
        "PlayStation 2 (DVD/CD)"
    }

    fn platform(&self) -> Platform {
        Platform::Ps2
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
        let Ok(info) = parse_pvd(head, map) else {
            return ProbeResult::no_match(self.id(), self.platform());
        };
        if !info.system_id.contains("PLAYSTATION") {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let mut evidence = vec![format!(
            "ISO 9660 com system id \"{}\" (volume \"{}\")",
            info.system_id, info.volume_id
        )];
        match identify_playstation(head, map) {
            Some((PsKind::Two, boot)) => {
                evidence.push(format!("SYSTEM.CNF: {boot}"));
                ProbeResult {
                    confidence: 0.96,
                    evidence,
                    ..ProbeResult::no_match(self.id(), self.platform())
                }
            }
            Some((PsKind::One, _)) => ProbeResult::no_match(self.id(), self.platform()),
            None => {
                evidence.push(
                    "SYSTEM.CNF fora da janela de probe — PS1 vs PS2 confirma na extracao"
                        .to_string(),
                );
                ProbeResult {
                    // Sem CNF: 2048 pende pra DVD (PS2); raw pende pra PS1.
                    confidence: if map == SectorMap::Plain2048 {
                        0.45
                    } else {
                        0.3
                    },
                    evidence,
                    ..ProbeResult::no_match(self.id(), self.platform())
                }
            }
        }
    }

    fn list_resources(&self, data: &[u8]) -> Result<Vec<ResourceDescriptor>> {
        let map = detect_map(data);
        let files = walk(data, map)?;
        Ok(iso9660::to_resources(data, map, &files))
    }

    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        extract_ascii_by_file(data, detect_map(data))
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        let map = detect_map(data);
        parse_pvd(data, map)
            .map_err(|_| CoreError::Project("ps2: imagem sem ISO 9660 valido".to_string()))?;
        apply_iso(data, map, entries, false)
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        verify_iso(self, data, |map, d| {
            matches!(identify_playstation(d, map), Some((PsKind::Two, _)))
        })
    }
}

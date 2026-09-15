//! Probe de Wii: magic word 0x5D1C9EA3 em 0x18 (disco ISO) ou container WBFS
//! (magic "WBFS" em 0x00). Deteccao apenas — particoes de disco Wii sao
//! cifradas; extracao exigiria keys, que este projeto NAO inclui (spec §23).

use crate::adapter::{GameAdapter, GameInput};
use crate::types::{AdapterCapabilities, Platform, ProbeResult};

pub struct WiiAdapter;

pub const MAGIC_OFFSET: usize = 0x18;
pub const MAGIC: [u8; 4] = [0x5D, 0x1C, 0x9E, 0xA3];

impl GameAdapter for WiiAdapter {
    fn id(&self) -> &'static str {
        "wii.probe"
    }

    fn display_name(&self) -> &'static str {
        "Wii (disc)"
    }

    fn platform(&self) -> Platform {
        Platform::Wii
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::detect_only()
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        let head = &input.head;

        if head.len() >= 4 && &head[0..4] == b"WBFS" {
            return ProbeResult {
                confidence: 0.9,
                evidence: vec![
                    "container WBFS detectado (imagem de disco Wii)".to_string(),
                    "extracao de particoes cifradas nao e suportada (exigiria keys)".to_string(),
                ],
                ..ProbeResult::no_match(self.id(), self.platform())
            };
        }

        if head.len() < 0x60 || head[MAGIC_OFFSET..MAGIC_OFFSET + 4] != MAGIC {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let mut confidence: f32 = 0.93;
        let mut evidence = vec!["magic word de disco Wii em 0x18".to_string()];
        let title: Vec<u8> = head[0x20..0x60.min(head.len())]
            .iter()
            .take_while(|&&b| b != 0)
            .copied()
            .collect();
        if !title.is_empty() && title.iter().all(|&b| (0x20..0x7F).contains(&b)) {
            confidence += 0.05;
            evidence.push(format!(
                "titulo interno: \"{}\"",
                String::from_utf8_lossy(&title)
            ));
        }
        evidence.push("particoes cifradas: extracao nao suportada (sem keys)".to_string());

        ProbeResult {
            confidence: confidence.min(0.99),
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }
}

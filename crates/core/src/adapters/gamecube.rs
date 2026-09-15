//! Probe de GameCube (disco GCM/ISO): magic word 0xC2339F3D em 0x1C
//! (boot.bin), game code ASCII em 0x00 e titulo interno em 0x20.
//! Deteccao apenas — filesystem FST e extracao ficam para uma fase futura.

use crate::adapter::{GameAdapter, GameInput};
use crate::types::{AdapterCapabilities, Platform, ProbeResult};

pub struct GameCubeAdapter;

pub const MAGIC_OFFSET: usize = 0x1C;
pub const MAGIC: [u8; 4] = [0xC2, 0x33, 0x9F, 0x3D];
const TITLE_OFFSET: usize = 0x20;

impl GameAdapter for GameCubeAdapter {
    fn id(&self) -> &'static str {
        "gamecube.probe"
    }

    fn display_name(&self) -> &'static str {
        "GameCube (disc)"
    }

    fn platform(&self) -> Platform {
        Platform::GameCube
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::detect_only()
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        let head = &input.head;
        if head.len() < 0x60 || head[MAGIC_OFFSET..MAGIC_OFFSET + 4] != MAGIC {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let mut confidence: f32 = 0.93;
        let mut evidence = vec!["magic word de disco GameCube em 0x1C".to_string()];

        let code = &head[0..6];
        if code.iter().all(|&b| b.is_ascii_alphanumeric()) {
            confidence += 0.03;
            evidence.push(format!(
                "game code ASCII: {}",
                String::from_utf8_lossy(code)
            ));
        }
        let title: Vec<u8> = head[TITLE_OFFSET..(TITLE_OFFSET + 0x40).min(head.len())]
            .iter()
            .take_while(|&&b| b != 0)
            .copied()
            .collect();
        if !title.is_empty() && title.iter().all(|&b| (0x20..0x7F).contains(&b)) {
            confidence += 0.03;
            evidence.push(format!(
                "titulo interno: \"{}\"",
                String::from_utf8_lossy(&title)
            ));
        }

        ProbeResult {
            confidence: confidence.min(0.99),
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }
}

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::types::{AdapterCapabilities, Platform, ProbeResult};

/// Quantos bytes iniciais carregamos para probing. 128 KiB cobre os headers de
/// NES (16 B), GBA (192 B) e SNES (0xFFC0 + 512 de copier header < 128 KiB).
pub const PROBE_HEAD_LEN: usize = 128 * 1024;

// ponytail: limite fixo de 512 MiB cobre cartucho (GBA max 32 MiB, SNES 6 MiB);
// vira config quando entrarem plataformas de disco (GC/Wii/WiiU).
pub const MAX_FILE_SIZE: u64 = 512 * 1024 * 1024;

/// Entrada de probing: caminho, tamanho e os primeiros bytes do arquivo.
/// Probes leem SOMENTE de `head`, sempre com bounds check.
#[derive(Debug, Clone)]
pub struct GameInput {
    pub path: PathBuf,
    pub size: u64,
    pub head: Vec<u8>,
}

impl GameInput {
    pub fn load(path: &Path) -> Result<Self> {
        let mut file = File::open(path).map_err(|e| CoreError::io(path, e))?;
        let size = file.metadata().map_err(|e| CoreError::io(path, e))?.len();
        if size > MAX_FILE_SIZE {
            return Err(CoreError::FileTooLarge {
                size,
                limit: MAX_FILE_SIZE,
            });
        }
        let cap = PROBE_HEAD_LEN.min(size as usize);
        let mut head = vec![0u8; cap];
        file.read_exact(&mut head)
            .map_err(|e| CoreError::io(path, e))?;
        Ok(GameInput {
            path: path.to_path_buf(),
            size,
            head,
        })
    }

    /// Constroi input direto de bytes — usado em testes com fixtures sinteticas.
    pub fn from_bytes(path: impl Into<PathBuf>, bytes: &[u8]) -> Self {
        GameInput {
            path: path.into(),
            size: bytes.len() as u64,
            head: bytes[..bytes.len().min(PROBE_HEAD_LEN)].to_vec(),
        }
    }
}

/// Contrato de adapter de plataforma.
///
/// Sprint 1 usa apenas `probe`. `extract`/`apply`/`verify` entram no Sprint 2+.
/// Sync por enquanto: probing e IO local rapido; vira async junto com os
/// providers de traducao (Sprint 3) se necessario.
pub trait GameAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> AdapterCapabilities;

    /// Nunca panica com input malformado; retorna confidence 0.0 quando nao reconhece.
    fn probe(&self, input: &GameInput) -> ProbeResult;
}

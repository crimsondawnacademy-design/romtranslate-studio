use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::types::{AdapterCapabilities, Platform, ProbeResult};

/// Quantos bytes iniciais carregamos para probing. 128 KiB cobre os headers de
/// NES (16 B), GBA (192 B) e SNES (0xFFC0 + 512 de copier header < 128 KiB).
pub const PROBE_HEAD_LEN: usize = 128 * 1024;

/// Limite de INSPECAO (probe + hash streaming): cobre discos GC (1.4 GiB) e
/// Wii (4.7-8.5 GiB). Hash de arquivos grandes roda em spawn_blocking.
pub const MAX_FILE_SIZE: u64 = 16 * 1024 * 1024 * 1024;

// ponytail: extracao/reinsercao carregam o arquivo INTEIRO em RAM; 2 GiB
// cobre cartuchos e disco GameCube (1.46 GiB) — o apply clona o buffer, entao
// o pico chega a ~2x o arquivo. Streaming por recurso se isso doer na pratica.
pub const IN_MEMORY_MAX: u64 = 2 * 1024 * 1024 * 1024;

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

use crate::types::TextEntry;
use serde::Serialize;

/// Resultado de `apply_text`: quantas entries entraram na imagem nova.
/// Falha de serializacao e `Err` (all-or-nothing) — nunca imagem parcial.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    /// Entries com traducao aplicadas na imagem.
    pub applied: usize,
    /// Entries estruturadas sem traducao: texto original mantido.
    pub kept_original: usize,
    /// Entries de scanner generico ignoradas (adapter so aplica as estruturadas).
    pub ignored_generic: usize,
    /// Das aplicadas, quantas foram gravadas em outro lugar com os ponteiros
    /// reapontados (nao cabiam no espaco original).
    pub relocated: usize,
}

/// Imagem modificada em memoria + relatorio. O caller decide onde gravar
/// (sempre working copy — nunca o original).
#[derive(Debug)]
pub struct AppliedImage {
    pub bytes: Vec<u8>,
    pub report: ApplyReport,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub ok: bool,
    pub checks: Vec<String>,
    pub problems: Vec<String>,
}

fn unsupported<T>(id: &str, what: &str) -> Result<T> {
    Err(crate::error::CoreError::Project(format!(
        "adapter {id} nao suporta {what}"
    )))
}

/// Contrato de adapter de plataforma.
///
/// `probe` e obrigatorio; os metodos estruturados tem default "nao suportado" —
/// so quem declara a capability implementa (Camada B). Sync por enquanto:
/// IO local rapido; vira async se algum adapter precisar de verdade.
pub trait GameAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> AdapterCapabilities;

    /// Nunca panica com input malformado; retorna confidence 0.0 quando nao reconhece.
    fn probe(&self, input: &GameInput) -> ProbeResult;

    /// Lista os recursos internos de um container (filesystem de cartucho/disco).
    fn list_resources(&self, _data: &[u8]) -> Result<Vec<crate::types::ResourceDescriptor>> {
        unsupported(self.id(), "listagem de recursos")
    }

    /// Extracao estruturada (offsets/ponteiros/limites reais) — Camada B.
    fn extract_structured(&self, _data: &[u8]) -> Result<Vec<TextEntry>> {
        unsupported(self.id(), "extracao estruturada")
    }

    /// Serializa traducoes numa NOVA imagem (checksums/ponteiros atualizados).
    /// All-or-nothing: qualquer traducao que nao serializa retorna Err.
    fn apply_text(&self, _data: &[u8], _entries: &[TextEntry]) -> Result<AppliedImage> {
        unsupported(self.id(), "reinsercao")
    }

    /// Verificacao estrutural da imagem (usada apos gravar a working copy).
    fn verify(&self, _data: &[u8]) -> Result<VerificationReport> {
        unsupported(self.id(), "verificacao")
    }
}

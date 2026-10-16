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
/// Probes leem de `head`, sempre com bounds check. Quem precisa olhar alem
/// dele (disco optico) pede `whole`.
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

    /// O arquivo inteiro por mapeamento em memoria (nada e carregado alem do
    /// que for lido), pra o probe que `head` nao decide: num DVD de PS2 o
    /// diretorio raiz e o SYSTEM.CNF ficam depois dos 128 KiB, atras da ponte
    /// UDF. `None` se nao ha mais nada a ver (o arquivo cabe em `head`), se a
    /// entrada veio de bytes (teste) ou se o arquivo sumiu ou mudou de tamanho.
    pub fn whole(&self) -> Option<crate::fileio::FileBytes> {
        if self.size as usize <= self.head.len() {
            return None;
        }
        let view = crate::fileio::read_view(&self.path).ok()?;
        (view.len() as u64 == self.size).then_some(view)
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
    /// Traducoes de bloco comprimido que ficaram de fora: o bloco regravado
    /// ficou maior que o original e nao tem ponteiro em tabela pra mudar de
    /// lugar. O bloco inteiro fica como estava.
    pub packed_left_out: usize,
    /// Traducoes de mensagem de banco que ficaram de fora por falta de
    /// espaco: o bloco remontado nao coube em lugar nenhum do arquivo, ou
    /// passou do que a tabela dele alcanca. O bloco fica como estava.
    pub no_room: usize,
}

/// Imagem modificada em memoria + relatorio. O caller decide onde gravar
/// (sempre working copy — nunca o original).
#[derive(Debug)]
pub struct AppliedImage {
    pub bytes: Vec<u8>,
    pub report: ApplyReport,
}

/// Resultado da extracao estruturada: as entries e, em numero, o que ficou
/// de fora — nada e descartado em silencio.
#[derive(Debug, Clone, Default)]
pub struct Extraction {
    pub entries: Vec<TextEntry>,
    /// Sequencias descartadas por nao parecerem texto
    /// (`scan::looks_like_text`). String com ponteiro em tabela nunca entra
    /// aqui.
    pub discarded: usize,
    /// A varredura parou num teto (`scan::MAX_ENTRIES`): pode haver mais texto.
    pub truncated: bool,
    /// Tabelas de caracteres que a extracao montou sozinha, (id, conteudo do
    /// `.tbl`): as entries de encoding `Table(id)` sao lidas e regravadas por
    /// elas. Quem tem o projeto as grava em `tables/` (`tbl::ensure_table`).
    pub tables: Vec<(String, String)>,
}

/// O que o projeto ensinou a mao sobre o texto do jogo, pra extracao usar
/// (`GameAdapter::extract_with`).
#[derive(Debug, Clone, Default)]
pub struct ExtractHints {
    /// Alfabetos que valem depois de um comando, ate o fim da mensagem:
    /// (comando, codigo -> caractere). No jogo medido, as falas dos Sr.
    /// Saturno sao escritas com `FF0B` e uma fonte propria de 66 desenhos.
    pub alphabets: Vec<(u16, std::collections::HashMap<u16, char>)>,
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
    /// Quem varre bytes atras de texto descarta o que nao parece texto e conta
    /// (`Extraction::discarded`): entry de lixo aqui e traducao gravada por
    /// cima de tile ou de codigo.
    fn extract(&self, _data: &[u8]) -> Result<Extraction> {
        unsupported(self.id(), "extracao estruturada")
    }

    /// `extract` com as dicas do projeto (`ExtractHints`). O padrao as ignora.
    fn extract_with(&self, data: &[u8], _hints: &ExtractHints) -> Result<Extraction> {
        self.extract(data)
    }

    /// So as entries de `extract`.
    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        Ok(self.extract(data)?.entries)
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

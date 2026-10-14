pub(crate) mod bank;
pub(crate) mod cdrom;
pub mod gamecube;
pub mod gba;
pub(crate) mod inplace;
pub(crate) mod iso9660;
pub mod nds;
pub mod nes;
pub(crate) mod packed;
pub(crate) mod pointers;
pub mod ps1;
pub mod ps2;
pub mod psp;
pub mod rtsf;
pub mod snes;
pub mod wii;
pub mod wiiu;

use std::path::Path;

use serde::Serialize;

use crate::adapter::GameAdapter;
use crate::db::ProjectDb;
use crate::error::{CoreError, Result};

/// Registry dos adapters compilados no workspace (spec: sem plugin loading
/// dinamico no MVP). Ordem: magics fortes primeiro, heuristicos por ultimo.
pub fn all() -> Vec<Box<dyn GameAdapter>> {
    vec![
        Box::new(rtsf::RtsfAdapter),
        Box::new(nds::NdsAdapter),
        Box::new(gamecube::GameCubeAdapter),
        Box::new(wii::WiiAdapter),
        Box::new(wiiu::WiiUAdapter),
        Box::new(psp::PspAdapter),
        Box::new(ps2::Ps2Adapter),
        Box::new(ps1::Ps1Adapter),
        Box::new(nes::NesAdapter),
        Box::new(gba::GbaAdapter),
        Box::new(snes::SnesAdapter),
    ]
}

pub fn find(id: &str) -> Option<Box<dyn GameAdapter>> {
    all().into_iter().find(|a| a.id() == id)
}

/// O que a extracao estruturada de um projeto deixou no banco e o que ela
/// deixou de fora.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractOutcome {
    pub saved: usize,
    /// Sequencias descartadas por nao parecerem texto.
    pub discarded: usize,
    /// A varredura parou no teto: pode haver mais texto.
    pub truncated: bool,
    /// Entries antigas, sem traducao, que sairam do banco por nao parecerem
    /// texto (`ProjectDb::remove_stale_junk`).
    pub removed: usize,
    /// Entries fora das regioes de texto do arquivo, tiradas da traducao
    /// (`scan::stray_ids`): ficam como `Internal`, com desfazer por entry.
    pub stray: usize,
    /// Das salvas, quantas estavam dentro de blocos comprimidos.
    pub packed: usize,
    /// Das salvas, quantas sao texto japones em codigos de 16 bits, lido pela
    /// tabela que a extracao montou (`kuten`).
    pub jis16: usize,
    /// Das salvas, quantas estao numa mensagem de banco em blocos
    /// (`bank`): a traducao delas pode crescer.
    pub banked: usize,
}

/// Extracao estruturada do arquivo de origem de um projeto, gravada no banco.
/// O upsert e por id (`scan-<offset>` nos adapters que varrem), entao extrair
/// de novo atualiza limites e ponteiros e mantem as traducoes ja feitas.
/// Entry antiga que a extracao nao devolve mais so sai do banco se for lixo
/// sem traducao (projeto de antes do filtro); o resto continua.
pub fn extract_project(project_dir: &Path) -> Result<ExtractOutcome> {
    let game = crate::project::load_project(project_dir)?;
    let adapter = find(&game.adapter_id)
        .ok_or_else(|| CoreError::Project(format!("adapter {} nao existe", game.adapter_id)))?;
    // mmap: DVD de PS2 (>2 GiB) extrai sem carregar o arquivo em RAM.
    let data = crate::fileio::read_view(&game.source_path)?;
    let extraction = adapter.extract(&data)?;
    for (id, text) in &extraction.tables {
        crate::tbl::ensure_table(project_dir, id, text)?;
    }
    let jis16 = crate::types::TextEncoding::Table(crate::kuten::TABLE_ID.to_string());
    let mut db = ProjectDb::open(project_dir)?;
    let saved = db.upsert_entries(&extraction.entries)?;
    let removed =
        db.remove_stale_junk(&extraction.entries)? + db.remove_overlapped(&extraction.entries)?;
    let stray = db.mark_stray(&crate::scan::stray_ids(&extraction.entries))?;
    Ok(ExtractOutcome {
        saved,
        discarded: extraction.discarded,
        truncated: extraction.truncated,
        removed,
        stray,
        packed: extraction
            .entries
            .iter()
            .filter(|e| packed::is_packed(e))
            .count(),
        jis16: extraction
            .entries
            .iter()
            .filter(|e| e.encoding == jis16)
            .count(),
        banked: extraction
            .entries
            .iter()
            .filter(|e| bank::is_banked(e))
            .count(),
    })
}

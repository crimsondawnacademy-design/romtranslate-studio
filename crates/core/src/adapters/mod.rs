pub(crate) mod bank;
pub mod cartucho;
pub(crate) mod cdrom;
pub mod computador;
pub mod gamecube;
pub mod gba;
pub(crate) mod inplace;
pub(crate) mod iso9660;
pub(crate) mod mpt;
pub mod nds;
pub mod nes;
pub(crate) mod nftr;
pub(crate) mod packed;
pub(crate) mod pointers;
pub mod ps1;
pub mod ps2;
pub mod psp;
pub mod rtsf;
pub mod sega_disco;
pub mod snes;
pub mod wii;
pub mod wiiu;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::adapter::GameAdapter;
use crate::db::ProjectDb;
use crate::error::{CoreError, Result};
use crate::tbl::TblTable;
use crate::types::{TextEncoding, TextEntry};

/// Registry dos adapters compilados no workspace (spec: sem plugin loading
/// dinamico no MVP). Ordem: magics fortes primeiro, heuristicos por ultimo.
pub fn all() -> Vec<Box<dyn GameAdapter>> {
    let mut adapters: Vec<Box<dyn GameAdapter>> = vec![
        Box::new(rtsf::RtsfAdapter),
        Box::new(nds::NdsAdapter),
        Box::new(gamecube::GameCubeAdapter),
        Box::new(wii::WiiAdapter),
        Box::new(wiiu::WiiUAdapter),
        Box::new(psp::PspAdapter),
        Box::new(ps2::Ps2Adapter),
        Box::new(ps1::Ps1Adapter),
        Box::new(sega_disco::SegaDisc(&sega_disco::DISCS[0])),
        Box::new(sega_disco::SegaDisc(&sega_disco::DISCS[1])),
        Box::new(nes::NesAdapter),
        Box::new(gba::GbaAdapter),
        Box::new(snes::SnesAdapter),
    ];
    // Computadores japoneses: o formato da imagem (D88, FDI, HDI, DIM, XDF)
    // fecha pelo tamanho, antes dos cartuchos que so a extensao reconhece.
    adapters.extend(
        computador::MACHINES
            .iter()
            .map(|m| Box::new(computador::Computador(m)) as Box<dyn GameAdapter>),
    );
    // Cartuchos com cabecalho forte antes; MSX e PC Engine (extensao) por
    // ultimo, na ordem da ficha.
    adapters.extend(
        cartucho::CONSOLES
            .iter()
            .map(|c| Box::new(cartucho::Cartucho(c)) as Box<dyn GameAdapter>),
    );
    adapters
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
    /// Traducoes ja feitas relidas com os comandos na forma da leitura de
    /// agora (`TblTable::reread`): o original mudou so na forma, os bytes sao
    /// os mesmos.
    pub reread: usize,
}

/// As dicas do projeto pra extracao (`ExtractHints`): cada
/// `tables/<tabela>@<COMANDO>.tbl` (ex.: `jis16@FF0B.tbl`) e o alfabeto que
/// vale depois do comando, uma linha por codigo, a chave como na tabela
/// principal (os 2 bytes do codigo, o menos significativo primeiro):
/// `0100=あ` e o codigo 1. Feito a mao, olhando a fonte do jogo no emulador.
pub fn hints(project_dir: &Path) -> Result<crate::adapter::ExtractHints> {
    let mut hints = crate::adapter::ExtractHints {
        source_language: crate::project::load_project(project_dir)
            .ok()
            .and_then(|project| project.source_language),
        ..Default::default()
    };
    let dir = project_dir.join(crate::tbl::TABLES_DIR);
    let Ok(list) = std::fs::read_dir(&dir) else {
        return Ok(hints);
    };
    let mut files: Vec<PathBuf> = list.filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.sort();
    for path in files {
        let Some(stem) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".tbl"))
        else {
            continue;
        };
        let Some(command) = stem
            .split_once('@')
            .and_then(|(_, hex)| u16::from_str_radix(hex, 16).ok())
        else {
            continue;
        };
        let text = std::fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
        let mut alphabet = HashMap::new();
        for line in text.lines().map(|line| line.trim_end_matches('\r')) {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (Ok(code), Some(c)) = (u16::from_str_radix(key, 16), value.chars().next()) else {
                continue;
            };
            if key.len() == 4 && !line.starts_with('#') {
                alphabet.insert(code.swap_bytes(), c);
            }
        }
        hints.alphabets.push((command, alphabet));
    }
    Ok(hints)
}

/// Codigo sem letra conhecida num alfabeto ensinado (`alphabets`,
/// `save_alphabet`): ocupa o lugar do codigo e nao e gravado.
pub const UNKNOWN_LETTER: char = '〓';

/// Grava o alfabeto que vale depois de `command`
/// (`tables/jis16@<COMANDO>.tbl`, lido por `hints`): `letters` sao as letras
/// dos codigos 1, 2, 3... na ordem; espaco e quebra de linha nao contam e
/// `UNKNOWN_LETTER` pula o codigo. Sem letra nenhuma, o alfabeto sai do
/// projeto. A extracao seguinte le as falas com ele.
pub fn save_alphabet(project_dir: &Path, command: u16, letters: &str) -> Result<PathBuf> {
    let dir = project_dir.join(crate::tbl::TABLES_DIR);
    let path = dir.join(format!("{}@{command:04X}.tbl", crate::kuten::TABLE_ID));
    let mut text = String::new();
    let letters = letters.chars().filter(|c| !c.is_whitespace());
    for (code, c) in (1..=u16::MAX).zip(letters) {
        if c != UNKNOWN_LETTER {
            // A chave e a da tabela principal: os 2 bytes do codigo, o menos
            // significativo primeiro.
            text.push_str(&format!("{:04X}={c}\n", code.swap_bytes()));
        }
    }
    if text.is_empty() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(CoreError::io(&path, e))
            }
            _ => return Ok(path),
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    std::fs::write(&path, text).map_err(|e| CoreError::io(&path, e))?;
    Ok(path)
}

/// Os alfabetos ensinados no projeto: (comando, letras dos codigos 1, 2,
/// 3... na ordem, com `UNKNOWN_LETTER` no codigo sem letra), na forma que
/// `save_alphabet` recebe.
pub fn alphabets(project_dir: &Path) -> Result<Vec<(u16, String)>> {
    Ok(hints(project_dir)?
        .alphabets
        .into_iter()
        .map(|(command, alphabet)| {
            let last = alphabet.keys().copied().max().unwrap_or(0);
            let letters = (1..=last)
                .map(|code| alphabet.get(&code).copied().unwrap_or(UNKNOWN_LETTER))
                .collect();
            (command, letters)
        })
        .collect())
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
    // Com perfil do jogo, o texto e o que ele descreve: a varredura do adapter
    // fica de fora (num DS ela traz o video e o som lidos como frase).
    let profile = crate::profile::load(project_dir)?;
    let extraction = match &profile {
        Some(profile) => crate::profile::extract(project_dir, &data, profile)?,
        None => adapter.extract_with(&data, &hints(project_dir)?)?,
    };
    // A tabela de antes fica a mao: e com ela que as traducoes ja feitas
    // foram escritas.
    let mut older = HashMap::new();
    for (id, text) in &extraction.tables {
        let path = project_dir
            .join(crate::tbl::TABLES_DIR)
            .join(format!("{id}.tbl"));
        if let Ok(table) = TblTable::load(&path) {
            older.insert(id.clone(), table);
        }
        crate::tbl::ensure_table(project_dir, id, text)?;
    }
    let jis16 = crate::types::TextEncoding::Table(crate::kuten::TABLE_ID.to_string());
    let mut db = ProjectDb::open(project_dir)?;
    let before: HashMap<String, TextEntry> = db
        .load_entries()?
        .into_iter()
        .map(|e| (e.id.clone(), e))
        .collect();
    let saved = db.upsert_entries(&extraction.entries)?;
    // O original que mudou so na forma dos comandos (agora a leitura sabe que
    // um deles leva parametro) leva junto a traducao que ja tinha: os mesmos
    // bytes, relidos pela tabela nova. Sem isto, cada uma delas virava erro de
    // codigo faltando e voltava ao provider.
    let mut tables = crate::tbl::ProjectTables::new(project_dir);
    let mut reread = 0;
    for entry in &extraction.entries {
        let TextEncoding::Table(id) = &entry.encoding else {
            continue;
        };
        let (Some(old), Some(older)) = (before.get(&entry.id), older.get(id)) else {
            continue;
        };
        let Some(translation) = old.translated_text.as_deref() else {
            continue;
        };
        if old.source_text == entry.source_text {
            continue;
        }
        let table = tables.get(id)?;
        if let Some(text) = table
            .reread(older, translation)
            .filter(|text| text != translation)
        {
            db.update_translation(&entry.id, &text, old.status)?;
            reread += 1;
        }
    }
    let mut removed =
        db.remove_stale_junk(&extraction.entries)? + db.remove_overlapped(&extraction.entries)?;
    if profile.is_some() {
        removed += db.remove_untranslated_absent(&extraction.entries)?;
    }
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
        reread,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taught_alphabet_goes_back_and_forth_through_the_table_file() {
        let dir = std::env::temp_dir().join(format!("rts-alphabet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = save_alphabet(&dir, 0xFF0B, "あい 〓\nう").unwrap();
        assert!(path.ends_with("tables/jis16@FF0B.tbl"), "{path:?}");
        // Codigo 1 = あ, com o byte menos significativo primeiro; o 3 pulado.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "0100=あ\n0200=い\n0400=う\n"
        );
        let hint = &hints(&dir).unwrap().alphabets[0];
        assert_eq!((hint.0, hint.1.get(&4)), (0xFF0B, Some(&'う')));
        assert_eq!(
            alphabets(&dir).unwrap(),
            vec![(0xFF0B, "あい〓う".to_string())]
        );
        // Sem letra, o alfabeto sai.
        save_alphabet(&dir, 0xFF0B, " ").unwrap();
        assert!(!path.exists());
        assert!(alphabets(&dir).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

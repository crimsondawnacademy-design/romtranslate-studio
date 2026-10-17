//! Imagem montada por mapa de quadros, pelo projeto: a leitura da imagem
//! montada toma o lugar da leitura dos mesmos quadros na ordem guardada (senao
//! a mesma palavra seria redesenhada duas vezes, uma delas fora do lugar).

use std::fs;
use std::path::PathBuf;

use romtranslate_core::db::ProjectDb;
use romtranslate_core::images::{self, Reader};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::synth::{self, TILEMAP_MAP_AT, TILEMAP_TILES_AT};
use romtranslate_core::types::{Platform, TextEncoding};

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// O numero (de 1 em diante) do trecho da folha em `y`.
fn strip_at(data: &[u8], picture: &images::Picture, y: usize) -> usize {
    let (_, sheet) = images::open(data, picture).unwrap();
    images::blocks(&sheet)
        .iter()
        .position(|b| b.y == y)
        .unwrap_or_else(|| panic!("nenhum trecho em y = {y} de {}", picture.id))
        + 1
}

#[tokio::test]
async fn reading_the_assembled_picture_replaces_the_reading_in_stored_order() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = TempDir(std::env::temp_dir().join(format!("rts-tilemap-{nanos}")));
    fs::create_dir_all(&tmp.0).unwrap();
    let rom = synth::make_gba_rom_with_tilemap();
    let source = tmp.0.join("game.gba");
    fs::write(&source, &rom).unwrap();
    let dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: source,
        platform: Platform::Gba,
        adapter_id: "gba.generic".to_string(),
        source_language: Some("ja".to_string()),
        target_language: "pt-BR".to_string(),
        project_dir: dir.clone(),
    })
    .unwrap();

    let found = images::find(&rom);
    let raw = found.iter().find(|p| p.map.is_none()).unwrap();
    let map = found
        .iter()
        .find(|p| p.map.is_some_and(|m| m.offset == TILEMAP_MAP_AT))
        .unwrap();
    // A linha 0 da lista cabe numa tira: legivel nas duas folhas. Na ordem
    // guardada, a tira 0 fica nas linhas 1 e 2 do bloco.
    let block_row = (TILEMAP_TILES_AT - raw.offset) / 32 / 32 + 1;
    let stored = strip_at(&rom, raw, block_row * 8 + 3);
    let (list_row, _) = synth::TILEMAP_LIST_CELL;
    let shown = strip_at(&rom, map, list_row * 8 + 3);
    let answer = |n: usize| format!("{{\"texts\": {{\"{n}\": \"ことば\"}}}}");

    let read = images::read_pictures(
        &dir,
        Reader::Answer(&answer(stored)),
        std::slice::from_ref(&raw.id),
    )
    .await
    .unwrap();
    assert_eq!(read.entries, 1, "{read:?}");
    let read = images::read_pictures(
        &dir,
        Reader::Answer(&answer(shown)),
        std::slice::from_ref(&map.id),
    )
    .await
    .unwrap();
    assert_eq!(read.entries, 1, "{read:?}");

    let ids: Vec<String> = ProjectDb::open(&dir)
        .unwrap()
        .load_entries()
        .unwrap()
        .into_iter()
        .filter(|e| e.encoding == TextEncoding::Image)
        .map(|e| e.id)
        .collect();
    assert_eq!(ids, [format!("{}-{shown:02}", map.id)]);
}

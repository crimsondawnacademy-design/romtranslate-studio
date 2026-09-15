//! GameCube: filesystem FST (formato confirmado no Dolphin), extracao por
//! arquivo e reinsercao in-place conservadora.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::gamecube::{GameCubeAdapter, FST_OFFSET_FIELD, FST_SIZE_FIELD};
use romtranslate_core::db::ProjectDb;
use romtranslate_core::patch::{apply_ips, export_patch};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::reinsert_project;
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, TranslationStatus};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-gc-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fst_lists_files_with_directory_paths() {
    let disc = synth::make_gc_disc();
    let files = GameCubeAdapter.list_resources(&disc).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, "opening.txt");
    assert_eq!(files[1].path, "data/config.bin", "subdiretorio no path");
    assert_eq!(files[0].offset, 0x2000);
    assert!(files[1].size > 0);
}

#[test]
fn fst_lying_headers_error_cleanly_never_panic() {
    let disc = synth::make_gc_disc();

    let mut lying = disc.clone();
    lying[FST_OFFSET_FIELD..FST_OFFSET_FIELD + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(GameCubeAdapter.list_resources(&lying).is_err());

    let mut lying2 = disc.clone();
    lying2[FST_SIZE_FIELD..FST_SIZE_FIELD + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(GameCubeAdapter.list_resources(&lying2).is_err());

    // Entry de arquivo apontando para fora do disco.
    let mut lying3 = disc.clone();
    let fst_offset = 0x1000;
    lying3[fst_offset + 12 + 4..fst_offset + 12 + 8].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(GameCubeAdapter.list_resources(&lying3).is_err());

    for len in (0..disc.len()).step_by(251) {
        let _ = GameCubeAdapter.extract_structured(&disc[..len]);
    }
}

#[test]
fn extract_tags_resource_paths_including_boot_header() {
    let disc = synth::make_gc_disc();
    let entries = GameCubeAdapter.extract_structured(&disc).unwrap();

    let by_text = |t: &str| entries.iter().find(|e| e.source_text == t);
    // O ultimo byte do magic (0x3D, '=') e printable e cola no titulo — o run
    // do scanner vira "=SYNTHETIC...". Comportamento esperado de scan generico.
    let title = entries
        .iter()
        .find(|e| e.source_text.contains("SYNTHETIC GC ADVENTURE"))
        .expect("titulo do boot.bin");
    assert_eq!(title.resource_path.as_deref(), Some("boot.bin"));

    let intro = by_text("WELCOME TO GAMECUBE ISLAND!").expect("string do arquivo");
    assert_eq!(intro.resource_path.as_deref(), Some("opening.txt"));
    assert_eq!(intro.max_bytes, Some(27));

    let sound = by_text("SOUND OPTIONS").expect("string do subdiretorio");
    assert_eq!(sound.resource_path.as_deref(), Some("data/config.bin"));
}

#[test]
fn gc_roundtrip_and_full_project_flow() {
    let disc = synth::make_gc_disc();
    let mut entries = GameCubeAdapter.extract_structured(&disc).unwrap();
    for e in entries.iter_mut() {
        e.translated_text = match e.source_text.as_str() {
            "WELCOME TO GAMECUBE ISLAND!" => Some("BEM-VINDO A ILHA GC!".to_string()),
            "SOUND OPTIONS" => Some("OPCOES DE SOM".to_string()),
            _ => None,
        };
        if e.translated_text.is_some() {
            e.status = TranslationStatus::Machine;
        }
    }

    let applied = GameCubeAdapter.apply_text(&disc, &entries).unwrap();
    assert_eq!(applied.report.applied, 2);
    let verification = GameCubeAdapter.verify(&applied.bytes).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);

    let reread = GameCubeAdapter.extract_structured(&applied.bytes).unwrap();
    assert!(reread
        .iter()
        .any(|e| e.source_text == "BEM-VINDO A ILHA GC!"));
    assert!(reread.iter().any(|e| e.source_text == "OPCOES DE SOM"));

    // Fluxo projeto -> reinsercao -> patch, original intacto.
    let tmp = TempDir::new("flow");
    let disc_path = tmp.0.join("game.iso");
    fs::write(&disc_path, &disc).unwrap();
    let project_dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: disc_path.clone(),
        platform: Platform::GameCube,
        adapter_id: "gamecube.generic".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();
    let mut db = ProjectDb::open(&project_dir).unwrap();
    db.upsert_entries(&entries).unwrap();
    drop(db);

    let reinserted = reinsert_project(&project_dir, false).unwrap();
    assert!(reinserted.verification.ok);
    assert_eq!(fs::read(&disc_path).unwrap(), disc, "original intacto");

    let outcome = export_patch(&project_dir, None).unwrap();
    let working = fs::read(&reinserted.working_path).unwrap();
    let patch = fs::read(&outcome.patch_path).unwrap();
    assert_eq!(apply_ips(&disc, &patch).unwrap(), working);
}

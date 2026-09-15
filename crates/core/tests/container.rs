//! Integracao do Sprint 8: NDS (probe/filesystem/extracao/round-trip in-place)
//! e probes de disco GameCube/Wii.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::{GameAdapter, GameInput};
use romtranslate_core::adapters::nds::{crc16, NdsAdapter};
use romtranslate_core::db::ProjectDb;
use romtranslate_core::detect::inspect;
use romtranslate_core::patch::{apply_ips, export_patch};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::reinsert_project;
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, TextEncoding, TranslationStatus};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-cont-{label}-{nanos}"));
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
fn crc16_matches_known_vector() {
    // CRC-16/MODBUS (poly 0xA001, init 0xFFFF) de "123456789" = 0x4B37.
    assert_eq!(crc16(b"123456789"), 0x4B37);
}

#[test]
fn nds_probe_detects_fixture_and_rejects_garbage() {
    let rom = synth::make_nds_rom();
    let result = NdsAdapter.probe(&GameInput::from_bytes("t.nds", &rom));
    assert!(
        result.confidence >= 0.9,
        "confidence: {}",
        result.confidence
    );
    assert_eq!(result.platform, Platform::Nds);
    assert!(result.evidence.iter().any(|e| e.contains("CRC-16")));
    assert!(
        result.evidence.iter().any(|e| e.contains("2 arquivos")),
        "{:?}",
        result.evidence
    );

    assert_eq!(
        NdsAdapter
            .probe(&GameInput::from_bytes(
                "r.bin",
                &synth::make_random(4096, 5)
            ))
            .confidence,
        0.0
    );
    for len in (0..0x220).step_by(31) {
        NdsAdapter.probe(&GameInput::from_bytes("t.nds", &rom[..len.min(rom.len())]));
    }
}

#[test]
fn nds_filesystem_lists_files_and_rejects_lying_headers() {
    let rom = synth::make_nds_rom();
    let files = NdsAdapter.list_resources(&rom).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, "intro.txt");
    assert_eq!(files[1].path, "menu.bin");
    assert!(files[0].offset >= 0x200 && files[0].size > 0);

    // FNT apontando pra fora do arquivo.
    let mut lying = rom.clone();
    lying[0x40..0x44].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(NdsAdapter.list_resources(&lying).is_err());
    // FAT com tamanho absurdo.
    let mut lying2 = rom.clone();
    lying2[0x4C..0x50].copy_from_slice(&0xFFFF_FF00u32.to_le_bytes());
    assert!(NdsAdapter.list_resources(&lying2).is_err());
}

#[test]
fn nds_extract_tags_resource_paths_and_finds_utf16() {
    let rom = synth::make_nds_rom();
    let entries = NdsAdapter.extract_structured(&rom).unwrap();

    let by_text = |t: &str| entries.iter().find(|e| e.source_text == t);
    let title = by_text("SYNTHDS").expect("titulo do header");
    assert_eq!(title.resource_path.as_deref(), Some("header"));

    let intro = by_text("WELCOME TO THE SYNTH DS!").expect("string ascii");
    assert_eq!(intro.resource_path.as_deref(), Some("intro.txt"));
    assert_eq!(intro.max_bytes, Some(24));

    let menu = by_text("START GAME").expect("string utf16");
    assert_eq!(menu.resource_path.as_deref(), Some("menu.bin"));
    assert_eq!(menu.encoding, TextEncoding::Utf16Le);
    assert_eq!(menu.max_bytes, Some(20));
}

#[test]
fn nds_roundtrip_ascii_and_utf16_recalculates_header_crc() {
    let rom = synth::make_nds_rom();
    let mut entries = NdsAdapter.extract_structured(&rom).unwrap();
    for e in entries.iter_mut() {
        e.translated_text = match e.source_text.as_str() {
            "WELCOME TO THE SYNTH DS!" => Some("BEM-VINDO AO SYNTH DS!".to_string()),
            "START GAME" => Some("INICIAR".to_string()), // utf16: 14 bytes <= 20
            "SYNTHDS" => Some("DSSINT".to_string()),     // titulo: muda o CRC do header
            _ => None,
        };
    }

    let applied = NdsAdapter.apply_text(&rom, &entries).unwrap();
    assert_eq!(applied.report.applied, 3);

    let verification = NdsAdapter.verify(&applied.bytes).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);
    assert_ne!(
        &rom[0x15E..0x160],
        &applied.bytes[0x15E..0x160],
        "CRC do header recalculado"
    );

    let reread = NdsAdapter.extract_structured(&applied.bytes).unwrap();
    let texts: Vec<_> = reread.iter().map(|e| e.source_text.as_str()).collect();
    assert!(texts.contains(&"BEM-VINDO AO SYNTH DS!"));
    assert!(texts.contains(&"INICIAR"));
    assert!(texts.contains(&"DSSINT"));
    assert!(texts.contains(&"OPTIONS MENU"), "nao traduzida intacta");

    // Overflow em utf16 (traducao maior que o espaco) recusa limpo.
    let mut too_long = NdsAdapter.extract_structured(&rom).unwrap();
    for e in too_long
        .iter_mut()
        .filter(|e| e.source_text == "START GAME")
    {
        e.translated_text = Some("INICIAR NOVO JOGO AGORA".to_string()); // 46 bytes > 20
    }
    assert!(NdsAdapter.apply_text(&rom, &too_long).is_err());
}

#[test]
fn nds_full_project_flow_reinsert_and_patch() {
    let tmp = TempDir::new("flow");
    let rom = synth::make_nds_rom();
    let rom_path = tmp.0.join("game.nds");
    fs::write(&rom_path, &rom).unwrap();
    let project_dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: rom_path.clone(),
        platform: Platform::Nds,
        adapter_id: "nds.generic".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();

    let mut entries = NdsAdapter.extract_structured(&rom).unwrap();
    for e in entries.iter_mut() {
        if e.source_text == "PRESS START BUTTON" {
            e.translated_text = Some("APERTE START".to_string());
            e.status = TranslationStatus::Machine;
        }
    }
    let mut db = ProjectDb::open(&project_dir).unwrap();
    db.upsert_entries(&entries).unwrap();
    drop(db);

    let reinserted = reinsert_project(&project_dir, false).unwrap();
    assert!(reinserted.verification.ok);
    assert_eq!(fs::read(&rom_path).unwrap(), rom, "original intacto");

    let outcome = export_patch(&project_dir, None).unwrap();
    let working = fs::read(&reinserted.working_path).unwrap();
    let patch = fs::read(&outcome.patch_path).unwrap();
    assert_eq!(apply_ips(&rom, &patch).unwrap(), working);
    assert!(working.windows(12).any(|w| w == b"APERTE START"));
}

#[test]
fn gamecube_and_wii_probes_detect_discs() {
    let tmp = TempDir::new("disc");

    let gc_path = tmp.0.join("game.iso");
    fs::write(&gc_path, synth::make_gc_disc_header()).unwrap();
    let report = inspect(&gc_path).unwrap();
    let best = report.best.expect("gc detectado");
    assert_eq!(best.platform, Platform::GameCube);
    assert!(best.confidence >= 0.93);
    assert!(best
        .evidence
        .iter()
        .any(|e| e.contains("SYNTHETIC GC ADVENTURE")));

    let wii_path = tmp.0.join("wii.iso");
    fs::write(&wii_path, synth::make_wii_disc_header()).unwrap();
    let report = inspect(&wii_path).unwrap();
    let best = report.best.expect("wii detectado");
    assert_eq!(best.platform, Platform::Wii);
    assert!(best.evidence.iter().any(|e| e.contains("cifradas")));

    // WBFS: container Wii.
    let mut wbfs = vec![0u8; 1024];
    wbfs[0..4].copy_from_slice(b"WBFS");
    let wbfs_path = tmp.0.join("game.wbfs");
    fs::write(&wbfs_path, &wbfs).unwrap();
    let report = inspect(&wbfs_path).unwrap();
    assert_eq!(report.best.expect("wbfs detectado").platform, Platform::Wii);

    // Probes de disco nao suportam extracao estruturada.
    use romtranslate_core::adapters::gamecube::GameCubeAdapter;
    assert!(GameCubeAdapter
        .extract_structured(&synth::make_gc_disc_header())
        .is_err());
}

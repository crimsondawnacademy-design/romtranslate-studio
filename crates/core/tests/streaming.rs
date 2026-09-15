//! Reinsercao streaming (imagens acima do teto em memoria): o caminho e
//! exercitado com teto injetado bem baixo, num ISO pequeno — o resultado
//! tem que ser byte a byte IDENTICO ao caminho em memoria.

use std::fs;
use std::path::{Path, PathBuf};

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::ps2::Ps2Adapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::patch::export_patch;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::{reinsert_project, reinsert_project_with_limit};
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, TranslationStatus};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-stream-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn setup_ps2_project(tmp: &TempDir, name: &str, iso: &[u8], iso_path: &Path) -> PathBuf {
    let project_dir = tmp.0.join(format!("{name}.rtsproj"));
    create_project(CreateProjectArgs {
        source_path: iso_path.to_path_buf(),
        platform: Platform::Ps2,
        adapter_id: "ps2.generic".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();
    let mut entries = Ps2Adapter.extract_structured(iso).unwrap();
    for e in entries.iter_mut() {
        if e.source_text == "SAVE PROGRESS?" {
            e.translated_text = Some("SALVAR JOGO?".to_string());
            e.status = TranslationStatus::Machine;
        }
    }
    ProjectDb::open(&project_dir)
        .unwrap()
        .upsert_entries(&entries)
        .unwrap();
    project_dir
}

#[test]
fn streaming_reinsert_matches_in_memory_byte_for_byte() {
    let tmp = TempDir::new("diff");
    let iso = synth::make_ps2_iso();
    let iso_path = tmp.0.join("game.iso");
    fs::write(&iso_path, &iso).unwrap();

    let dir_mem = setup_ps2_project(&tmp, "mem", &iso, &iso_path);
    let dir_stream = setup_ps2_project(&tmp, "stream", &iso, &iso_path);

    let mem = reinsert_project(&dir_mem, false).unwrap();
    // Teto de 1 KiB forca o caminho streaming no mesmo ISO pequeno.
    let stream = reinsert_project_with_limit(&dir_stream, false, 1024).unwrap();

    assert_eq!(mem.apply.applied, stream.apply.applied);
    assert!(stream.verification.ok, "{:?}", stream.verification.problems);
    assert_eq!(
        fs::read(&mem.working_path).unwrap(),
        fs::read(&stream.working_path).unwrap(),
        "streaming e memoria tem que produzir a MESMA imagem"
    );
    assert_eq!(fs::read(&iso_path).unwrap(), iso, "original intacto");

    // O resto do fluxo (patch) funciona em cima da working copy streaming.
    let outcome = export_patch(&dir_stream, None).unwrap();
    assert!(outcome.patch_path.is_file());
}

#[test]
fn streaming_refuses_non_iso_2048_formats() {
    let tmp = TempDir::new("refuse");
    let bin = synth::make_ps1_bin();
    let bin_path = tmp.0.join("game.bin");
    fs::write(&bin_path, &bin).unwrap();
    let project_dir = tmp.0.join("ps1.rtsproj");
    create_project(CreateProjectArgs {
        source_path: bin_path,
        platform: Platform::Ps1,
        adapter_id: "ps1.generic".into(),
        source_language: None,
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();
    let mut entries = romtranslate_core::adapters::ps1::Ps1Adapter
        .extract_structured(&bin)
        .unwrap();
    for e in entries.iter_mut() {
        if e.source_text == "INSERT COIN TO CONTINUE" {
            e.translated_text = Some("INSIRA FICHA".to_string());
            e.status = TranslationStatus::Machine;
        }
    }
    ProjectDb::open(&project_dir)
        .unwrap()
        .upsert_entries(&entries)
        .unwrap();

    // Raw 2352 acima do teto: recusa clara (EDC/ECC exige o caminho em memoria).
    let err = reinsert_project_with_limit(&project_dir, false, 1024).unwrap_err();
    assert!(err.to_string().contains("2048"), "{err}");
    // No teto normal o mesmo projeto reinsere sem drama.
    assert!(
        reinsert_project(&project_dir, false)
            .unwrap()
            .verification
            .ok
    );
}

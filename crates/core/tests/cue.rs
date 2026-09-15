//! Cue sheet multi-track: resolucao pro track de dados (single-file e
//! multi-file), fluxo inspect/projeto via .cue e roundtrip numa imagem
//! com setores de audio anexados.

use std::fs;
use std::path::{Path, PathBuf};

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::ps1::Ps1Adapter;
use romtranslate_core::cue::{resolve_cue, resolve_source};
use romtranslate_core::detect::inspect;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::synth;
use romtranslate_core::types::Platform;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-cue-{label}-{nanos}"));
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
fn cue_resolves_to_data_track_and_project_uses_the_bin() {
    let tmp = TempDir::new("resolve");
    let bin_path = tmp.0.join("Jogo Sintetico (BR).bin");
    fs::write(&bin_path, synth::make_ps1_bin_with_audio().0).unwrap();
    let cue_path = tmp.0.join("Jogo Sintetico (BR).cue");
    fs::write(
        &cue_path,
        "FILE \"Jogo Sintetico (BR).bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\n  TRACK 02 AUDIO\r\n    INDEX 00 00:04:00\r\n    INDEX 01 00:06:00\r\n",
    )
    .unwrap();

    let report = inspect(&cue_path).unwrap();
    assert_eq!(report.path, bin_path, "report aponta pro BIN resolvido");
    let best = report.best.expect("ps1 detectado via cue");
    assert_eq!(best.platform, Platform::Ps1);
    assert!(
        best.evidence
            .iter()
            .any(|e| e.contains("cue sheet") && e.contains("1 track(s) de audio")),
        "{:?}",
        best.evidence
    );

    let project = create_project(CreateProjectArgs {
        source_path: cue_path,
        platform: Platform::Ps1,
        adapter_id: "ps1.generic".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: tmp.0.join("jogo.rtsproj"),
    })
    .unwrap();
    assert_eq!(
        project.source_path, bin_path,
        "projeto guarda o BIN, nao o cue"
    );
}

#[test]
fn multifile_cue_resolves_even_without_audio_files_on_disk() {
    let tmp = TempDir::new("multifile");
    let data = tmp.0.join("data.bin");
    fs::write(&data, synth::make_ps1_bin()).unwrap();
    let cue = tmp.0.join("game.cue");
    fs::write(
        &cue,
        "FILE \"data.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\nFILE \"faixa02.bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n",
    )
    .unwrap();
    let r = resolve_cue(&cue).unwrap();
    assert_eq!(r.bin_path, data);
    assert_eq!((r.data_track, r.total_tracks, r.audio_tracks), (1, 2, 1));
    assert_eq!(r.data_mode, "MODE2/2352");
}

#[test]
fn single_file_multitrack_roundtrip_keeps_audio_intact() {
    let (bin, audio_sectors) = synth::make_ps1_bin_with_audio();
    let audio_start = bin.len() - audio_sectors * 2352;

    // Verify limpo: setores de audio (sem sync) ignorados, nao "invalidos".
    let baseline = Ps1Adapter.verify(&bin).unwrap();
    assert!(baseline.ok, "{:?}", baseline.problems);
    assert!(
        baseline.checks.iter().any(|c| c.contains("sem sync")),
        "{:?}",
        baseline.checks
    );

    let mut entries = Ps1Adapter.extract_structured(&bin).unwrap();
    for e in entries.iter_mut() {
        if e.source_text == "INSERT COIN TO CONTINUE" {
            e.translated_text = Some("INSIRA FICHA".to_string());
        }
    }
    let applied = Ps1Adapter.apply_text(&bin, &entries).unwrap();
    assert_eq!(applied.report.applied, 1);
    assert_eq!(
        &applied.bytes[audio_start..],
        &bin[audio_start..],
        "audio intacto byte a byte"
    );
    let verification = Ps1Adapter.verify(&applied.bytes).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);
}

#[test]
fn cue_errors_are_clear_and_non_cue_passes_through() {
    let tmp = TempDir::new("errors");
    let write_cue = |name: &str, body: &str| {
        let p = tmp.0.join(name);
        fs::write(&p, body).unwrap();
        p
    };

    let audio_only = write_cue(
        "audio.cue",
        "FILE \"a.bin\" BINARY\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n",
    );
    assert!(resolve_cue(&audio_only)
        .unwrap_err()
        .to_string()
        .contains("nenhum track de dados"));

    let not_first = write_cue(
        "notfirst.cue",
        "FILE \"a.bin\" BINARY\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 MODE2/2352\nINDEX 01 03:00:00\n",
    );
    assert!(resolve_cue(&not_first)
        .unwrap_err()
        .to_string()
        .contains("nao e o primeiro"));

    let offset = write_cue(
        "offset.cue",
        "FILE \"a.bin\" BINARY\nTRACK 01 MODE2/2352\nINDEX 00 00:00:00\nINDEX 01 00:02:00\n",
    );
    assert!(resolve_cue(&offset)
        .unwrap_err()
        .to_string()
        .contains("INDEX 01 00:00:00"));

    let missing = write_cue(
        "missing.cue",
        "FILE \"nao_existe.bin\" BINARY\nTRACK 01 MODE2/2352\nINDEX 01 00:00:00\n",
    );
    assert!(resolve_cue(&missing)
        .unwrap_err()
        .to_string()
        .contains("nao_existe.bin"));

    let garbage = write_cue("garbage.cue", "\u{0}\u{1}isto nao e um cue\n");
    assert!(resolve_cue(&garbage).is_err());

    // Arquivo que nao e .cue passa direto, sem tocar o disco.
    let (path, cue) = resolve_source(Path::new("qualquer.bin")).unwrap();
    assert_eq!(path, Path::new("qualquer.bin"));
    assert!(cue.is_none());
}

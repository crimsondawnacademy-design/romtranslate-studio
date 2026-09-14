//! Testes de integracao: inspecao em arquivos reais no disco + ciclo de projeto.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::detect::inspect;
use romtranslate_core::project::{create_project, load_project, CreateProjectArgs};
use romtranslate_core::synth;
use romtranslate_core::types::Platform;

/// Diretorio temporario unico por teste, limpo no drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-test-{label}-{nanos}"));
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
fn inspect_detects_each_synthetic_platform() {
    let tmp = TempDir::new("detect");
    let cases: [(&str, Vec<u8>, Platform); 4] = [
        ("game.gba", synth::make_gba_rom("SYNTHRPG"), Platform::Gba),
        ("game.nes", synth::make_nes_rom(), Platform::Nes),
        (
            "game.sfc",
            synth::make_snes_lorom("SYNTHETIC QUEST"),
            Platform::Snes,
        ),
        (
            "game.smc",
            synth::make_snes_headered("SYNTHETIC QUEST"),
            Platform::Snes,
        ),
    ];

    for (name, bytes, platform) in cases {
        let path = tmp.0.join(name);
        fs::write(&path, &bytes).unwrap();
        let report = inspect(&path).unwrap();
        let best = report.best.expect(name);
        assert_eq!(best.platform, platform, "{name}");
        assert!(!best.evidence.is_empty(), "{name}: evidencia vazia");
        assert_eq!(report.size, bytes.len() as u64);
        assert_eq!(report.sha256.len(), 64);
    }
}

#[test]
fn inspect_random_and_empty_files_yield_no_best_match() {
    let tmp = TempDir::new("nomatch");
    for (name, bytes) in [
        ("random.bin", synth::make_random(64 * 1024, 99)),
        ("empty.bin", Vec::new()),
        ("tiny.bin", vec![0x42; 7]),
    ] {
        let path = tmp.0.join(name);
        fs::write(&path, &bytes).unwrap();
        let report = inspect(&path).unwrap();
        assert!(report.best.is_none(), "{name} nao deveria ter match");
    }
}

/// Probes nunca panicam com arquivos truncados em qualquer tamanho.
#[test]
fn truncated_inputs_never_panic() {
    let tmp = TempDir::new("trunc");
    let full = synth::make_snes_headered("SYNTHETIC QUEST");
    for len in (0..0x600).step_by(37).chain([0x7FC0, 0x8000, 0x81FF]) {
        let path = tmp.0.join(format!("t{len}.bin"));
        fs::write(&path, &full[..len.min(full.len())]).unwrap();
        inspect(&path).unwrap();
    }
}

#[test]
fn project_roundtrip_preserves_data_and_source() {
    let tmp = TempDir::new("project");
    let rom_path = tmp.0.join("game.gba");
    let rom = synth::make_gba_rom("SYNTHRPG");
    fs::write(&rom_path, &rom).unwrap();

    let project_dir = tmp.0.join("game.rtsproj");
    let created = create_project(CreateProjectArgs {
        source_path: rom_path.clone(),
        platform: Platform::Gba,
        adapter_id: "gba.generic".into(),
        source_language: None,
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();

    let loaded = load_project(&project_dir).unwrap();
    assert_eq!(created.id, loaded.id);
    assert_eq!(created.source_sha256, loaded.source_sha256);
    assert_eq!(loaded.target_language, "pt-BR");

    // Original intocado (regra: nunca sobrescrever a origem).
    assert_eq!(fs::read(&rom_path).unwrap(), rom);
    // Estrutura de subdiretorios criada.
    for sub in ["cache", "extracted", "working", "exports"] {
        assert!(project_dir.join(sub).is_dir(), "faltou {sub}/");
    }

    // Recusa sobrescrever projeto existente.
    let again = create_project(CreateProjectArgs {
        source_path: rom_path,
        platform: Platform::Gba,
        adapter_id: "gba.generic".into(),
        source_language: None,
        target_language: "pt-BR".into(),
        project_dir,
    });
    assert!(again.is_err());
}

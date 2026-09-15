//! Reinsercao conservadora NES/SNES: round-trips in-place e o recalculo da
//! soma canonica do SNES (incluindo espelhamento de tamanho nao-potencia-de-2).

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::nes::NesAdapter;
use romtranslate_core::adapters::snes::{snes_sum, SnesAdapter};
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
        let dir = std::env::temp_dir().join(format!("rts-con-{label}-{nanos}"));
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
fn snes_sum_handles_power_of_two_and_mirroring() {
    // Potencia de 2: soma direta.
    assert_eq!(snes_sum(&[1u8, 2, 3, 4]), 10);

    // 48 KiB = 32 KiB + 16 KiB espelhada 2x: sum(low) + 2*sum(rest).
    let mut body = vec![1u8; 48 * 1024];
    for b in body[32 * 1024..].iter_mut() {
        *b = 3;
    }
    let expected = (32 * 1024u32 + 2 * (3 * 16 * 1024u32)) as u16;
    assert_eq!(snes_sum(&body), expected);

    assert_eq!(snes_sum(&[]), 0);
}

#[test]
fn nes_roundtrip_in_place() {
    let rom = synth::make_nes_rom();
    let entries = NesAdapter.extract_structured(&rom).unwrap();
    let texts: Vec<_> = entries.iter().map(|e| e.source_text.as_str()).collect();
    assert!(texts.contains(&"PLAY BALL!"), "{texts:?}");
    assert!(texts.contains(&"GAME OVER"));
    assert_eq!(
        entries.len(),
        2,
        "filler nao-printable: so as strings plantadas"
    );

    let mut translated = entries.clone();
    for e in translated.iter_mut() {
        e.translated_text = match e.source_text.as_str() {
            "PLAY BALL!" => Some("JOGAR!".to_string()),
            "GAME OVER" => Some("FIM DE JOGO".to_string()), // 11 > 9 bytes: estoura
            _ => None,
        };
    }
    // Uma cabe, outra estoura -> all-or-nothing recusa tudo.
    assert!(NesAdapter.apply_text(&rom, &translated).is_err());

    for e in translated.iter_mut() {
        if e.source_text == "GAME OVER" {
            e.translated_text = Some("FIM JOGO".to_string()); // 8 <= 9
        }
    }
    let applied = NesAdapter.apply_text(&rom, &translated).unwrap();
    assert_eq!(applied.report.applied, 2);
    let verification = NesAdapter.verify(&applied.bytes).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);

    let reread = NesAdapter.extract_structured(&applied.bytes).unwrap();
    let texts: Vec<_> = reread.iter().map(|e| e.source_text.as_str()).collect();
    assert!(texts.contains(&"JOGAR!"));
    assert!(texts.contains(&"FIM JOGO"));
}

#[test]
fn snes_roundtrip_recalculates_real_checksum() {
    for rom in [
        synth::make_snes_lorom("SYNTHETIC QUEST"),
        synth::make_snes_headered("SYNTHETIC QUEST"),
    ] {
        // Fixture nova ja sai com checksum REAL: verify passa direto.
        let verification = SnesAdapter.verify(&rom).unwrap();
        assert!(verification.ok, "{:?}", verification.problems);

        let mut entries = SnesAdapter.extract_structured(&rom).unwrap();
        for e in entries.iter_mut() {
            e.translated_text = match e.source_text.as_str() {
                "MAGIC SWORD" => Some("ESPADA MAG.".to_string()), // 11 == 11
                "NEW QUEST" => Some("MISSAO".to_string()),        // 6 <= 9
                _ => None,
            };
        }
        let applied = SnesAdapter.apply_text(&rom, &entries).unwrap();
        assert_eq!(applied.report.applied, 2);

        // Checksum interno recalculado bate com a soma real da imagem nova.
        let verification = SnesAdapter.verify(&applied.bytes).unwrap();
        assert!(verification.ok, "{:?}", verification.problems);
        assert_ne!(rom, applied.bytes);

        let reread = SnesAdapter.extract_structured(&applied.bytes).unwrap();
        assert!(reread.iter().any(|e| e.source_text == "ESPADA MAG."));
        assert!(reread.iter().any(|e| e.source_text == "MISSAO"));

        // Probe continua forte na imagem traduzida.
        use romtranslate_core::adapter::GameInput;
        let probe = SnesAdapter.probe(&GameInput::from_bytes("t.sfc", &applied.bytes));
        assert!(probe.confidence >= 0.9, "{}", probe.confidence);
    }
}

#[test]
fn nes_full_project_flow_reinsert_and_patch() {
    let tmp = TempDir::new("nesflow");
    let rom = synth::make_nes_rom();
    let rom_path = tmp.0.join("game.nes");
    fs::write(&rom_path, &rom).unwrap();
    let project_dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: rom_path.clone(),
        platform: Platform::Nes,
        adapter_id: "nes.ines".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();

    let mut entries = NesAdapter.extract_structured(&rom).unwrap();
    for e in entries.iter_mut() {
        if e.source_text == "PLAY BALL!" {
            e.translated_text = Some("JOGAR!".to_string());
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
    assert!(working.windows(6).any(|w| w == b"JOGAR!"));
}

//! Integracao do Sprint 5: round-trip do adapter RTSF (fixed + relocatable +
//! pointer table + checksum) e o fluxo completo de reinsercao com working copy.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::rtsf::{compute_checksum, RtsfAdapter};
use romtranslate_core::db::ProjectDb;
use romtranslate_core::detect::inspect;
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
        let dir = std::env::temp_dir().join(format!("rts-re-{label}-{nanos}"));
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
fn probe_detects_rtsf_fixture_with_full_support() {
    let tmp = TempDir::new("probe");
    let path = tmp.0.join("game.rtsf");
    fs::write(&path, synth::make_rtsf_fixture()).unwrap();

    let report = inspect(&path).unwrap();
    let best = report.best.expect("rtsf detectado");
    assert_eq!(best.platform, Platform::Synthetic);
    assert_eq!(best.adapter_id, "synthetic.rtsf");
    assert!(best.confidence >= 0.95);
    assert!(best.evidence.iter().any(|e| e.contains("checksum")));
}

#[test]
fn extract_finds_fixed_and_relocatable_with_limits() {
    let data = synth::make_rtsf_fixture();
    let entries = RtsfAdapter.extract_structured(&data).unwrap();
    assert_eq!(entries.len(), 8);

    let fixed: Vec<_> = entries
        .iter()
        .filter(|e| e.id.starts_with("fixed-"))
        .collect();
    assert_eq!(fixed.len(), 4);
    assert_eq!(fixed[0].source_text, "SAVE GAME");
    assert_eq!(fixed[0].max_bytes, Some(23), "slot 24 - terminador");

    let reloc: Vec<_> = entries
        .iter()
        .filter(|e| e.id.starts_with("reloc-"))
        .collect();
    assert_eq!(reloc.len(), 4);
    assert_eq!(reloc[0].source_text, "WELCOME TO THE VILLAGE!");
    assert_eq!(reloc[0].max_bytes, None, "relocavel nao tem limite fixo");
}

/// DoD do Sprint 5: aplicar e RE-LER prova que a alteracao funcionou.
#[test]
fn roundtrip_apply_verify_and_reextract() {
    let data = synth::make_rtsf_fixture();
    let mut entries = RtsfAdapter.extract_structured(&data).unwrap();

    // Traduz 2 fixas e 2 relocaveis — uma relocavel BEM maior que a original
    // (forca o blob a realocar e a tabela de ponteiros a mudar).
    for e in entries.iter_mut() {
        e.translated_text = match e.id.as_str() {
            "fixed-0" => Some("SALVAR JOGO".to_string()),
            "fixed-3" => Some("SAIR".to_string()),
            "reloc-0" => Some("BEM-VINDO AO VILAREJO DOS TESTES SINTETICOS!".to_string()),
            "reloc-1" => Some("HP {0} RESTAURADO".to_string()),
            _ => None,
        };
        if e.translated_text.is_some() {
            e.status = TranslationStatus::Machine;
        }
    }

    let applied = RtsfAdapter.apply_text(&data, &entries).unwrap();
    assert_eq!(applied.report.applied, 4);
    assert_eq!(applied.report.kept_original, 4);

    // Verificacao estrutural + checksum recalculado.
    let verification = RtsfAdapter.verify(&applied.bytes).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);
    assert_ne!(compute_checksum(&data), compute_checksum(&applied.bytes));

    // Round-trip: re-extrair devolve exatamente o que foi aplicado.
    let reread = RtsfAdapter.extract_structured(&applied.bytes).unwrap();
    let text = |id: &str| {
        reread
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .source_text
            .clone()
    };
    assert_eq!(text("fixed-0"), "SALVAR JOGO");
    assert_eq!(
        text("fixed-1"),
        "LOAD GAME",
        "nao traduzida mantem original"
    );
    assert_eq!(text("fixed-3"), "SAIR");
    assert_eq!(
        text("reloc-0"),
        "BEM-VINDO AO VILAREJO DOS TESTES SINTETICOS!"
    );
    assert_eq!(text("reloc-1"), "HP {0} RESTAURADO");
    assert_eq!(text("reloc-2"), "YOU FOUND A POTION");
}

#[test]
fn apply_rejects_overflow_and_non_ascii_without_partial_writes() {
    let data = synth::make_rtsf_fixture();
    let mut entries = RtsfAdapter.extract_structured(&data).unwrap();

    // Fixa maior que o slot -> Err com orientacao.
    entries[0].translated_text = Some("ESTE TEXTO E GRANDE DEMAIS PARA O SLOT".to_string());
    let err = RtsfAdapter.apply_text(&data, &entries).unwrap_err();
    assert!(err.to_string().contains("encurte"), "{err}");

    // Nao-ASCII em fixa -> Err.
    entries[0].translated_text = Some("SALVAÇÃO".to_string());
    assert!(RtsfAdapter.apply_text(&data, &entries).is_err());

    // Relocaveis estourando a capacidade do blob -> Err com numeros.
    let mut entries2 = RtsfAdapter.extract_structured(&data).unwrap();
    for e in entries2.iter_mut().filter(|e| e.id.starts_with("reloc-")) {
        e.translated_text = Some("X".repeat(120));
    }
    let err = RtsfAdapter.apply_text(&data, &entries2).unwrap_err();
    assert!(err.to_string().contains("capacidade"), "{err}");
}

#[test]
fn verify_catches_corruption() {
    let data = synth::make_rtsf_fixture();
    assert!(RtsfAdapter.verify(&data).unwrap().ok);

    let mut corrupted = data.clone();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0xFF;
    let report = RtsfAdapter.verify(&corrupted).unwrap();
    assert!(!report.ok);
    assert!(report.problems.iter().any(|p| p.contains("checksum")));
}

#[test]
fn malformed_headers_error_cleanly_never_panic() {
    let good = synth::make_rtsf_fixture();
    // Truncados em todos os tamanhos.
    for len in 0..good.len().min(0x60) {
        let _ = RtsfAdapter.extract_structured(&good[..len]);
    }
    // Header mentindo offsets/contagens gigantes.
    let mut lying = good.clone();
    lying[0x08..0x0C].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(RtsfAdapter.extract_structured(&lying).is_err());
    let mut lying2 = good.clone();
    lying2[0x10..0x14].copy_from_slice(&99_999u32.to_le_bytes());
    assert!(RtsfAdapter.extract_structured(&lying2).is_err());
}

#[test]
fn reinsert_flow_blocks_errors_and_never_touches_original() {
    let tmp = TempDir::new("flow");
    let rom_path = tmp.0.join("game.rtsf");
    let original_bytes = synth::make_rtsf_fixture();
    fs::write(&rom_path, &original_bytes).unwrap();

    let project_dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: rom_path.clone(),
        platform: Platform::Synthetic,
        adapter_id: "synthetic.rtsf".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();

    // Extrai estruturado e salva no DB com uma traducao BOA e uma QUEBRADA.
    let mut db = ProjectDb::open(&project_dir).unwrap();
    let entries = RtsfAdapter.extract_structured(&original_bytes).unwrap();
    db.upsert_entries(&entries).unwrap();
    db.update_translation("fixed-0", "SALVAR", TranslationStatus::Machine)
        .unwrap();
    db.update_translation("reloc-1", "HP RESTAURADO", TranslationStatus::Machine)
        .unwrap(); // removeu {0}: erro de validacao
    drop(db);

    // Bloqueio: a validacao interna acha o placeholder quebrado.
    let err = reinsert_project(&project_dir, false).unwrap_err();
    assert!(err.to_string().contains("erro de validacao"), "{err}");
    assert!(!project_dir.join("working/game.rtsf").exists());

    // Modo avancado: prossegue, aplica e verifica.
    let outcome = reinsert_project(&project_dir, true).unwrap();
    assert!(outcome.verification.ok);
    assert_eq!(outcome.apply.applied, 2);
    assert_eq!(outcome.forced_errors, 1);
    assert!(outcome.working_path.exists());

    // Original INTACTO byte a byte; working diferente.
    assert_eq!(fs::read(&rom_path).unwrap(), original_bytes);
    assert_ne!(fs::read(&outcome.working_path).unwrap(), original_bytes);

    // Corrigindo a traducao, o fluxo normal passa sem modo avancado.
    let mut db = ProjectDb::open(&project_dir).unwrap();
    db.update_translation("reloc-1", "HP {0} RESTAURADO", TranslationStatus::Machine)
        .unwrap();
    drop(db);
    let outcome = reinsert_project(&project_dir, false).unwrap();
    assert!(outcome.verification.ok);
    assert_eq!(outcome.forced_errors, 0);
    let working = fs::read(&outcome.working_path).unwrap();
    let reread = RtsfAdapter.extract_structured(&working).unwrap();
    assert!(reread.iter().any(|e| e.source_text == "HP {0} RESTAURADO"));

    // Origem alterada depois do projeto criado -> reinsercao aborta.
    fs::write(&rom_path, b"outra coisa").unwrap();
    let err = reinsert_project(&project_dir, false).unwrap_err();
    assert!(err.to_string().contains("SHA-256"), "{err}");
}

#[test]
fn probe_only_adapters_refuse_structured_calls() {
    use romtranslate_core::adapters::wii::WiiAdapter;
    let data = synth::make_wii_disc_header();
    assert!(WiiAdapter.extract_structured(&data).is_err());
    assert!(WiiAdapter.apply_text(&data, &[]).is_err());
    assert!(WiiAdapter.verify(&data).is_err());
}

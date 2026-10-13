//! Politica `Skip` da reinsercao: a traducao reprovada fica de fora e a frase
//! continua como no original, em vez de um erro travar o jogo inteiro.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::{reinsert_project, reinsert_project_with, ErrorPolicy};
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, TextEntry, TranslationStatus};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-skip-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn at(entries: &[TextEntry], text: &str) -> usize {
    entries
        .iter()
        .find(|e| e.source_text == text)
        .unwrap_or_else(|| panic!("\"{text}\" nao foi extraida"))
        .offset
        .unwrap() as usize
}

/// Projeto GBA com tres frases sem ponteiro em tabela traduzidas: uma boa,
/// uma com acento (nao codifica em ASCII) e uma maior que o espaco.
fn project(tmp: &TempDir, scanner_like: bool) -> (PathBuf, Vec<u8>, Vec<TextEntry>) {
    let rom = synth::make_gba_rom_with_pointers();
    let source = tmp.0.join("game.gba");
    fs::write(&source, &rom).unwrap();
    let dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: source,
        platform: Platform::Gba,
        adapter_id: GbaAdapter.id().to_string(),
        source_language: None,
        target_language: "pt-BR".to_string(),
        project_dir: dir.clone(),
    })
    .unwrap();

    let mut entries = GbaAdapter.extract_structured(&rom).unwrap();
    for e in &mut entries {
        let translation = match e.source_text.as_str() {
            "POTION" => "POCAO",
            "WELCOME TO THE VILLAGE!" => "BEM-VINDO À VILA!",
            "GAME OVER" => "FIM DE JOGO",
            _ => continue,
        };
        e.translated_text = Some(translation.to_string());
        e.status = TranslationStatus::Machine;
        if scanner_like {
            // Entry do scanner generico nao traz limite: o estouro so e aviso.
            e.max_bytes = None;
        }
    }
    ProjectDb::open(&dir)
        .unwrap()
        .upsert_entries(&entries)
        .unwrap();
    (dir, rom, entries)
}

#[test]
fn broken_translations_stay_original_and_the_rest_goes_in() {
    let tmp = TempDir::new("mixed");
    let (dir, rom, entries) = project(&tmp, false);

    // Sem a politica, um erro trava tudo — e isso continua valendo.
    assert!(reinsert_project(&dir, false).is_err());

    let outcome = reinsert_project_with(&dir, ErrorPolicy::Skip).unwrap();
    assert_eq!(outcome.skipped_errors, 2);
    assert_eq!(outcome.apply.applied, 1);
    assert_eq!(outcome.forced_errors, 0);
    assert!(outcome.verification.ok);

    let written = fs::read(&outcome.working_path).unwrap();
    let potion = at(&entries, "POTION");
    assert_eq!(&written[potion..potion + 6], b"POCAO\0");
    // As duas reprovadas: byte a byte como no original.
    for text in ["GAME OVER", "WELCOME TO THE VILLAGE!"] {
        let offset = at(&entries, text);
        assert_eq!(
            &written[offset..offset + text.len()],
            &rom[offset..offset + text.len()],
            "{text}"
        );
    }
    // E nada mais mudou no arquivo alem da frase que entrou.
    let changed = (0..rom.len()).filter(|&i| rom[i] != written[i]).count();
    assert!(changed <= 6, "{changed} bytes alterados");

    // As reprovadas continuam marcadas pra correcao.
    let after = ProjectDb::open(&dir).unwrap().load_entries().unwrap();
    let errors = after
        .iter()
        .filter(|e| e.status == TranslationStatus::Error)
        .count();
    assert_eq!(errors, 2);
}

#[test]
fn overflow_without_a_limit_is_skipped_instead_of_failing_everything() {
    let tmp = TempDir::new("scanner");
    let (dir, rom, entries) = project(&tmp, true);

    let outcome = reinsert_project_with(&dir, ErrorPolicy::Skip).unwrap();
    // A com acento e erro de validacao; a que estoura so era aviso, e mesmo
    // assim fica de fora — gravar recusaria o projeto inteiro.
    assert_eq!(outcome.skipped_errors, 2);
    assert_eq!(outcome.apply.applied, 1);
    let written = fs::read(&outcome.working_path).unwrap();
    let over = at(&entries, "GAME OVER");
    assert_eq!(&written[over..over + 9], &rom[over..over + 9]);
}

#[test]
fn a_clean_project_skips_nothing() {
    let tmp = TempDir::new("clean");
    let (dir, _, entries) = project(&tmp, false);
    let mut db = ProjectDb::open(&dir).unwrap();
    for e in &entries {
        if e.translated_text.is_some() && e.source_text != "POTION" {
            db.update_translation(&e.id, &e.source_text, TranslationStatus::Machine)
                .unwrap();
        }
    }
    let outcome = reinsert_project_with(&dir, ErrorPolicy::Skip).unwrap();
    assert_eq!(outcome.skipped_errors, 0);
    assert_eq!(outcome.apply.applied, 3);
}

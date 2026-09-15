//! Integracao do Sprint 4: validacao sobre o banco do projeto e fluxo de
//! edicao manual / revisao.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::db::ProjectDb;
use romtranslate_core::types::{TextEncoding, TextEntry, TranslationStatus};
use romtranslate_core::validate::{
    apply_manual_translation, set_reviewed, validate_project_db, IssueKind, Severity,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-val-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn entry(id: &str, source: &str, translation: Option<&str>) -> TextEntry {
    TextEntry {
        id: id.to_string(),
        resource_path: None,
        offset: None,
        original_bytes: source.as_bytes().to_vec(),
        source_text: source.to_string(),
        translated_text: translation.map(String::from),
        context: None,
        max_bytes: None,
        encoding: TextEncoding::Utf8,
        status: if translation.is_some() {
            TranslationStatus::Machine
        } else {
            TranslationStatus::Untranslated
        },
        metadata: serde_json::Value::Null,
    }
}

#[test]
fn validate_project_flags_errors_and_recovers() {
    let tmp = TempDir::new("flags");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    db.upsert_entries(&[
        entry("bad", "HP {0}: 120", Some("PV: 120")), // placeholder removido
        entry("good", "SAVE GAME", Some("SALVAR JOGO")),
        entry("pending", "EXIT", None),
    ])
    .unwrap();

    let report = validate_project_db(&mut db).unwrap();
    assert_eq!(report.checked, 2, "so entries traduzidas");
    assert_eq!(report.errors, 1);
    assert!(report
        .issues
        .iter()
        .any(|i| i.entry_id == "bad" && i.kind == IssueKind::PlaceholderMismatch));

    let entries = db.load_entries().unwrap();
    let by_id = |id: &str| entries.iter().find(|e| e.id == id).unwrap();
    assert_eq!(by_id("bad").status, TranslationStatus::Error);
    assert_eq!(by_id("good").status, TranslationStatus::Machine);
    assert_eq!(by_id("pending").status, TranslationStatus::Untranslated);

    // Corrigiu -> revalidar limpa o Error de volta para Machine.
    db.update_translation("bad", "PV {0}: 120", TranslationStatus::Machine)
        .unwrap();
    let report2 = validate_project_db(&mut db).unwrap();
    assert_eq!(report2.errors, 0);
    assert_eq!(
        db.get_entry("bad").unwrap().unwrap().status,
        TranslationStatus::Machine
    );
}

#[test]
fn manual_edit_feeds_tm_and_flags_errors() {
    let tmp = TempDir::new("edit");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    db.upsert_entries(&[entry("a", "Use the {0} now!", Some("Use o {0} agora!"))])
        .unwrap();

    // Edicao valida: TM alimentada, sem issues.
    let issues =
        apply_manual_translation(&mut db, None, "a", "Use {0} ja!", "en-US", "pt-BR").unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(
        db.tm_lookup("Use the {0} now!", "en-US", "pt-BR")
            .unwrap()
            .as_deref(),
        Some("Use {0} ja!")
    );

    // Edicao que quebra placeholder: issue Error e status Error.
    let issues = apply_manual_translation(&mut db, None, "a", "Use ja!", "en-US", "pt-BR").unwrap();
    assert!(issues.iter().any(|i| i.severity == Severity::Error));
    assert_eq!(
        db.get_entry("a").unwrap().unwrap().status,
        TranslationStatus::Error
    );

    // Entry inexistente: erro claro.
    assert!(apply_manual_translation(&mut db, None, "nope", "x", "", "pt-BR").is_err());
}

#[test]
fn review_requires_clean_validation() {
    let tmp = TempDir::new("review");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    db.upsert_entries(&[
        entry("ok", "SAVE", Some("SALVAR")),
        entry("broken", "HP {0}", Some("PV")),
        entry("pending", "EXIT", None),
    ])
    .unwrap();

    assert_eq!(
        set_reviewed(&mut db, "ok", true).unwrap(),
        TranslationStatus::Reviewed
    );
    assert!(set_reviewed(&mut db, "broken", true).is_err());
    assert!(set_reviewed(&mut db, "pending", true).is_err());

    // Desmarcar volta para Machine.
    assert_eq!(
        set_reviewed(&mut db, "ok", false).unwrap(),
        TranslationStatus::Machine
    );

    // Reviewed limpo permanece Reviewed apos validate_project_db.
    set_reviewed(&mut db, "ok", true).unwrap();
    validate_project_db(&mut db).unwrap();
    assert_eq!(
        db.get_entry("ok").unwrap().unwrap().status,
        TranslationStatus::Reviewed
    );
}

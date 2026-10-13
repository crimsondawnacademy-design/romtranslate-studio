//! Entry solta (fora das regioes de texto) sai da traducao: vira nome interno
//! e perde a traducao automatica que ja tinha; a revisada por gente fica.

use std::fs;

use romtranslate_core::db::ProjectDb;
use romtranslate_core::types::{TextEncoding, TextEntry, TranslationStatus};

fn entry(id: &str, text: &str) -> TextEntry {
    TextEntry {
        id: id.to_string(),
        resource_path: None,
        offset: None,
        original_bytes: text.as_bytes().to_vec(),
        source_text: text.to_string(),
        translated_text: None,
        context: None,
        max_bytes: None,
        encoding: TextEncoding::Ascii,
        status: TranslationStatus::Untranslated,
        metadata: serde_json::Value::Null,
    }
}

#[test]
fn stray_entries_leave_the_queue_and_lose_machine_translation_but_not_a_reviewed_one() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rts-stray-{nanos}.rtsproj"));
    fs::create_dir_all(&dir).unwrap();
    let mut db = ProjectDb::open(&dir).unwrap();
    db.upsert_entries(&[
        entry("pending", "Grida"),
        entry("machine", "Gridb"),
        entry("reviewed", "Gridc"),
        entry("text", "A real line of the game."),
    ])
    .unwrap();
    db.update_translation("machine", "Gradeb", TranslationStatus::Machine)
        .unwrap();
    db.update_translation("reviewed", "Gradec", TranslationStatus::Reviewed)
        .unwrap();

    let stray: Vec<String> = ["pending", "machine", "reviewed"]
        .map(String::from)
        .to_vec();
    assert_eq!(db.mark_stray(&stray).unwrap(), 2);
    // Marcar de novo nao muda nada.
    assert_eq!(db.mark_stray(&stray).unwrap(), 0);

    let now = |id: &str| db.get_entry(id).unwrap().unwrap();
    assert_eq!(now("pending").status, TranslationStatus::Internal);
    assert_eq!(now("machine").status, TranslationStatus::Internal);
    assert_eq!(now("machine").translated_text, None);
    assert_eq!(now("reviewed").status, TranslationStatus::Reviewed);
    assert_eq!(now("reviewed").translated_text.as_deref(), Some("Gradec"));
    assert_eq!(now("text").status, TranslationStatus::Untranslated);
    let _ = fs::remove_dir_all(&dir);
}

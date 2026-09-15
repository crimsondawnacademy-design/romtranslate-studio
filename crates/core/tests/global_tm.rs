//! TM global cross-projeto: cascata (projeto primeiro, global como fallback)
//! e alimentacao dupla por traducao de maquina e edicao manual.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use async_trait::async_trait;
use romtranslate_core::db::{GlobalTm, ProjectDb};
use romtranslate_core::error::Result;
use romtranslate_core::pipeline::{run_translation, TranslateOptions};
use romtranslate_core::provider::{
    TranslatedItem, TranslationProvider, TranslationRequest, TranslationResponse,
};
use romtranslate_core::types::{TextEncoding, TextEntry, TranslationStatus};
use romtranslate_core::validate::apply_manual_translation;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-gtm-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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

struct MockProvider {
    calls: AtomicU32,
}

#[async_trait]
impl TranslationProvider for MockProvider {
    fn id(&self) -> &'static str {
        "mock"
    }

    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(TranslationResponse {
            translations: request
                .items
                .iter()
                .map(|i| TranslatedItem {
                    id: i.id.clone(),
                    translation: format!("[pt] {}", i.text),
                })
                .collect(),
        })
    }

    async fn health_check(&self) -> Result<()> {
        Ok(())
    }
}

fn opts() -> TranslateOptions {
    TranslateOptions {
        retry_delay_ms: 1,
        source_language: Some("en-US".into()),
        ..TranslateOptions::new("pt-BR")
    }
}

#[tokio::test]
async fn translation_in_one_project_feeds_global_and_hits_in_another() {
    let tmp = TempDir::new("cascade");
    let mut global = GlobalTm::open(&tmp.0.join("config/global_tm.sqlite")).unwrap();
    let cancel = AtomicBool::new(false);

    // Projeto A traduz via provider e alimenta a TM global.
    fs::create_dir_all(tmp.0.join("a.rtsproj")).unwrap();
    let mut db_a = ProjectDb::open(&tmp.0.join("a.rtsproj")).unwrap();
    db_a.upsert_entries(&[entry("a1", "MAGIC SWORD")]).unwrap();
    let provider = MockProvider {
        calls: AtomicU32::new(0),
    };
    let summary = run_translation(
        &mut db_a,
        Some(&mut global),
        &provider,
        "m",
        &opts(),
        &cancel,
        &|_| {},
    )
    .await
    .unwrap();
    assert_eq!(summary.translated, 1);
    assert_eq!(
        global
            .tm_lookup("MAGIC SWORD", "en-US", "pt-BR")
            .unwrap()
            .as_deref(),
        Some("[pt] MAGIC SWORD")
    );

    // Projeto B, DB novo, MESMA string: resolve pela global, sem provider.
    fs::create_dir_all(tmp.0.join("b.rtsproj")).unwrap();
    let mut db_b = ProjectDb::open(&tmp.0.join("b.rtsproj")).unwrap();
    db_b.upsert_entries(&[entry("b1", "MAGIC SWORD"), entry("b2", "NEW TEXT")])
        .unwrap();
    let provider_b = MockProvider {
        calls: AtomicU32::new(0),
    };
    let summary = run_translation(
        &mut db_b,
        Some(&mut global),
        &provider_b,
        "m",
        &opts(),
        &cancel,
        &|_| {},
    )
    .await
    .unwrap();
    assert_eq!(summary.tm_hits, 1, "hit veio da TM GLOBAL");
    assert_eq!(summary.translated, 1, "so a string inedita foi ao provider");
    assert_eq!(provider_b.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn project_tm_wins_over_global() {
    let tmp = TempDir::new("priority");
    let mut global = GlobalTm::open(&tmp.0.join("global_tm.sqlite")).unwrap();
    global
        .tm_store("SAVE GAME", "en-US", "pt-BR", "TRADUCAO GLOBAL")
        .unwrap();

    fs::create_dir_all(tmp.0.join("p.rtsproj")).unwrap();
    let mut db = ProjectDb::open(&tmp.0.join("p.rtsproj")).unwrap();
    db.tm_store("SAVE GAME", "en-US", "pt-BR", "TRADUCAO DO PROJETO")
        .unwrap();
    db.upsert_entries(&[entry("x", "SAVE GAME")]).unwrap();

    let provider = MockProvider {
        calls: AtomicU32::new(0),
    };
    let cancel = AtomicBool::new(false);
    run_translation(
        &mut db,
        Some(&mut global),
        &provider,
        "m",
        &opts(),
        &cancel,
        &|_| {},
    )
    .await
    .unwrap();

    let e = db.get_entry("x").unwrap().unwrap();
    assert_eq!(
        e.translated_text.as_deref(),
        Some("TRADUCAO DO PROJETO"),
        "TM do projeto tem prioridade sobre a global"
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn manual_edit_feeds_both_memories() {
    let tmp = TempDir::new("manual");
    let mut global = GlobalTm::open(&tmp.0.join("global_tm.sqlite")).unwrap();
    fs::create_dir_all(tmp.0.join("p.rtsproj")).unwrap();
    let mut db = ProjectDb::open(&tmp.0.join("p.rtsproj")).unwrap();
    db.upsert_entries(&[entry("m1", "PRESS START")]).unwrap();

    apply_manual_translation(
        &mut db,
        Some(&mut global),
        "m1",
        "APERTE START",
        "en-US",
        "pt-BR",
    )
    .unwrap();

    assert_eq!(
        db.tm_lookup("PRESS START", "en-US", "pt-BR")
            .unwrap()
            .as_deref(),
        Some("APERTE START")
    );
    assert_eq!(
        global
            .tm_lookup("PRESS START", "en-US", "pt-BR")
            .unwrap()
            .as_deref(),
        Some("APERTE START"),
        "edicao manual alimenta a TM global tambem"
    );
}

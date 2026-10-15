//! Integracao do Sprint 3: DB do projeto, TM, glossario e pipeline com mock.
//! O teste com Ollama real fica atras de #[ignore] (roda manual, sem rede no CI).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::error::{CoreError, Result};
use romtranslate_core::pipeline::{run_translation, TranslateOptions};
use romtranslate_core::provider::{
    GlossaryTerm, TranslatedItem, TranslationProvider, TranslationRequest, TranslationResponse,
};
use romtranslate_core::types::{TextEncoding, TextEntry, TranslationStatus};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-tr-{label}-{nanos}"));
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

/// Mock deterministico: traduz prefixando "[pt] "; pode falhar as N primeiras
/// chamadas; captura os requests para inspecao.
struct MockProvider {
    fail_first: AtomicU32,
    calls: AtomicU32,
    requests: Mutex<Vec<TranslationRequest>>,
}

impl MockProvider {
    fn new(fail_first: u32) -> Self {
        MockProvider {
            fail_first: AtomicU32::new(fail_first),
            calls: AtomicU32::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl TranslationProvider for MockProvider {
    fn id(&self) -> &'static str {
        "mock"
    }

    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(request.clone());
        if self.fail_first.load(Ordering::SeqCst) > 0 {
            self.fail_first.fetch_sub(1, Ordering::SeqCst);
            return Err(CoreError::Provider("falha simulada".to_string()));
        }
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

    /// Cada chamada "cobra" 100 tokens de entrada e 40 de saida.
    fn usage(&self) -> romtranslate_core::provider::Usage {
        let calls = self.calls.load(Ordering::SeqCst) as u64;
        romtranslate_core::provider::Usage {
            input_tokens: calls * 100,
            output_tokens: calls * 40,
        }
    }
}

fn opts() -> TranslateOptions {
    TranslateOptions {
        retry_delay_ms: 1, // testes nao esperam backoff real
        batch_size: 4,
        // Estes testes contam as chamadas uma a uma: batches em paralelo e a
        // repergunta do que nao coube tem teste proprio (`fast_translate.rs`).
        parallel: 1,
        shorten_rounds: 0,
        ..TranslateOptions::new("pt-BR")
    }
}

fn no_progress() -> impl Fn(romtranslate_core::pipeline::Progress) + Send + Sync {
    |_| {}
}

#[test]
fn db_roundtrip_preserves_translations_on_rescan() {
    let tmp = TempDir::new("db");
    let mut db = ProjectDb::open(&tmp.0).unwrap();

    db.upsert_entries(&[entry("a", "HELLO"), entry("b", "WORLD")])
        .unwrap();
    db.update_translation("a", "OLÁ", TranslationStatus::Machine)
        .unwrap();

    // Re-scan (upsert de novo sem traducao) NAO pode apagar a traducao.
    db.upsert_entries(&[entry("a", "HELLO"), entry("c", "NEW")])
        .unwrap();

    let entries = db.load_entries().unwrap();
    assert_eq!(entries.len(), 3);
    let a = entries.iter().find(|e| e.id == "a").unwrap();
    assert_eq!(a.translated_text.as_deref(), Some("OLÁ"));
    assert_eq!(a.status, TranslationStatus::Machine);
    assert_eq!(a.encoding, TextEncoding::Ascii);
}

#[test]
fn glossary_crud_and_tm_normalization() {
    let tmp = TempDir::new("gloss");
    let mut db = ProjectDb::open(&tmp.0).unwrap();

    db.glossary_upsert(&GlossaryTerm {
        term: "Potion".into(),
        translation: Some("Poção".into()),
        no_translate: false,
        case_sensitive: false,
        note: Some("item".into()),
    })
    .unwrap();
    db.glossary_upsert(&GlossaryTerm {
        term: "HP".into(),
        translation: None,
        no_translate: true,
        case_sensitive: true,
        note: None,
    })
    .unwrap();
    assert_eq!(db.glossary_list().unwrap().len(), 2);
    db.glossary_delete("Potion").unwrap();
    assert_eq!(db.glossary_list().unwrap().len(), 1);
    assert!(db
        .glossary_upsert(&GlossaryTerm {
            term: "  ".into(),
            translation: None,
            no_translate: false,
            case_sensitive: false,
            note: None,
        })
        .is_err());

    // TM: whitespace colapsado, case preservado.
    db.tm_store("Hello   World", "", "pt-BR", "Olá Mundo")
        .unwrap();
    assert_eq!(
        db.tm_lookup(" Hello World ", "", "pt-BR")
            .unwrap()
            .as_deref(),
        Some("Olá Mundo")
    );
    assert!(db.tm_lookup("hello world", "", "pt-BR").unwrap().is_none());
    assert!(db.tm_lookup("Hello World", "", "en-US").unwrap().is_none());
}

#[tokio::test]
async fn full_flow_translates_saves_and_hits_tm_on_rerun() {
    let tmp = TempDir::new("flow");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    let entries: Vec<TextEntry> = (0..10)
        .map(|i| entry(&format!("e{i}"), &format!("LINE NUMBER {i}")))
        .collect();
    db.upsert_entries(&entries).unwrap();

    let provider = MockProvider::new(0);
    let cancel = AtomicBool::new(false);
    let summary = run_translation(
        &mut db,
        None,
        &provider,
        "mock-model",
        &opts(),
        &cancel,
        &no_progress(),
    )
    .await
    .unwrap();
    assert_eq!(summary.translated, 10);
    assert_eq!(summary.tm_hits, 0);
    assert_eq!(summary.failed, 0);
    assert!(!summary.cancelled);
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        3,
        "10 itens em batches de 4"
    );

    let all = db.load_entries().unwrap();
    assert!(all.iter().all(|e| e.translated_text.is_some()));
    assert_eq!(
        all[0].translated_text.as_deref(),
        Some("[pt] LINE NUMBER 0")
    );

    // Nova entry com MESMO texto de uma ja traduzida -> resolve via TM, sem provider.
    db.upsert_entries(&[entry("dup", "LINE NUMBER 3")]).unwrap();
    let provider2 = MockProvider::new(0);
    let summary2 = run_translation(
        &mut db,
        None,
        &provider2,
        "mock-model",
        &opts(),
        &cancel,
        &no_progress(),
    )
    .await
    .unwrap();
    assert_eq!(summary2.tm_hits, 1);
    assert_eq!(summary2.translated, 0);
    assert_eq!(provider2.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_translation_with_errors_goes_back_to_the_provider_only_when_asked() {
    let tmp = TempDir::new("redo");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    let mut broken = entry("bad", "LINE ONE");
    broken.translated_text = Some("RESPOSTA RUIM".to_string());
    broken.status = TranslationStatus::Error;
    db.upsert_entries(&[broken, entry("new", "LINE TWO")])
        .unwrap();
    // A memoria tem a resposta ruim: quem volta com erro nao passa por ela.
    db.tm_store("LINE ONE", "", "pt-BR", "RESPOSTA RUIM")
        .unwrap();
    let cancel = AtomicBool::new(false);

    let provider = MockProvider::new(0);
    run_translation(
        &mut db,
        None,
        &provider,
        "mock-model",
        &opts(),
        &cancel,
        &no_progress(),
    )
    .await
    .unwrap();
    let sent: Vec<String> = provider.requests.lock().unwrap()[0]
        .items
        .iter()
        .map(|i| i.id.clone())
        .collect();
    assert_eq!(sent, ["new"], "sem pedir, a frase com erro fica como esta");

    let provider = MockProvider::new(0);
    let redo = TranslateOptions {
        redo_errors: true,
        ..opts()
    };
    let summary = run_translation(
        &mut db,
        None,
        &provider,
        "mock-model",
        &redo,
        &cancel,
        &no_progress(),
    )
    .await
    .unwrap();
    assert_eq!((summary.translated, summary.tm_hits), (1, 0));
    let bad = db.get_entry("bad").unwrap().unwrap();
    assert_eq!(bad.translated_text.as_deref(), Some("[pt] LINE ONE"));
}

#[tokio::test]
async fn retry_recovers_and_exhaustion_counts_failed() {
    let tmp = TempDir::new("retry");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    db.upsert_entries(&[entry("a", "RETRY ME")]).unwrap();

    // Falha 2x, opts permite 3 tentativas -> sucesso.
    let provider = MockProvider::new(2);
    let cancel = AtomicBool::new(false);
    let summary = run_translation(
        &mut db,
        None,
        &provider,
        "m",
        &opts(),
        &cancel,
        &no_progress(),
    )
    .await
    .unwrap();
    assert_eq!(summary.translated, 1);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 3);

    // Esgota tentativas -> failed, entry continua sem traducao.
    let tmp2 = TempDir::new("retry2");
    let mut db2 = ProjectDb::open(&tmp2.0).unwrap();
    db2.upsert_entries(&[entry("b", "ALWAYS FAILS")]).unwrap();
    let provider2 = MockProvider::new(99);
    let summary2 = run_translation(
        &mut db2,
        None,
        &provider2,
        "m",
        &opts(),
        &cancel,
        &no_progress(),
    )
    .await
    .unwrap();
    assert_eq!(summary2.failed, 1);
    assert_eq!(summary2.translated, 0);
    assert!(db2.load_entries().unwrap()[0].translated_text.is_none());
}

/// A estimativa monta os mesmos lotes da rodada: o tamanho que ela promete e
/// o que o provider recebe, e o que ele cobra volta somado no resumo.
#[tokio::test]
async fn estimate_measures_the_round_before_it_runs_and_usage_is_summed_after() {
    use romtranslate_core::pipeline::estimate_translation;
    use romtranslate_core::provider::{build_system_prompt, build_user_prompt};
    use romtranslate_core::types::TranslationStatus;

    let tmp = TempDir::new("estimate");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    let mut lines: Vec<TextEntry> = (0..9)
        .map(|i| entry(&format!("e{i}"), &format!("LINE NUMBER {i}")))
        .collect();
    // Nome interno e frase ja traduzida nao entram na conta.
    let mut internal = entry("room", "LV4_31_01");
    internal.status = TranslationStatus::Internal;
    let mut done = entry("done", "ALREADY DONE");
    done.translated_text = Some("JA FEITA".to_string());
    lines.extend([internal, done]);
    db.upsert_entries(&lines).unwrap();

    let (estimate, sample) = estimate_translation(&db, &opts()).unwrap();
    assert_eq!((estimate.entries, estimate.batches), (9, 3)); // lotes de 4
    assert!(sample.is_some());
    assert!(estimate.output_chars > 9 * "LINE NUMBER 0".len());

    let provider = MockProvider::new(0);
    let summary = run_translation(
        &mut db,
        None,
        &provider,
        "m",
        &opts(),
        &AtomicBool::new(false),
        &no_progress(),
    )
    .await
    .unwrap();
    let sent: usize = provider
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|r| build_system_prompt(r).len() + build_user_prompt(r).len())
        .sum();
    assert_eq!(estimate.input_chars, sent);
    assert_eq!(summary.translated, 9);
    assert_eq!((summary.input_tokens, summary.output_tokens), (300, 120));

    // Com tudo traduzido, nao sobra nada a estimar.
    let (after, sample) = estimate_translation(&db, &opts()).unwrap();
    assert_eq!((after.entries, after.batches, after.input_chars), (0, 0, 0));
    assert!(sample.is_none());
}

/// Conta sem limite, sem login ou sem rede: o provider nao volta mais. A
/// rodada para em vez de falhar a fila inteira lote por lote, e diz por que.
#[tokio::test]
async fn a_provider_that_stopped_answering_ends_the_run_with_its_reason() {
    let lines: Vec<TextEntry> = (0..20)
        .map(|i| entry(&format!("e{i:02}"), &format!("LINE NUMBER {i}")))
        .collect();
    let cancel = AtomicBool::new(false);

    let tmp = TempDir::new("stopped");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    db.upsert_entries(&lines).unwrap();
    let dead = MockProvider::new(999);
    let summary = run_translation(&mut db, None, &dead, "m", &opts(), &cancel, &no_progress())
        .await
        .unwrap();
    let reason = summary.stopped.expect("a rodada devia ter parado");
    assert!(reason.contains("falha simulada"), "{reason}");
    // 5 lotes de 4: so os 2 primeiros foram tentados, 3 vezes cada.
    assert_eq!(dead.calls.load(Ordering::SeqCst), 6);
    assert_eq!(summary.failed, 8);
    assert_eq!(summary.translated, 0);
    assert!(db
        .load_entries()
        .unwrap()
        .iter()
        .all(|e| e.translated_text.is_none()));

    // Um lote que falha sozinho nao para nada: o seguinte responde e a
    // contagem zera.
    let tmp = TempDir::new("stumble");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    db.upsert_entries(&lines).unwrap();
    let stumbling = MockProvider::new(3);
    let summary = run_translation(
        &mut db,
        None,
        &stumbling,
        "m",
        &opts(),
        &cancel,
        &no_progress(),
    )
    .await
    .unwrap();
    assert_eq!(summary.stopped, None);
    assert_eq!(summary.failed, 4);
    assert_eq!(summary.translated, 16);
}

#[tokio::test]
async fn glossary_reaches_prompt_only_when_relevant() {
    let tmp = TempDir::new("glossflow");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    db.upsert_entries(&[entry("a", "Drink the potion now")])
        .unwrap();
    db.glossary_upsert(&GlossaryTerm {
        term: "Potion".into(),
        translation: Some("Poção".into()),
        no_translate: false,
        case_sensitive: false,
        note: None,
    })
    .unwrap();
    db.glossary_upsert(&GlossaryTerm {
        term: "Zelda".into(),
        translation: None,
        no_translate: true,
        case_sensitive: true,
        note: None,
    })
    .unwrap();

    let provider = MockProvider::new(0);
    let cancel = AtomicBool::new(false);
    run_translation(
        &mut db,
        None,
        &provider,
        "m",
        &opts(),
        &cancel,
        &no_progress(),
    )
    .await
    .unwrap();

    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let glossary = &requests[0].glossary;
    assert_eq!(glossary.len(), 1, "so o termo presente no texto");
    assert_eq!(glossary[0].term, "Potion");
}

#[tokio::test]
async fn cancel_stops_between_batches() {
    let tmp = TempDir::new("cancel");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    let entries: Vec<TextEntry> = (0..12)
        .map(|i| entry(&format!("c{i}"), &format!("CANCEL TEST {i}")))
        .collect();
    db.upsert_entries(&entries).unwrap();

    let provider = MockProvider::new(0);
    let cancel = AtomicBool::new(false);
    // Cancela assim que o primeiro progresso de traducao chegar.
    let summary = run_translation(&mut db, None, &provider, "m", &opts(), &cancel, &|p| {
        if p.phase == "translate" {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .await
    .unwrap();
    assert!(summary.cancelled);
    assert!(summary.translated < 12, "nao traduziu tudo: {summary:?}");
    assert!(provider.calls.load(Ordering::SeqCst) < 3);
}

/// Prova de fogo do DoD com Ollama REAL. Roda manual:
/// `cargo test -p romtranslate-core --test translate -- --ignored --nocapture`
/// Requer `ollama serve` ativo e o modelo em OLLAMA_TEST_MODEL (default llama3.2:3b).
#[tokio::test]
#[ignore]
async fn real_ollama_translates_synthetic_strings() {
    use romtranslate_core::providers::ollama::OllamaProvider;

    let model = std::env::var("OLLAMA_TEST_MODEL").unwrap_or_else(|_| "llama3.2:3b".to_string());
    let provider = OllamaProvider::new("http://localhost:11434", &model, 300).unwrap();
    provider.health_check().await.expect("ollama fora do ar?");

    let tmp = TempDir::new("real");
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    db.upsert_entries(&[
        entry("w", "WELCOME TO THE VILLAGE!"),
        entry("p", "Drink the POTION to restore HP {0}."),
        entry("s", "SAVE GAME"),
    ])
    .unwrap();
    db.glossary_upsert(&GlossaryTerm {
        term: "HP".into(),
        translation: None,
        no_translate: true,
        case_sensitive: true,
        note: None,
    })
    .unwrap();

    let cancel = AtomicBool::new(false);
    let mut o = TranslateOptions::new("pt-BR");
    o.source_language = Some("en-US".into());
    let summary = run_translation(&mut db, None, &provider, &model, &o, &cancel, &|p| {
        eprintln!("progress: {}/{} ({})", p.done, p.total, p.phase);
    })
    .await
    .unwrap();
    eprintln!("summary: {summary:?}");
    assert_eq!(summary.failed, 0, "{summary:?}");

    for e in db.load_entries().unwrap() {
        let tr = e.translated_text.expect(&e.id);
        eprintln!("{} => {tr}", e.source_text);
        assert!(!tr.trim().is_empty());
    }
}

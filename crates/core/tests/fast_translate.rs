//! A rodada de traducao manda varios batches ao provider ao mesmo tempo e
//! repede o que voltou maior que o espaco da frase, com o tamanho que passou.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use romtranslate_core::adapters::extract_project;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::error::{CoreError, Result};
use romtranslate_core::pipeline::{run_translation, TranslateOptions, TranslateSummary};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::provider::{
    TranslatedItem, TranslationProvider, TranslationRequest, TranslationResponse,
};
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, TextEntry, TranslationStatus};
use romtranslate_core::validate::{validate_project_db, IssueKind};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-fast-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// Projeto de GBA sintetico com a extracao estruturada no banco.
    fn project(&self) -> PathBuf {
        let source = self.0.join("game.gba");
        fs::write(&source, synth::make_gba_rom_with_names()).unwrap();
        let dir = self.0.join("game.rtsproj");
        create_project(CreateProjectArgs {
            source_path: source,
            platform: Platform::Gba,
            adapter_id: "gba.generic".to_string(),
            source_language: Some("en-US".to_string()),
            target_language: "pt-BR".to_string(),
            project_dir: dir.clone(),
        })
        .unwrap();
        extract_project(&dir).unwrap();
        dir
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Answer {
    /// Devolve o proprio original: sempre cabe.
    Fits,
    /// Primeiro uma traducao comprida; curta quando o pedido e repergunta.
    LongThenShort,
    /// Sempre comprida, mesmo na repergunta.
    AlwaysLong,
    /// Comprida, e vazia quando o pedido e repergunta.
    LongThenEmpty,
    /// O provider esta fora do ar.
    Down,
}

const TAIL: &str = " e mais um pouco de texto";
const SHORT: &str = "ok";

/// Provider de mentira que demora um pouco pra responder, pra dar tempo de as
/// chamadas se sobreporem, e conta quantas estavam abertas ao mesmo tempo.
struct FakeProvider {
    answer: Answer,
    open: AtomicUsize,
    peak: AtomicUsize,
    requests: Mutex<Vec<TranslationRequest>>,
}

impl FakeProvider {
    fn new(answer: Answer) -> Self {
        FakeProvider {
            answer,
            open: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<TranslationRequest> {
        self.requests.lock().unwrap().clone()
    }
}

/// O pedido e uma repergunta de tamanho?
fn is_retry(request: &TranslationRequest) -> bool {
    request.items.iter().any(|item| {
        item.context
            .as_deref()
            .is_some_and(|c| c.contains("does not fit"))
    })
}

#[async_trait]
impl TranslationProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }

    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse> {
        self.requests.lock().unwrap().push(request.clone());
        let open = self.open.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(open, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.open.fetch_sub(1, Ordering::SeqCst);

        if self.answer == Answer::Down {
            return Err(CoreError::Provider("limite de uso da conta".to_string()));
        }
        let retry = is_retry(request);
        Ok(TranslationResponse {
            translations: request
                .items
                .iter()
                .map(|item| TranslatedItem {
                    id: item.id.clone(),
                    translation: match self.answer {
                        Answer::LongThenShort if retry => SHORT.to_string(),
                        Answer::LongThenEmpty if retry => String::new(),
                        Answer::LongThenShort | Answer::AlwaysLong | Answer::LongThenEmpty => {
                            format!("{}{TAIL}", item.text)
                        }
                        _ => item.text.clone(),
                    },
                })
                .collect(),
        })
    }

    async fn health_check(&self) -> Result<()> {
        Ok(())
    }
}

fn options(batch_size: usize, parallel: usize) -> TranslateOptions {
    let mut opts = TranslateOptions::new("pt-BR");
    opts.source_language = Some("en-US".to_string());
    opts.batch_size = batch_size;
    opts.parallel = parallel;
    opts.max_attempts = 1;
    opts.retry_delay_ms = 0;
    opts
}

async fn translate(
    db: &mut ProjectDb,
    provider: &FakeProvider,
    opts: &TranslateOptions,
) -> (TranslateSummary, Vec<&'static str>) {
    let phases = Mutex::new(Vec::new());
    let summary = run_translation(
        db,
        None,
        provider,
        "fake",
        opts,
        &AtomicBool::new(false),
        &|p| phases.lock().unwrap().push(p.phase),
    )
    .await
    .unwrap();
    (summary, phases.into_inner().unwrap())
}

/// O que a rodada manda ao provider: sem traducao e fora dos nomes internos.
fn pending(db: &ProjectDb) -> Vec<TextEntry> {
    db.load_entries()
        .unwrap()
        .into_iter()
        .filter(|e| e.translated_text.is_none() && e.status != TranslationStatus::Internal)
        .collect()
}

#[tokio::test]
async fn batches_go_to_the_provider_together_up_to_the_limit() {
    for (parallel, expected_peak) in [(3, 3), (1, 1)] {
        let tmp = TempDir::new("parallel");
        let mut db = ProjectDb::open(&tmp.project()).unwrap();
        let waiting = pending(&db).len();
        assert!(waiting >= 6, "a fixture precisa de mais de uma leva");
        let provider = FakeProvider::new(Answer::Fits);

        let (summary, _) = translate(&mut db, &provider, &options(1, parallel)).await;

        assert_eq!(summary.translated, waiting);
        assert_eq!(summary.failed, 0);
        assert_eq!(provider.requests().len(), waiting, "um batch por entry");
        assert_eq!(provider.peak.load(Ordering::SeqCst), expected_peak);
        assert!(pending(&db).is_empty());
    }
}

#[tokio::test]
async fn translation_that_does_not_fit_is_asked_again_with_the_limit() {
    let tmp = TempDir::new("shorten");
    let mut db = ProjectDb::open(&tmp.project()).unwrap();
    let waiting = pending(&db);
    // Frase com ponteiro em tabela pode crescer: nao tem o que repedir.
    let fixed_space: Vec<&TextEntry> = waiting
        .iter()
        .filter(|e| e.pointer_offsets().is_empty())
        .collect();
    assert!(!fixed_space.is_empty());
    let provider = FakeProvider::new(Answer::LongThenShort);

    let (summary, phases) = translate(&mut db, &provider, &options(4, 2)).await;

    assert_eq!(summary.translated, waiting.len());
    assert_eq!(summary.shortened, fixed_space.len());
    assert_eq!(summary.too_long, 0);
    assert!(phases.contains(&"shorten"));

    // A repergunta leva a traducao que nao coube e o teto em caracteres.
    let retries: Vec<TranslationRequest> =
        provider.requests().into_iter().filter(is_retry).collect();
    assert!(!retries.is_empty());
    for item in retries.iter().flat_map(|r| &r.items) {
        let entry = fixed_space.iter().find(|e| e.id == item.id).unwrap();
        assert_eq!(item.max_chars, Some(entry.original_bytes.len()));
        let context = item.context.as_deref().unwrap();
        assert!(context.contains(&format!("{}{TAIL}", entry.source_text)));
    }

    // O banco ficou com a curta, e a validacao nao acusa mais tamanho.
    let entries = db.load_entries().unwrap();
    for entry in &fixed_space {
        let now = entries.iter().find(|e| e.id == entry.id).unwrap();
        assert_eq!(now.translated_text.as_deref(), Some(SHORT));
    }
    let report = validate_project_db(&mut db).unwrap();
    let ids: Vec<&str> = fixed_space.iter().map(|e| e.id.as_str()).collect();
    assert!(!report
        .issues
        .iter()
        .any(|i| i.kind == IssueKind::ByteOverflow && ids.contains(&i.entry_id.as_str())));
}

#[tokio::test]
async fn provider_that_never_shortens_is_asked_once_and_the_count_is_reported() {
    let tmp = TempDir::new("stubborn");
    let mut db = ProjectDb::open(&tmp.project()).unwrap();
    let fixed_space = pending(&db)
        .iter()
        .filter(|e| e.pointer_offsets().is_empty())
        .count();
    let provider = FakeProvider::new(Answer::AlwaysLong);

    // Tudo num batch so: a rodada e uma chamada, a repergunta outra.
    let (summary, _) = translate(&mut db, &provider, &options(50, 4)).await;

    assert_eq!(summary.shortened, 0);
    assert_eq!(summary.too_long, fixed_space);
    // Uma rodada de repergunta sem nenhum acerto: a segunda nao acontece.
    let retries = provider.requests().iter().filter(|r| is_retry(r)).count();
    assert_eq!(retries, 1);
    // A primeira resposta continua no banco, pra pessoa encurtar a mao.
    assert!(db.load_entries().unwrap().iter().any(|e| e
        .translated_text
        .as_deref()
        .is_some_and(|t| t.ends_with(TAIL))));
}

#[tokio::test]
async fn empty_answer_to_the_second_question_does_not_replace_the_translation() {
    let tmp = TempDir::new("empty");
    let mut db = ProjectDb::open(&tmp.project()).unwrap();
    let provider = FakeProvider::new(Answer::LongThenEmpty);

    let (summary, _) = translate(&mut db, &provider, &options(50, 4)).await;

    assert_eq!(summary.shortened, 0);
    assert!(summary.too_long > 0);
    assert!(db
        .load_entries()
        .unwrap()
        .iter()
        .all(|e| e.translated_text.as_deref() != Some("")));
}

#[tokio::test]
async fn no_shorten_rounds_means_no_second_question() {
    let tmp = TempDir::new("no-retry");
    let mut db = ProjectDb::open(&tmp.project()).unwrap();
    let provider = FakeProvider::new(Answer::LongThenShort);
    let mut opts = options(50, 4);
    opts.shorten_rounds = 0;

    let (summary, phases) = translate(&mut db, &provider, &opts).await;

    assert_eq!(summary.shortened, 0);
    assert!(summary.too_long > 0);
    assert!(!phases.contains(&"shorten"));
    assert!(!provider.requests().iter().any(is_retry));
}

#[tokio::test]
async fn provider_down_stops_the_round_after_the_first_wave() {
    let tmp = TempDir::new("down");
    let mut db = ProjectDb::open(&tmp.project()).unwrap();
    let waiting = pending(&db).len();
    let provider = FakeProvider::new(Answer::Down);

    let (summary, _) = translate(&mut db, &provider, &options(1, 4)).await;

    // O motivo e o erro do provider.
    assert!(summary
        .stopped
        .as_deref()
        .is_some_and(|why| why.contains("limite de uso da conta")));
    assert_eq!(summary.translated, 0);
    // So a primeira leva saiu: o resto da fila continua pendente.
    assert_eq!(provider.requests().len(), 4);
    assert_eq!(summary.failed, 4);
    assert_eq!(pending(&db).len(), waiting);
}

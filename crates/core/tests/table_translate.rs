//! A traducao automatica respeita a tabela de caracteres do projeto: o
//! prompt do batch lista o que a tabela escreve, a resposta com acento que a
//! tabela nao tem e gravada com a letra base (com aviso), e o que sobra fora
//! da tabela continua erro de validacao. Cada item leva o espaco da original
//! (texto de tabela e gravado no lugar), e traducao maior que ele e erro na
//! frase — inteira, sem corte.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use async_trait::async_trait;
use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::nes::NesAdapter;
use romtranslate_core::db::{GlobalTm, ProjectDb};
use romtranslate_core::error::Result;
use romtranslate_core::pipeline::{run_translation, TranslateOptions, TranslateSummary};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::provider::{
    build_system_prompt, build_user_prompt, TranslatedItem, TranslationProvider,
    TranslationRequest, TranslationResponse,
};
use romtranslate_core::reinsert::reinsert_project;
use romtranslate_core::relative::{build_tbl, save_tbl, search_project};
use romtranslate_core::scan::{scan_file, scan_project, ScanConfig, ScanEncoding};
use romtranslate_core::synth::{self, CUSTOM_LINES, CUSTOM_LOWER, CUSTOM_UPPER};
use romtranslate_core::types::{Platform, TextEncoding, TextEntry, TranslationStatus};
use romtranslate_core::validate::{validate_project_db, IssueKind, Severity, ValidationReport};

const ROM_NAME: &str = "game.nes";
/// O que a tabela da busca relativa escreve: espaco e letras sem acento.
const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-tbltr-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// Projeto de NES da fixture de tabela propria, ainda sem scan.
    fn project(&self) -> PathBuf {
        let source = self.0.join(ROM_NAME);
        fs::write(&source, synth::make_custom_table_rom()).unwrap();
        let dir = self.0.join("game.rtsproj");
        create_project(CreateProjectArgs {
            source_path: source,
            platform: Platform::Nes,
            adapter_id: NesAdapter.id().to_string(),
            source_language: Some("en-US".to_string()),
            target_language: "pt-BR".to_string(),
            project_dir: dir.clone(),
        })
        .unwrap();
        dir
    }

    /// Projeto com a tabela achada pela busca relativa e as frases do scan
    /// por ela ja no banco. Devolve (projeto, `.tbl` do projeto).
    fn scanned_project(&self) -> (PathBuf, PathBuf) {
        let dir = self.project();
        let found = &search_project(&dir, "Press").unwrap()[0];
        let tbl = save_tbl(&dir, found.upper_base, found.lower_base).unwrap();
        scan_into_db(&dir, &tbl);
        (dir, tbl)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn table_config(tbl: &Path) -> ScanConfig {
    ScanConfig {
        encoding: ScanEncoding::Table,
        tbl_path: Some(tbl.to_path_buf()),
        ..ScanConfig::default()
    }
}

fn scan_into_db(dir: &Path, tbl: &Path) {
    let outcome = scan_project(dir, &table_config(tbl)).unwrap();
    ProjectDb::open(dir)
        .unwrap()
        .upsert_entries(&outcome.entries)
        .unwrap();
}

/// Provider de mentira: responde o que esta em `answers` (por texto
/// original) e devolve o proprio original pro resto. Guarda os pedidos.
struct FakeProvider {
    answers: HashMap<&'static str, &'static str>,
    requests: Mutex<Vec<TranslationRequest>>,
}

impl FakeProvider {
    fn new(answers: &[(&'static str, &'static str)]) -> Self {
        FakeProvider {
            answers: answers.iter().copied().collect(),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<TranslationRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl TranslationProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }

    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(TranslationResponse {
            translations: request
                .items
                .iter()
                .map(|item| TranslatedItem {
                    id: item.id.clone(),
                    translation: self
                        .answers
                        .get(item.text.as_str())
                        .copied()
                        .unwrap_or(&item.text)
                        .to_string(),
                })
                .collect(),
        })
    }

    async fn health_check(&self) -> Result<()> {
        Ok(())
    }
}

async fn translate(
    dir: &Path,
    global: Option<&mut GlobalTm>,
    provider: &dyn TranslationProvider,
) -> TranslateSummary {
    let opts = TranslateOptions {
        source_language: Some("en-US".to_string()),
        batch_size: 4,
        ..TranslateOptions::new("pt-BR")
    };
    let mut db = ProjectDb::open(dir).unwrap();
    run_translation(
        &mut db,
        global,
        provider,
        "fake-model",
        &opts,
        &AtomicBool::new(false),
        &|_| {},
    )
    .await
    .unwrap()
}

fn validate(dir: &Path) -> ValidationReport {
    validate_project_db(&mut ProjectDb::open(dir).unwrap()).unwrap()
}

/// A traducao gravada da entry cujo original e `source`.
fn translation(dir: &Path, source: &str) -> String {
    ProjectDb::open(dir)
        .unwrap()
        .load_entries()
        .unwrap()
        .into_iter()
        .find(|e| e.source_text == source)
        .unwrap_or_else(|| panic!("sem entry \"{source}\""))
        .translated_text
        .unwrap_or_else(|| panic!("\"{source}\" sem traducao"))
}

/// Entry de texto que escreve tudo (UTF-8): sem lista de caracteres.
fn plain_entry(id: &str, text: &str) -> TextEntry {
    TextEntry {
        id: id.to_string(),
        resource_path: None,
        offset: None,
        original_bytes: text.as_bytes().to_vec(),
        source_text: text.to_string(),
        translated_text: None,
        context: None,
        max_bytes: None,
        encoding: TextEncoding::Utf8,
        status: TranslationStatus::Untranslated,
        metadata: serde_json::Value::Null,
    }
}

#[tokio::test]
async fn table_batch_lists_the_characters_and_plain_batch_is_untouched() {
    let tmp = TempDir::new("prompt");
    let (dir, _) = tmp.scanned_project();
    // Duas entries de outro encoding no mesmo projeto.
    ProjectDb::open(&dir)
        .unwrap()
        .upsert_entries(&[
            plain_entry("plain-1", "HELLO"),
            plain_entry("plain-2", "WORLD"),
        ])
        .unwrap();

    let provider = FakeProvider::new(&[]);
    let summary = translate(&dir, None, &provider).await;
    assert_eq!(summary.translated, CUSTOM_LINES.len() + 2);
    assert_eq!(summary.failed, 0);

    // Um batch, uma tabela: o texto sem restricao (UTF-8) vai num pedido so
    // dele, sem lista de caracteres; as 7 frases da tabela, em batches de 4.
    // O batch de texto ASCII esta em `tests/ascii_translate.rs`.
    let requests = provider.requests();
    let texts = |r: &TranslationRequest| -> Vec<String> {
        r.items.iter().map(|i| i.text.clone()).collect()
    };
    assert_eq!(requests.len(), 3);
    assert_eq!(texts(&requests[0]), ["HELLO", "WORLD"]);
    assert_eq!(requests[0].charset, None);
    let plain = build_system_prompt(&requests[0]);
    assert!(!plain.contains("Character set"), "{plain}");
    // Sem tabela, nenhum limite em caracteres: nem no item, nem no prompt.
    assert!(!plain.contains("Length limit"), "{plain}");
    assert!(requests[0].items.iter().all(|i| i.max_chars.is_none()));
    assert!(!build_user_prompt(&requests[0]).contains("maxChars"));

    assert_eq!(texts(&requests[1]), CUSTOM_LINES[..4]);
    assert_eq!(texts(&requests[2]), CUSTOM_LINES[4..]);
    for request in &requests[1..] {
        assert_eq!(request.charset, Some(format!(" {LETTERS}")));
        let prompt = build_system_prompt(request);
        assert!(prompt.contains(&format!("\n{LETTERS}\n")), "{prompt}");
        assert!(prompt.contains("plus the space"), "{prompt}");
        assert!(prompt.contains("ONLY those characters"), "{prompt}");
        assert!(prompt.contains("no accented letters"), "{prompt}");
        // Texto de tabela e gravado no lugar: cada item leva o espaco da
        // original. Nesta tabela um caractere e um byte.
        for item in &request.items {
            assert_eq!(item.max_chars, Some(item.text.len()), "{}", item.text);
            assert_eq!(item.max_bytes, None);
        }
        assert!(prompt.contains("Length limit (STRICT)"), "{prompt}");
    }
    // E o numero chega no pedido: "START GAME" tem 10 bytes.
    assert!(build_user_prompt(&requests[1]).contains("\"maxChars\":10"));
}

#[tokio::test]
async fn limit_counts_by_the_most_expensive_entry_of_the_table() {
    let tmp = TempDir::new("wide");
    let dir = tmp.project();
    // A mesma tabela com um caractere de 2 bytes: so com ele, cabe a metade.
    let rom = synth::make_custom_table_rom();
    let text = build_tbl(&rom, Some(CUSTOM_UPPER), Some(CUSTOM_LOWER)).unwrap() + "FF21=!\n";
    let outside = tmp.0.join("larga.tbl");
    fs::write(&outside, text).unwrap();
    scan_into_db(&dir, &outside);

    let provider = FakeProvider::new(&[]);
    translate(&dir, None, &provider).await;
    let items: Vec<(String, Option<usize>)> = provider
        .requests()
        .iter()
        .flat_map(|r| r.items.iter().map(|i| (i.text.clone(), i.max_chars)))
        .collect();
    assert_eq!(items.len(), CUSTOM_LINES.len());
    for (text, max_chars) in items {
        // "PRESS START" tem 11 bytes: 5, nunca 6.
        assert_eq!(max_chars, Some(text.len() / 2), "{text}");
    }
}

#[tokio::test]
async fn translation_larger_than_the_space_is_an_error_on_the_entry() {
    let tmp = TempDir::new("overflow");
    let (dir, tbl) = tmp.scanned_project();
    // "CONTINUE" passa a ter ponteiro em tabela (metadata do scan + ponteiro):
    // pode ser realocada, entao nao tem teto.
    let mut db = ProjectDb::open(&dir).unwrap();
    let mut pointed = db
        .load_entries()
        .unwrap()
        .into_iter()
        .find(|e| e.source_text == "CONTINUE")
        .unwrap();
    pointed.metadata["pointers"] = serde_json::json!([0x20]);
    db.upsert_entries(&[pointed]).unwrap();
    drop(db);

    let provider = FakeProvider::new(&[
        ("START GAME", "INICIAR O JOGO"),
        ("PRESS START", "APERTE AQUI"),
        ("CONTINUE", "CONTINUAR JOGO"),
    ]);
    translate(&dir, None, &provider).await;
    let limit_of = |source: &str| -> Option<usize> {
        provider
            .requests()
            .iter()
            .flat_map(|r| r.items.clone())
            .find(|i| i.text == source)
            .unwrap()
            .max_chars
    };
    assert_eq!(limit_of("START GAME"), Some(10));
    assert_eq!(limit_of("CONTINUE"), None);

    // A resposta maior que o espaco fica gravada inteira: nada e cortado.
    assert_eq!(translation(&dir, "START GAME"), "INICIAR O JOGO");
    let report = validate(&dir);
    let overflow: Vec<(Severity, &str)> = report
        .issues
        .iter()
        .filter(|i| i.kind == IssueKind::ByteOverflow)
        .map(|i| (i.severity, i.message.as_str()))
        .collect();
    assert_eq!(overflow.len(), 2, "{overflow:?}");
    // Gravada no lugar: erro, com os dois tamanhos.
    assert!(
        overflow
            .iter()
            .any(|(severity, m)| *severity == Severity::Error
                && m.contains("14 bytes")
                && m.contains("tem 10")
                && m.contains("encurte")),
        "{overflow:?}"
    );
    // Realocavel: aviso, como nos outros encodings.
    assert!(
        overflow
            .iter()
            .any(|(severity, m)| *severity == Severity::Warning && m.contains("original 8")),
        "{overflow:?}"
    );
    assert_eq!(report.errors, 1, "{:?}", report.issues);
    let status_of = |source: &str| {
        ProjectDb::open(&dir)
            .unwrap()
            .load_entries()
            .unwrap()
            .into_iter()
            .find(|e| e.source_text == source)
            .unwrap()
            .status
    };
    assert_eq!(status_of("START GAME"), TranslationStatus::Error);
    // Do tamanho exato do espaco nao e erro.
    assert_eq!(status_of("PRESS START"), TranslationStatus::Machine);

    let blocked = reinsert_project(&dir, false).unwrap_err().to_string();
    assert!(blocked.contains("1 entries com erro"), "{blocked}");
    assert!(blocked.contains("14 bytes"), "{blocked}");
    assert!(!dir.join("working").join(ROM_NAME).exists());

    // Encurtada a mao, a frase volta a valer e o texto entra no jogo.
    let mut db = ProjectDb::open(&dir).unwrap();
    for (source, shorter) in [("START GAME", "NOVO JOGO"), ("CONTINUE", "SEGUIR")] {
        let id = db
            .load_entries()
            .unwrap()
            .into_iter()
            .find(|e| e.source_text == source)
            .unwrap()
            .id;
        let issues = romtranslate_core::validate::apply_manual_translation(
            &mut db, None, &id, shorter, "en-US", "pt-BR",
        )
        .unwrap();
        assert!(issues.is_empty(), "{issues:?}");
    }
    drop(db);
    let outcome = reinsert_project(&dir, false).unwrap();
    assert!(outcome.verification.ok, "{:?}", outcome.verification);
    let read = scan_file(&outcome.working_path, &table_config(&tbl)).unwrap();
    assert_eq!(read.entries[0].source_text, "NOVO JOGO");
}

#[tokio::test]
async fn accented_answer_goes_into_the_game_without_the_accents() {
    let tmp = TempDir::new("roundtrip");
    let (dir, tbl) = tmp.scanned_project();

    // (resposta do provider, o que tem que ser gravado), na ordem do cartucho.
    let answers = [
        ("INÍCIO", "INICIO"),
        ("APERTE JÁ", "APERTE JA"),
        ("SEGUIR", "SEGUIR"),
        (
            "Aperte Start e começa a missão",
            "Aperte Start e comeca a missao",
        ),
        ("O herói foi à torre antiga", "O heroi foi a torre antiga"),
        ("a vila está quieta hoje", "a vila esta quieta hoje"),
        (
            "Recomeçar do último ponto salvo",
            "Recomecar do ultimo ponto salvo",
        ),
    ];
    let by_source: Vec<(&str, &str)> = CUSTOM_LINES
        .iter()
        .zip(answers)
        .map(|(source, (answer, _))| (*source, answer))
        .collect();
    let provider = FakeProvider::new(&by_source);
    let mut global = GlobalTm::open(&tmp.0.join("global_tm.sqlite")).unwrap();
    let summary = translate(&dir, Some(&mut global), &provider).await;
    assert_eq!(summary.translated, CUSTOM_LINES.len());

    for (source, (_, written)) in CUSTOM_LINES.iter().zip(answers) {
        assert_eq!(translation(&dir, source), written);
    }

    // Nenhum erro; a troca aparece como aviso em cada entry que mudou, com o
    // que o provider respondeu e o que foi gravado. "SEGUIR" nao mudou.
    let report = validate(&dir);
    assert_eq!(report.errors, 0, "{:?}", report.issues);
    let swaps: Vec<&str> = report
        .issues
        .iter()
        .filter(|i| i.kind == IssueKind::AccentDropped)
        .map(|i| {
            assert_eq!(i.severity, Severity::Warning);
            assert!(i.message.contains("tables/letras-80-A0.tbl"), "{i:?}");
            i.message.as_str()
        })
        .collect();
    assert_eq!(swaps.len(), 6, "{swaps:?}");
    assert!(
        swaps
            .iter()
            .any(|m| m.contains("\"INÍCIO\"") && m.contains("\"INICIO\"")),
        "{swaps:?}"
    );

    // A memoria do projeto guarda a resposta como veio; a global nao recebe
    // traducao moldada pela tabela deste jogo.
    let db = ProjectDb::open(&dir).unwrap();
    assert_eq!(
        db.tm_lookup("START GAME", "en-US", "pt-BR")
            .unwrap()
            .as_deref(),
        Some("INÍCIO")
    );
    assert_eq!(
        global.tm_lookup("START GAME", "en-US", "pt-BR").unwrap(),
        None
    );

    // E o texto sem acento volta pro jogo.
    let outcome = reinsert_project(&dir, false).unwrap();
    assert_eq!(outcome.apply.applied, CUSTOM_LINES.len());
    assert!(outcome.verification.ok, "{:?}", outcome.verification);
    let read: Vec<(u64, String)> = scan_file(&outcome.working_path, &table_config(&tbl))
        .unwrap()
        .entries
        .into_iter()
        .map(|e| (e.offset.unwrap(), e.source_text))
        .collect();
    let expected: Vec<(u64, String)> = answers
        .iter()
        .enumerate()
        .map(|(i, (_, written))| (0x100 + i as u64 * 0x40, written.to_string()))
        .collect();
    assert_eq!(read, expected);
}

#[tokio::test]
async fn what_the_table_cannot_write_stays_a_validation_error() {
    let tmp = TempDir::new("error");
    let dir = tmp.project();
    // Tabela sem o 'c' minusculo (nenhuma frase do cartucho usa).
    let rom = synth::make_custom_table_rom();
    let text: String = build_tbl(&rom, Some(CUSTOM_UPPER), Some(CUSTOM_LOWER))
        .unwrap()
        .lines()
        .filter(|line| !line.ends_with("=c"))
        .map(|line| format!("{line}\n"))
        .collect();
    let outside = tmp.0.join("semc.tbl");
    fs::write(&outside, text).unwrap();
    scan_into_db(&dir, &outside);

    let provider = FakeProvider::new(&[
        // 'Í' vira 'I'; o '!' nao tem troca.
        ("START GAME", "INÍCIO!"),
        // 'ã' vira 'a'; a base do 'ç' (o 'c') nao esta na tabela.
        ("the village is quiet tonight", "ação"),
        // Maiusculo, o 'C' existe: tudo trocado.
        ("CONTINUE", "AÇÃO"),
    ]);
    translate(&dir, None, &provider).await;
    assert_eq!(translation(&dir, "START GAME"), "INICIO!");
    assert_eq!(translation(&dir, "the village is quiet tonight"), "açao");
    assert_eq!(translation(&dir, "CONTINUE"), "ACAO");

    let report = validate(&dir);
    let errors: Vec<&str> = report
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| {
            assert_eq!(i.kind, IssueKind::Unencodable);
            i.message.as_str()
        })
        .collect();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors.iter().any(|m| m.contains("'!'")), "{errors:?}");
    assert!(errors.iter().any(|m| m.contains("'ç'")), "{errors:?}");
    // A troca que houve continua visivel, erro ou nao.
    let swapped = report
        .issues
        .iter()
        .filter(|i| i.kind == IssueKind::AccentDropped)
        .count();
    assert_eq!(swapped, 3);

    let blocked = reinsert_project(&dir, false).unwrap_err().to_string();
    assert!(blocked.contains("2 entries com erro"), "{blocked}");
    assert!(!dir.join("working").join(ROM_NAME).exists());
}

#[tokio::test]
async fn memory_hit_is_fitted_to_the_table_or_left_to_the_provider() {
    let tmp = TempDir::new("memory");
    let (dir, _) = tmp.scanned_project();
    // Memoria global vinda de um jogo de outro encoding.
    let mut global = GlobalTm::open(&tmp.0.join("global_tm.sqlite")).unwrap();
    for (source, translation) in [("START GAME", "INÍCIO"), ("CONTINUE", "CONTINUAR?")] {
        global
            .tm_store(source, "en-US", "pt-BR", translation)
            .unwrap();
    }

    let provider = FakeProvider::new(&[("CONTINUE", "SEGUIR")]);
    let summary = translate(&dir, Some(&mut global), &provider).await;

    // Com acento so: aproveitada, sem acento e com o aviso.
    assert_eq!(summary.tm_hits, 1);
    assert_eq!(translation(&dir, "START GAME"), "INICIO");
    assert!(validate(&dir)
        .issues
        .iter()
        .any(|i| i.kind == IssueKind::AccentDropped && i.message.contains("\"INÍCIO\"")));

    // Com '?', que a tabela nao escreve: quem responde e o provider.
    let asked: Vec<String> = provider
        .requests()
        .iter()
        .flat_map(|r| r.items.iter().map(|i| i.text.clone()))
        .collect();
    assert!(asked.contains(&"CONTINUE".to_string()), "{asked:?}");
    assert!(!asked.contains(&"START GAME".to_string()), "{asked:?}");
    assert_eq!(translation(&dir, "CONTINUE"), "SEGUIR");
    // E a memoria global segue como estava.
    assert_eq!(
        global
            .tm_lookup("CONTINUE", "en-US", "pt-BR")
            .unwrap()
            .as_deref(),
        Some("CONTINUAR?")
    );
}

#[tokio::test]
async fn warning_goes_away_when_the_translation_is_edited() {
    let tmp = TempDir::new("edit");
    let (dir, _) = tmp.scanned_project();
    let provider = FakeProvider::new(&[("START GAME", "INÍCIO")]);
    translate(&dir, None, &provider).await;
    let dropped = |dir: &Path| {
        validate(dir)
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::AccentDropped)
            .count()
    };
    assert_eq!(dropped(&dir), 1);

    // Um novo scan troca a metadata das entries; o aviso continua.
    scan_into_db(&dir, &dir.join("tables").join("letras-80-A0.tbl"));
    assert_eq!(dropped(&dir), 1);

    let mut db = ProjectDb::open(&dir).unwrap();
    let id = db
        .load_entries()
        .unwrap()
        .into_iter()
        .find(|e| e.source_text == "START GAME")
        .unwrap()
        .id;
    let issues = romtranslate_core::validate::apply_manual_translation(
        &mut db, None, &id, "COMECAR", "en-US", "pt-BR",
    )
    .unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(dropped(&dir), 0);
}

/// Repassa o pedido sem o `maxChars`: o controle "sem limite" da medicao.
struct NoLimit<'a>(&'a dyn TranslationProvider);

#[async_trait]
impl TranslationProvider for NoLimit<'_> {
    fn id(&self) -> &'static str {
        self.0.id()
    }

    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse> {
        let mut request = request.clone();
        for item in &mut request.items {
            item.max_chars = None;
        }
        self.0.translate_batch(&request).await
    }

    async fn health_check(&self) -> Result<()> {
        self.0.health_check().await
    }
}

/// Prova de fogo com modelo REAL: o que um modelo de verdade responde quando
/// recebe a lista de caracteres e o espaco de cada frase. Roda manual:
/// `cargo test -p romtranslate-core --test table_translate -- --ignored --nocapture`
/// Requer `ollama serve` ativo e o modelo em OLLAMA_TEST_MODEL (default llama3.2:3b).
/// Com `TABLE_TEST_PROVIDER=codex` quem responde e o Codex CLI logado (gasta
/// a cota da conta; modelo em CODEX_TEST_MODEL, vazio = o padrao do Codex).
/// Com `TABLE_TEST_NO_LIMIT=1` o pedido vai sem o espaco de cada frase, pra
/// comparar.
#[tokio::test]
#[ignore]
async fn real_model_is_told_the_table_characters() {
    use romtranslate_core::providers::codex_cli::CodexCliProvider;
    use romtranslate_core::providers::ollama::OllamaProvider;

    let codex = std::env::var("TABLE_TEST_PROVIDER").as_deref() == Ok("codex");
    let (provider, model): (Box<dyn TranslationProvider>, String) = if codex {
        let model = std::env::var("CODEX_TEST_MODEL").unwrap_or_default();
        let provider = CodexCliProvider::new("codex", &model, 180).unwrap();
        (Box::new(provider), format!("codex {model}"))
    } else {
        let model =
            std::env::var("OLLAMA_TEST_MODEL").unwrap_or_else(|_| "llama3.2:3b".to_string());
        let provider = OllamaProvider::new("http://localhost:11434", &model, 300).unwrap();
        (Box::new(provider), model)
    };
    let unlimited = NoLimit(provider.as_ref());
    let no_limit = std::env::var_os("TABLE_TEST_NO_LIMIT").is_some();
    let provider: &dyn TranslationProvider = if no_limit {
        &unlimited
    } else {
        provider.as_ref()
    };
    provider.health_check().await.expect("provider fora do ar?");

    let tmp = TempDir::new("real");
    let (dir, _) = tmp.scanned_project();
    let summary = translate(&dir, None, provider).await;
    eprintln!("summary: {summary:?}");
    assert_eq!(summary.failed, 0, "{summary:?}");

    // A memoria do projeto guarda a resposta crua; a entry, o que foi gravado.
    let db = ProjectDb::open(&dir).unwrap();
    for e in db.load_entries().unwrap() {
        let answer = db.tm_lookup(&e.source_text, "en-US", "pt-BR").unwrap();
        eprintln!(
            "{:?} => resposta {:?}, gravado {:?} ({} caracteres pra {} de espaco)",
            e.source_text,
            answer,
            e.translated_text,
            e.translated_text.as_deref().unwrap_or("").chars().count(),
            e.original_bytes.len()
        );
    }
    let report = validate(&dir);
    for issue in &report.issues {
        eprintln!("{:?} {:?}: {}", issue.severity, issue.kind, issue.message);
    }
    let too_long = report
        .issues
        .iter()
        .filter(|i| i.kind == IssueKind::ByteOverflow)
        .count();
    eprintln!(
        "{model}{}: {} erros e {} avisos em {} frases; {too_long} maiores que o espaco",
        if no_limit { " SEM limite" } else { "" },
        report.errors,
        report.warnings,
        report.checked
    );
}

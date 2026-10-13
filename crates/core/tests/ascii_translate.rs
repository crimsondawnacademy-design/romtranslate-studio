//! Jogo de texto ASCII traduzido pra idioma com acento: o batch ASCII leva a
//! lista de caracteres (sem letra acentuada), a resposta que mesmo assim vem
//! com acento e gravada com a letra base, com aviso, e chega ao arquivo; texto
//! UTF-8 e UTF-16 fica com o acento. E nome interno do jogo (identificador)
//! fica fora da traducao e do arquivo.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use async_trait::async_trait;
use romtranslate_core::adapters::extract_project;
use romtranslate_core::db::{GlobalTm, ProjectDb};
use romtranslate_core::error::Result;
use romtranslate_core::pipeline::{run_translation, TranslateOptions, TranslateSummary};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::provider::{
    build_system_prompt, TranslatedItem, TranslationProvider, TranslationRequest,
    TranslationResponse,
};
use romtranslate_core::reinsert::reinsert_project;
use romtranslate_core::scan::{scan_file, scan_project, ScanConfig, ScanEncoding};
use romtranslate_core::synth::{
    self, NAMES_ROM_INTERNAL, NAMES_ROM_LINES, NAMES_ROM_MENU, NAMES_ROM_NAMES, NAMES_ROM_WIDE,
};
use romtranslate_core::types::{Platform, TextEncoding, TextEntry, TranslationStatus};
use romtranslate_core::validate::{
    unmark_internal, validate_project_db, IssueKind, Severity, ValidationReport,
};

const ROM_NAME: &str = "game.gba";
/// O que uma entry ASCII escreve: o ASCII imprimivel.
const ASCII: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`\
                     abcdefghijklmnopqrstuvwxyz{|}~";

/// (resposta do provider, o que tem que ser gravado em texto ASCII) pras tres
/// primeiras falas do cartucho: acento, aspas e travessao tipograficos, e uma
/// resposta que ja veio em ASCII.
const ANSWERS: [(&str, &str); 3] = [
    ("Volte amanhã de manhã!", "Volte amanha de manha!"),
    ("“Não!” — disse ele…", "\"Nao!\" - disse ele..."),
    ("A ponte velha caiu de novo.", "A ponte velha caiu de novo."),
];

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-ascii-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// Projeto de GBA do `make_gba_rom_with_names`, com a extracao
    /// estruturada (ASCII) e a fala em UTF-16 ja no banco.
    fn project(&self) -> PathBuf {
        let source = self.0.join(ROM_NAME);
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
        let wide = scan_project(&dir, &wide_config()).unwrap();
        assert_eq!(wide.entries.len(), 1);
        ProjectDb::open(&dir)
            .unwrap()
            .upsert_entries(&wide.entries)
            .unwrap();
        dir
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// So a regiao da fala em UTF-16: fora dela o scanner UTF-16 leria o texto
/// ASCII como outra coisa.
fn wide_config() -> ScanConfig {
    ScanConfig {
        encoding: ScanEncoding::Utf16Le,
        region_start: Some(NAMES_ROM_WIDE.start),
        region_end: Some(NAMES_ROM_WIDE.end),
        ..ScanConfig::default()
    }
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

    /// As respostas de `ANSWERS` pras tres primeiras falas.
    fn accented() -> Self {
        let answers: Vec<(&str, &str)> = NAMES_ROM_LINES
            .iter()
            .zip(ANSWERS)
            .map(|(source, (answer, _))| (*source, answer))
            .collect();
        Self::new(&answers)
    }

    fn requests(&self) -> Vec<TranslationRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// Os textos pedidos, de todos os batches, na ordem.
    fn asked(&self) -> Vec<String> {
        self.requests()
            .iter()
            .flat_map(|r| r.items.iter().map(|i| i.text.clone()))
            .collect()
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

fn entries(dir: &Path) -> Vec<TextEntry> {
    ProjectDb::open(dir).unwrap().load_entries().unwrap()
}

/// A entry de encoding `encoding` cujo original e `source`.
fn entry(dir: &Path, source: &str, encoding: &TextEncoding) -> TextEntry {
    entries(dir)
        .into_iter()
        .find(|e| e.source_text == source && e.encoding == *encoding)
        .unwrap_or_else(|| panic!("sem entry {encoding:?} \"{source}\""))
}

fn ascii(dir: &Path, source: &str) -> TextEntry {
    entry(dir, source, &TextEncoding::Ascii)
}

/// As mensagens dos avisos de troca de acento do projeto.
fn swaps(report: &ValidationReport) -> Vec<&str> {
    report
        .issues
        .iter()
        .filter(|i| i.kind == IssueKind::AccentDropped)
        .map(|i| {
            assert_eq!(i.severity, Severity::Warning);
            i.message.as_str()
        })
        .collect()
}

#[tokio::test]
async fn accented_answer_to_ascii_text_goes_into_the_game_without_the_accents() {
    let tmp = TempDir::new("roundtrip");
    let dir = tmp.project();
    let provider = FakeProvider::accented();
    let summary = translate(&dir, None, &provider).await;
    assert_eq!(summary.failed, 0);

    for (source, (_, written)) in NAMES_ROM_LINES.iter().zip(ANSWERS) {
        assert_eq!(
            ascii(&dir, source).translated_text.as_deref(),
            Some(written)
        );
    }

    // Nenhum erro; a troca aparece como aviso nas duas falas que mudaram,
    // com o que o provider respondeu e o que foi gravado. A terceira ja veio
    // em ASCII e a fala em UTF-16 nao tem o que trocar.
    let report = validate(&dir);
    assert_eq!(report.errors, 0, "{:?}", report.issues);
    let swaps = swaps(&report);
    assert_eq!(swaps.len(), 2, "{swaps:?}");
    for (answer, written) in &ANSWERS[..2] {
        let quoted = |text: &str| format!("\"{text}\"");
        assert!(
            swaps
                .iter()
                .any(|m| m.contains(&quoted(answer)) && m.contains(&quoted(written))),
            "{answer}: {swaps:?}"
        );
    }
    assert!(swaps.iter().all(|m| m.contains("ASCII")), "{swaps:?}");

    // E o texto sem acento chega ao arquivo, no lugar das falas originais.
    let outcome = reinsert_project(&dir, false).unwrap();
    assert!(outcome.verification.ok, "{:?}", outcome.verification);
    let rom = fs::read(&outcome.working_path).unwrap();
    for (i, (_, written)) in ANSWERS.iter().enumerate() {
        let at = 0x400 + i * 0x40;
        let slot = NAMES_ROM_LINES[i].len();
        let mut expected = written.as_bytes().to_vec();
        expected.resize(slot, 0);
        assert_eq!(&rom[at..at + slot], &expected[..], "{written}");
    }
    let read: Vec<String> = scan_file(&outcome.working_path, &ScanConfig::default())
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.source_text)
        .collect();
    for (_, written) in ANSWERS {
        assert!(read.contains(&written.to_string()), "{written}: {read:?}");
    }
}

#[tokio::test]
async fn utf16_text_keeps_the_accents_of_the_same_answer() {
    let tmp = TempDir::new("wide");
    let dir = tmp.project();
    let provider = FakeProvider::accented();
    translate(&dir, None, &provider).await;

    // A mesma frase, a mesma resposta: em ASCII perde o acento, em UTF-16 nao.
    let (answer, written) = ANSWERS[0];
    assert_eq!(
        ascii(&dir, NAMES_ROM_LINES[0]).translated_text.as_deref(),
        Some(written)
    );
    let wide = entry(&dir, NAMES_ROM_LINES[0], &TextEncoding::Utf16Le);
    assert_eq!(wide.translated_text.as_deref(), Some(answer));
    // Nenhuma troca registrada nem avisada pra ela.
    let db = ProjectDb::open(&dir).unwrap();
    assert!(!db.accent_swaps().unwrap().contains_key(&wide.id));
    let report = validate(&dir);
    assert!(
        report.issues.iter().all(|i| i.entry_id != wide.id),
        "{:?}",
        report.issues
    );

    // E o acento chega ao arquivo.
    let outcome = reinsert_project(&dir, false).unwrap();
    let read = scan_file(&outcome.working_path, &wide_config()).unwrap();
    assert_eq!(read.entries[0].source_text, answer);
}

#[tokio::test]
async fn ascii_batch_is_told_there_are_no_accents_and_free_batch_is_not() {
    let tmp = TempDir::new("prompt");
    let dir = tmp.project();
    // Mais uma entry sem restricao, de outro encoding.
    ProjectDb::open(&dir)
        .unwrap()
        .upsert_entries(&[TextEntry {
            id: "utf8-1".to_string(),
            resource_path: None,
            offset: None,
            original_bytes: b"Good night".to_vec(),
            source_text: "Good night".to_string(),
            translated_text: None,
            context: None,
            max_bytes: None,
            encoding: TextEncoding::Utf8,
            status: TranslationStatus::Untranslated,
            metadata: serde_json::Value::Null,
        }])
        .unwrap();

    let provider = FakeProvider::new(&[]);
    translate(&dir, None, &provider).await;
    let requests = provider.requests();

    // Um batch, um conjunto: nenhum pedido mistura encodings, e o texto que
    // escreve tudo vai primeiro, num pedido so dele.
    let encoding_of: HashMap<String, TextEncoding> = entries(&dir)
        .into_iter()
        .map(|e| (e.id, e.encoding))
        .collect();
    let encodings = |r: &TranslationRequest| -> Vec<TextEncoding> {
        r.items.iter().map(|i| encoding_of[&i.id].clone()).collect()
    };
    assert_eq!(
        encodings(&requests[0]),
        [TextEncoding::Utf16Le, TextEncoding::Utf8]
    );
    assert_eq!(requests[0].charset, None);
    let free = build_system_prompt(&requests[0]);
    assert!(!free.contains("Character set"), "{free}");
    assert!(!free.contains("no accented letters"), "{free}");

    assert!(requests.len() > 2);
    for request in &requests[1..] {
        assert!(
            encodings(request).iter().all(|e| *e == TextEncoding::Ascii),
            "{:?}",
            encodings(request)
        );
        assert_eq!(request.charset.as_deref(), Some(ASCII));
        let prompt = build_system_prompt(request);
        assert!(prompt.contains("Character set (STRICT)"), "{prompt}");
        assert!(prompt.contains("plus the space"), "{prompt}");
        assert!(prompt.contains(&format!("\n{}\n", &ASCII[1..])), "{prompt}");
        assert!(prompt.contains("no accented letters"), "{prompt}");
        // Texto ASCII tem `maxBytes` do adapter; o teto em caracteres e so
        // de texto de tabela.
        assert!(request.items.iter().all(|i| i.max_chars.is_none()));
    }
}

#[tokio::test]
async fn what_ascii_cannot_write_stays_an_error_naming_the_character() {
    let tmp = TempDir::new("error");
    let dir = tmp.project();
    // O 'ê' tem troca; o 'º' nao tem.
    let provider = FakeProvider::new(&[(NAMES_ROM_LINES[3], "Você ficou em 1º lugar!")]);
    translate(&dir, None, &provider).await;
    assert_eq!(
        ascii(&dir, NAMES_ROM_LINES[3]).translated_text.as_deref(),
        Some("Voce ficou em 1º lugar!")
    );

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
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("'º'") && errors[0].contains("posicao 16"),
        "{errors:?}"
    );
    // A troca que houve continua visivel, erro ou nao.
    assert_eq!(swaps(&report).len(), 1);

    let blocked = reinsert_project(&dir, false).unwrap_err().to_string();
    assert!(blocked.contains("1 entries com erro"), "{blocked}");
    assert!(blocked.contains("'º'"), "{blocked}");
    assert!(!dir.join("working").join(ROM_NAME).exists());
}

#[tokio::test]
async fn memory_hit_is_fitted_to_ascii_or_left_to_the_provider() {
    let tmp = TempDir::new("memory");
    let dir = tmp.project();
    // Memoria global vinda de um jogo que escreve acento.
    let mut global = GlobalTm::open(&tmp.0.join("global_tm.sqlite")).unwrap();
    for (source, translation) in [("NEW GAME", "INÍCIO"), ("OPTIONS", "OPÇ. Nº1")] {
        global
            .tm_store(source, "en-US", "pt-BR", translation)
            .unwrap();
    }

    let provider = FakeProvider::new(&[("OPTIONS", "OPCOES")]);
    let summary = translate(&dir, Some(&mut global), &provider).await;

    // So com acento: aproveitada, sem acento e com o aviso.
    assert_eq!(summary.tm_hits, 1);
    assert_eq!(
        ascii(&dir, "NEW GAME").translated_text.as_deref(),
        Some("INICIO")
    );
    assert!(swaps(&validate(&dir))
        .iter()
        .any(|m| m.contains("\"INÍCIO\"") && m.contains("\"INICIO\"")));

    // Com 'º', que ASCII nao escreve: quem responde e o provider.
    let asked = provider.asked();
    assert!(asked.contains(&"OPTIONS".to_string()), "{asked:?}");
    assert!(!asked.contains(&"NEW GAME".to_string()), "{asked:?}");
    assert_eq!(
        ascii(&dir, "OPTIONS").translated_text.as_deref(),
        Some("OPCOES")
    );
}

#[tokio::test]
async fn internal_names_stay_out_of_the_translation_and_of_the_game() {
    let tmp = TempDir::new("names");
    let dir = tmp.project();

    // A extracao ja marca: os quatro nomes, e so eles.
    let internal: Vec<String> = entries(&dir)
        .into_iter()
        .filter(|e| e.status == TranslationStatus::Internal)
        .map(|e| e.source_text)
        .collect();
    assert_eq!(internal, NAMES_ROM_NAMES);
    assert!(entries(&dir)
        .iter()
        .filter(|e| NAMES_ROM_NAMES.contains(&e.source_text.as_str()))
        .all(|e| !e.pointers().is_empty()));

    let provider = FakeProvider::accented();
    let summary = translate(&dir, None, &provider).await;

    // O provider recebe tudo que nao e nome interno, uma vez cada, e nada
    // alem disso: os itens e as chamadas batem com a conta.
    let text: Vec<TextEntry> = entries(&dir)
        .into_iter()
        .filter(|e| e.status != TranslationStatus::Internal)
        .collect();
    let mut expected: Vec<&str> = text.iter().map(|e| e.source_text.as_str()).collect();
    let asked = provider.asked();
    let mut got: Vec<&str> = asked.iter().map(String::as_str).collect();
    expected.sort_unstable();
    got.sort_unstable();
    assert_eq!(got, expected);
    assert_eq!(summary.translated, text.len());
    for name in NAMES_ROM_NAMES {
        assert!(
            !asked.contains(&name.to_string()),
            "{name} foi pro provider"
        );
    }
    for menu in NAMES_ROM_MENU {
        assert!(asked.contains(&menu.to_string()), "{menu} ficou de fora");
    }
    // Um batch de UTF-16 e os de ASCII, de 4 em 4.
    let ascii_items = text
        .iter()
        .filter(|e| e.encoding == TextEncoding::Ascii)
        .count();
    assert_eq!(provider.requests().len(), 1 + ascii_items.div_ceil(4));

    // Continuam na lista, sem traducao e marcados.
    for name in NAMES_ROM_NAMES {
        let e = ascii(&dir, name);
        assert_eq!(e.status, TranslationStatus::Internal, "{name}");
        assert_eq!(e.translated_text, None, "{name}");
    }

    // E a reinsercao nao toca neles nem na tabela de ponteiros deles.
    let original = synth::make_gba_rom_with_names();
    let outcome = reinsert_project(&dir, false).unwrap();
    assert!(outcome.verification.ok, "{:?}", outcome.verification);
    assert_eq!(outcome.apply.kept_original, NAMES_ROM_NAMES.len());
    let rom = fs::read(&outcome.working_path).unwrap();
    assert_eq!(rom[NAMES_ROM_INTERNAL], original[NAMES_ROM_INTERNAL]);
    assert_ne!(rom, original, "as falas foram traduzidas");
}

#[tokio::test]
async fn unmarked_internal_name_goes_to_the_next_translation() {
    let tmp = TempDir::new("unmark");
    let dir = tmp.project();
    let provider = FakeProvider::new(&[]);
    translate(&dir, None, &provider).await;
    assert!(!provider.asked().contains(&"SALA12".to_string()));

    let id = ascii(&dir, "SALA12").id;
    let mut db = ProjectDb::open(&dir).unwrap();
    unmark_internal(&mut db, &id).unwrap();
    assert_eq!(
        ascii(&dir, "SALA12").status,
        TranslationStatus::Untranslated
    );
    // So desfaz o que esta marcado.
    assert!(unmark_internal(&mut db, &id).is_err());
    assert!(unmark_internal(&mut db, "nao-existe").is_err());
    drop(db);

    let again = FakeProvider::new(&[("SALA12", "QUARTO12")]);
    let summary = translate(&dir, None, &again).await;
    assert_eq!(again.asked(), ["SALA12"]);
    assert_eq!(summary.translated, 1);
    assert_eq!(
        ascii(&dir, "SALA12").translated_text.as_deref(),
        Some("QUARTO12")
    );
}

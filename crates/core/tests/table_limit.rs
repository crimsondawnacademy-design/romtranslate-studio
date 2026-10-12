//! O teto em caracteres que vai pro provider e o que a memoria de traducao
//! pode devolver pra texto de tabela gravado no lugar. Sem ROM: o projeto e
//! um banco, uma tabela escrita a mao e entries codificadas por ela.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use async_trait::async_trait;
use romtranslate_core::db::{GlobalTm, ProjectDb};
use romtranslate_core::error::Result;
use romtranslate_core::pipeline::{run_translation, TranslateOptions, TranslateSummary};
use romtranslate_core::provider::{
    TranslatedItem, TranslationProvider, TranslationRequest, TranslationResponse,
};
use romtranslate_core::tbl::TblTable;
use romtranslate_core::types::{TextEncoding, TextEntry, TranslationStatus};
use romtranslate_core::validate::validate_project_db;

/// Letras maiusculas de 1 byte a partir de 0x80, espaco, e dois codigos de
/// controle de 1 byte.
fn table_text(extra: &str) -> String {
    let letters: String = (0..26u8)
        .map(|i| format!("{:02X}={}\n", 0x80 + i, (b'A' + i) as char))
        .collect();
    format!("{letters}40= \nFE=[FIM]\nFD=[NOME]\n{extra}")
}

struct Project {
    dir: PathBuf,
    table: TblTable,
}

impl Project {
    fn new(label: &str, tbl: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-tbllim-{label}-{nanos}"));
        fs::create_dir_all(dir.join("tables")).unwrap();
        fs::write(dir.join("tables").join("jogo.tbl"), tbl).unwrap();
        Project {
            dir,
            table: TblTable::parse(tbl).unwrap(),
        }
    }

    /// Entry de tabela com os bytes que a propria tabela da pro texto.
    fn entry(&self, id: &str, source: &str, terminated: bool) -> TextEntry {
        TextEntry {
            id: id.to_string(),
            resource_path: None,
            offset: None,
            original_bytes: self.table.encode(source).unwrap(),
            source_text: source.to_string(),
            translated_text: None,
            context: None,
            max_bytes: None,
            encoding: TextEncoding::Table("jogo".into()),
            status: TranslationStatus::Untranslated,
            metadata: serde_json::json!({ "terminated": terminated }),
        }
    }

    fn db(&self) -> ProjectDb {
        ProjectDb::open(&self.dir).unwrap()
    }

    fn translation(&self, id: &str) -> Option<String> {
        self.db().get_entry(id).unwrap().unwrap().translated_text
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Responde o que esta em `answers` (por texto original) e o proprio
/// original pro resto. Guarda os pedidos.
struct FakeProvider {
    answers: Vec<(&'static str, &'static str)>,
    requests: Mutex<Vec<TranslationRequest>>,
}

impl FakeProvider {
    fn new(answers: &[(&'static str, &'static str)]) -> Self {
        FakeProvider {
            answers: answers.to_vec(),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// (texto, maxChars) de tudo o que foi pedido.
    fn asked(&self) -> Vec<(String, Option<usize>)> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .flat_map(|r| r.items.iter().map(|i| (i.text.clone(), i.max_chars)))
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
                        .iter()
                        .find(|(source, _)| *source == item.text)
                        .map_or(item.text.as_str(), |(_, answer)| answer)
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
    provider: &FakeProvider,
) -> TranslateSummary {
    let opts = TranslateOptions {
        source_language: Some("en-US".to_string()),
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

#[tokio::test]
async fn control_codes_copied_from_the_original_do_not_eat_the_limit() {
    let project = Project::new("codes", &table_text(""));
    project
        .db()
        .upsert_entries(&[
            project.entry("a", "HELLO[FIM]", true),
            project.entry("b", "[NOME] WINS[FIM]", true),
            project.entry("c", "[FIM]", true),
            project.entry("d", "CONTINUE", true),
        ])
        .unwrap();

    // "HELLO[FIM]" ocupa 6 bytes e tem 10 caracteres. O codigo custa 1 byte
    // e o modelo conta 5: o teto e 5 letras mais o codigo, nao 6 caracteres.
    let provider = FakeProvider::new(&[
        ("HELLO[FIM]", "SALVE[FIM]"),
        ("[NOME] WINS[FIM]", "[NOME] LEVA[FIM]"),
    ]);
    translate(&project.dir, None, &provider).await;
    let mut asked = provider.asked();
    asked.sort();
    assert_eq!(
        asked,
        [
            ("CONTINUE".to_string(), Some(8)),
            ("HELLO[FIM]".to_string(), Some(10)),
            ("[FIM]".to_string(), Some(5)),
            ("[NOME] WINS[FIM]".to_string(), Some(16)),
        ]
    );

    // Resposta do tamanho do teto, com os codigos copiados, cabe: nenhum
    // erro de validacao.
    assert_eq!(project.translation("a").as_deref(), Some("SALVE[FIM]"));
    let report = validate_project_db(&mut project.db()).unwrap();
    assert_eq!(report.errors, 0, "{:?}", report.issues);
}

#[tokio::test]
async fn code_only_string_never_gets_a_zero_limit() {
    // Um caractere de 2 bytes na tabela derruba a conta de 1 byte pra zero.
    let project = Project::new("zero", &table_text("FF21=!\n"));
    project
        .db()
        .upsert_entries(&[
            project.entry("a", "[FIM]", true),
            project.entry("b", "HELLO[FIM]", true),
        ])
        .unwrap();

    let provider = FakeProvider::new(&[]);
    translate(&project.dir, None, &provider).await;
    let mut asked = provider.asked();
    asked.sort();
    // "HELLO[FIM]": sobram 5 bytes fora o codigo, 2 caracteres no pior caso.
    assert_eq!(
        asked,
        [
            ("HELLO[FIM]".to_string(), Some(7)),
            ("[FIM]".to_string(), Some(5)),
        ]
    );
}

#[tokio::test]
async fn memory_hit_larger_than_the_space_goes_to_the_provider() {
    let project = Project::new("tm", &table_text(""));
    project
        .db()
        .upsert_entries(&[
            project.entry("continue", "CONTINUE", true),
            project.entry("start", "START GAME", true),
            project.entry("options", "OPTIONS", true),
        ])
        .unwrap();

    // Memoria global vinda de um projeto sem tabela: uma traducao maior que
    // o espaco (8 bytes) e uma que cabe (10 bytes).
    let mut global = GlobalTm::open(&project.dir.join("global_tm.sqlite")).unwrap();
    for (source, translation) in [("CONTINUE", "CONTINUAR"), ("START GAME", "NOVO JOGO")] {
        global
            .tm_store(source, "en-US", "pt-BR", translation)
            .unwrap();
    }
    // Memoria do projeto com uma resposta antiga que estourou (7 bytes).
    project
        .db()
        .tm_store("OPTIONS", "en-US", "pt-BR", "CONFIGURAR")
        .unwrap();

    let provider = FakeProvider::new(&[("CONTINUE", "SEGUIR"), ("OPTIONS", "OPCOES")]);
    let summary = translate(&project.dir, Some(&mut global), &provider).await;

    // So a que cabe sai da memoria; as outras duas vao pro provider, com o
    // teto de cada uma.
    assert_eq!(summary.tm_hits, 1);
    assert_eq!(summary.translated, 2);
    let mut asked = provider.asked();
    asked.sort();
    assert_eq!(
        asked,
        [
            ("CONTINUE".to_string(), Some(8)),
            ("OPTIONS".to_string(), Some(7)),
        ]
    );
    assert_eq!(project.translation("start").as_deref(), Some("NOVO JOGO"));
    assert_eq!(project.translation("continue").as_deref(), Some("SEGUIR"));
    assert_eq!(project.translation("options").as_deref(), Some("OPCOES"));
    let report = validate_project_db(&mut project.db()).unwrap();
    assert_eq!(report.errors, 0, "{:?}", report.issues);
}

#[tokio::test]
async fn memory_hit_that_cannot_fill_a_fixed_width_string_is_not_used() {
    // Tabela sem espaco: string de largura fixa so aceita o tamanho exato.
    let letters: String = table_text("")
        .lines()
        .filter(|line| !line.ends_with("= "))
        .map(|line| format!("{line}\n"))
        .collect();
    let project = Project::new("fixed", &letters);
    project
        .db()
        .upsert_entries(&[
            project.entry("exact", "START", false),
            project.entry("short", "PRESS", false),
        ])
        .unwrap();
    for (source, translation) in [("START", "JOGAR"), ("PRESS", "TOCA")] {
        project
            .db()
            .tm_store(source, "en-US", "pt-BR", translation)
            .unwrap();
    }

    let provider = FakeProvider::new(&[("PRESS", "TOQUE")]);
    let summary = translate(&project.dir, None, &provider).await;
    assert_eq!(summary.tm_hits, 1);
    assert_eq!(provider.asked(), [("PRESS".to_string(), Some(5))]);
    assert_eq!(project.translation("exact").as_deref(), Some("JOGAR"));
    assert_eq!(project.translation("short").as_deref(), Some("TOQUE"));
}

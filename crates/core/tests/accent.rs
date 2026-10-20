//! Passada de acentuacao: a traducao feita sem acento (a fonte do jogo nao
//! tinha a letra) ganha os acentos, e so isso — a resposta que muda qualquer
//! outra coisa e recusada e a frase fica como estava.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use async_trait::async_trait;
use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::extract_project;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::error::Result;
use romtranslate_core::pipeline::{run_accent_pass, TranslateOptions};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::provider::{
    Task, TranslatedItem, TranslationProvider, TranslationRequest, TranslationResponse,
};
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
        let dir = std::env::temp_dir().join(format!("rts-acento-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Devolve a resposta combinada pra cada texto; o que nao esta no mapa volta igual.
struct FakeProvider {
    answers: HashMap<&'static str, &'static str>,
    requests: Mutex<Vec<TranslationRequest>>,
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

const SPEECH: &str = "[E3:10]The castle is quiet tonight.[E6]Too quiet.";
const PLAIN: &str = "[E3:10]O castelo esta quieto.[E6]Nao ha ninguem.";
const ACCENTED: &str = "[E3:10]O castelo está quieto.[E6]Não há ninguém.";

/// O banco sintetico do perfil, com letras acentuadas na tabela e traduzido
/// sem acento.
fn project(tmp: &TempDir) -> PathBuf {
    let source = tmp.0.join("banco.gba");
    fs::write(&source, synth::make_profile_bank()).unwrap();
    let dir = tmp.0.join("banco.rtsproj");
    create_project(CreateProjectArgs {
        source_path: source,
        platform: Platform::Gba,
        adapter_id: GbaAdapter.id().to_string(),
        source_language: None,
        target_language: "pt-BR".to_string(),
        project_dir: dir.clone(),
    })
    .unwrap();
    fs::create_dir_all(dir.join("tables")).unwrap();
    let mut table = synth::profile_table();
    table.push_str("6B=á\n6C=ã\n6D=ç\n6E=ê\n6F=é\n70=í\n");
    fs::write(dir.join("tables/letras.tbl"), table).unwrap();
    fs::write(dir.join("perfil.toml"), synth::profile_toml()).unwrap();
    extract_project(&dir).unwrap();
    let mut db = ProjectDb::open(&dir).unwrap();
    let mut entries = db.load_entries().unwrap();
    for e in entries.iter_mut() {
        let t = match e.source_text.as_str() {
            "Bat" => "Morcego",
            SPEECH => PLAIN,
            "Restores 100 HP." => "Cura 100 HP.",
            "Skeleton" => "Esqueleto",
            _ => continue,
        };
        e.translated_text = Some(t.to_string());
        e.status = TranslationStatus::Machine;
    }
    db.upsert_entries(&entries).unwrap();
    dir
}

fn translation(dir: &Path, source: &str) -> String {
    ProjectDb::open(dir)
        .unwrap()
        .load_entries()
        .unwrap()
        .into_iter()
        .find(|e| e.source_text == source)
        .and_then(|e| e.translated_text)
        .unwrap()
}

#[test]
fn only_an_answer_that_adds_accents_and_nothing_else_is_saved() {
    let tmp = TempDir::new("passada");
    let dir = project(&tmp);
    let provider = FakeProvider {
        answers: HashMap::from([
            (PLAIN, ACCENTED),
            // Troca palavra: recusada.
            ("Cura 100 HP.", "Cura 100 PV."),
            // Letra que a fonte nao tem (ô): recusada.
            ("Morcego", "Morcegô"),
        ]),
        requests: Mutex::new(Vec::new()),
    };
    let mut db = ProjectDb::open(&dir).unwrap();
    let opts = TranslateOptions::new("pt-BR");
    let summary = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run_accent_pass(
            &mut db,
            &provider,
            &opts,
            &AtomicBool::new(false),
            &|_| {},
        ))
        .unwrap();

    // 5 frases, 4 textos (as duas copias de "Morcego" vao uma vez).
    assert_eq!((summary.candidates, summary.sent), (5, 4));
    assert_eq!(summary.changed, 1);
    assert_eq!(summary.unchanged, 1, "Esqueleto nao precisa de acento");
    assert_eq!(
        summary.rejected, 3,
        "PV no lugar de HP, e as duas copias de Morcegô"
    );
    assert_eq!(summary.failed, 0);

    assert_eq!(translation(&dir, SPEECH), ACCENTED);
    assert_eq!(translation(&dir, "Restores 100 HP."), "Cura 100 HP.");
    assert_eq!(translation(&dir, "Bat"), "Morcego");

    // O pedido e de acentuacao, com as letras que a tabela grava.
    let requests = provider.requests.lock().unwrap();
    assert!(requests.iter().all(|r| r.task == Task::Accent));
    assert_eq!(requests[0].charset.as_deref(), Some("áãçéêí"));

    // A frase acentuada ocupa o mesmo espaco e grava.
    let outcome = reinsert_project(&dir, false).unwrap();
    assert!(outcome.verification.ok);
}

#[test]
fn a_translation_that_already_has_accents_is_not_sent_again() {
    let tmp = TempDir::new("ja");
    let dir = project(&tmp);
    let mut db = ProjectDb::open(&dir).unwrap();
    let mut entries = db.load_entries().unwrap();
    for e in entries.iter_mut() {
        if e.source_text == SPEECH {
            e.translated_text = Some(ACCENTED.to_string());
        }
    }
    db.upsert_entries(&entries).unwrap();
    let provider = FakeProvider {
        answers: HashMap::new(),
        requests: Mutex::new(Vec::new()),
    };
    let summary = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run_accent_pass(
            &mut db,
            &provider,
            &TranslateOptions::new("pt-BR"),
            &AtomicBool::new(false),
            &|_| {},
        ))
        .unwrap();
    assert_eq!(summary.candidates, 4);
    assert!(provider
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|r| r.items.iter().all(|i| i.text != ACCENTED)));
}

//! Jogo em japones: texto em Shift-JIS e lido ao lado do ASCII (uma leitura
//! so por faixa de bytes), a traducao volta em Shift-JIS no lugar ou
//! realocada, letra acentuada e erro que diz qual e, e a IA recebe o espaco
//! da frase ja no primeiro pedido.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use async_trait::async_trait;
use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::extract_project;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::adapters::ps2::Ps2Adapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::error::Result;
use romtranslate_core::pipeline::{run_translation, TranslateOptions};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::provider::{
    TranslatedItem, TranslationProvider, TranslationRequest, TranslationResponse,
};
use romtranslate_core::reinsert::reinsert_project;
use romtranslate_core::scan::{scan_bytes, ScanConfig, ScanEncoding};
use romtranslate_core::sjis;
use romtranslate_core::synth::{
    self, JP_FOUND, JP_GREETING, JP_KANJI_ONLY, JP_MIXED, JP_NO, JP_YES,
};
use romtranslate_core::types::{Platform, TextEncoding, TextEntry, TranslationStatus};
use romtranslate_core::validate::{validate_entry, validate_project_db, IssueKind, Severity};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-jp-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// Projeto de GBA do `make_gba_rom_japanese`, ja extraido.
    fn project(&self) -> (PathBuf, Vec<u8>) {
        let rom = synth::make_gba_rom_japanese();
        let source = self.0.join("game.gba");
        fs::write(&source, &rom).unwrap();
        let dir = self.0.join("game.rtsproj");
        create_project(CreateProjectArgs {
            source_path: source,
            platform: Platform::Gba,
            adapter_id: GbaAdapter.id().to_string(),
            source_language: Some("ja".to_string()),
            target_language: "pt-BR".to_string(),
            project_dir: dir.clone(),
        })
        .unwrap();
        extract_project(&dir).unwrap();
        (dir, rom)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn by_text<'a>(entries: &'a [TextEntry], text: &str) -> &'a TextEntry {
    entries
        .iter()
        .find(|e| e.source_text == text)
        .unwrap_or_else(|| panic!("\"{text}\" nao foi extraida"))
}

fn translated(entry: &TextEntry, text: &str) -> TextEntry {
    TextEntry {
        translated_text: Some(text.to_string()),
        status: TranslationStatus::Machine,
        ..entry.clone()
    }
}

fn set_translations(dir: &Path, pairs: &[(&str, &str)]) {
    let mut db = ProjectDb::open(dir).unwrap();
    let mut entries = db.load_entries().unwrap();
    for e in entries.iter_mut() {
        if let Some((_, t)) = pairs.iter().find(|(s, _)| *s == e.source_text) {
            *e = translated(e, t);
        }
    }
    db.upsert_entries(&entries).unwrap();
}

fn pointer_at(rom: &[u8], at: usize) -> usize {
    (u32::from_le_bytes(rom[at..at + 4].try_into().unwrap()) - 0x0800_0000) as usize
}

/// Provider de mentira: responde o que esta em `answers` e guarda os pedidos.
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
                        .unwrap_or("Ok")
                        .to_string(),
                })
                .collect(),
        })
    }

    async fn health_check(&self) -> Result<()> {
        Ok(())
    }
}

#[test]
fn japanese_text_is_extracted_next_to_the_ascii_and_kanji_alone_is_not() {
    let rom = synth::make_gba_rom_japanese();
    let entries = GbaAdapter.extract_structured(&rom).unwrap();

    let greeting = by_text(&entries, JP_GREETING);
    assert_eq!(greeting.encoding, TextEncoding::ShiftJis);
    assert_eq!(greeting.offset, Some(0x400));
    // 15 caracteres de dois bytes e um espaco: e o espaco que a traducao tem.
    assert_eq!(greeting.max_bytes, Some(31));
    assert_eq!(greeting.original_bytes, sjis::encode(JP_GREETING).unwrap());
    assert!(greeting.pointers().is_empty());

    // Resposta de menu de 2 e 3 caracteres, com a tabela de ponteiros.
    for (text, pointer) in [(JP_YES, 0x800), (JP_NO, 0x804), (JP_FOUND, 0x808)] {
        let e = by_text(&entries, text);
        assert_eq!(e.encoding, TextEncoding::ShiftJis, "{text}");
        assert_eq!(e.pointer_offsets(), vec![pointer], "{text}");
        assert_eq!(e.max_bytes, None, "realocavel: {text}");
    }
    assert_eq!(by_text(&entries, JP_MIXED).encoding, TextEncoding::ShiftJis);

    // O ASCII do mesmo cartucho sai como sempre saiu.
    for text in ["NEW GAME", "WELCOME TO THE VILLAGE!", "POTION"] {
        assert_eq!(by_text(&entries, text).encoding, TextEncoding::Ascii);
    }
    // So kanji, sem kana: fica de fora (ver `sjis::looks_japanese`).
    assert!(!entries.iter().any(|e| e.source_text == JP_KANJI_ONLY));
    // Uma leitura so por faixa de bytes: nenhuma entry dentro de outra.
    let mut spans: Vec<(u64, u64)> = entries
        .iter()
        .map(|e| {
            let start = e.offset.unwrap();
            (start, start + e.original_bytes.len() as u64)
        })
        .collect();
    spans.sort_unstable();
    assert!(spans.windows(2).all(|pair| pair[0].1 <= pair[1].0));
}

#[test]
fn the_explicit_shift_jis_scan_reads_everything_and_counts_bytes() {
    let rom = synth::make_gba_rom_japanese();
    let config = ScanConfig {
        encoding: ScanEncoding::ShiftJis,
        ..ScanConfig::default()
    };
    let entries = scan_bytes(&rom, &config).unwrap().entries;
    assert!(entries.iter().all(|e| e.encoding == TextEncoding::ShiftJis));
    // "はい" tem 2 caracteres e 4 bytes: passa no minimo de 4.
    for text in [JP_GREETING, JP_YES, JP_NO, JP_KANJI_ONLY, "NEW GAME"] {
        by_text(&entries, text);
    }
    // O scanner ASCII comum nao le japones, e o modo dos adapters e opcional.
    let plain = scan_bytes(&rom, &ScanConfig::default()).unwrap().entries;
    assert!(plain.iter().all(|e| e.encoding == TextEncoding::Ascii));
    assert!(!plain.iter().any(|e| e.source_text == JP_YES));
}

#[test]
fn an_accented_translation_is_an_error_that_names_the_letter() {
    let rom = synth::make_gba_rom_japanese();
    let entries = GbaAdapter.extract_structured(&rom).unwrap();
    let greeting = by_text(&entries, JP_GREETING);

    let issues = validate_entry(&translated(greeting, "Olá, viajante."), None);
    assert!(
        issues.iter().any(|i| i.kind == IssueKind::Unencodable
            && i.severity == Severity::Error
            && i.message.contains("'á'")),
        "{issues:?}"
    );
    // Sem acento e dentro do espaco: nada a dizer.
    let issues = validate_entry(&translated(greeting, "Bem-vindo a vila, viajante."), None);
    assert!(
        !issues.iter().any(|i| i.severity == Severity::Error),
        "{issues:?}"
    );
    // O que o Shift-JIS escreve vale, kana e pontuacao japonesa inclusive.
    let issues = validate_entry(&translated(greeting, "ようこそ！ Ola."), None);
    assert!(
        !issues.iter().any(|i| i.kind == IssueKind::Unencodable),
        "{issues:?}"
    );
    // Maior que o espaco, numa frase sem ponteiro: erro.
    let long = "Seja muito bem-vindo a nossa pequena vila, caro viajante.";
    let issues = validate_entry(&translated(greeting, long), None);
    assert!(
        issues
            .iter()
            .any(|i| i.kind == IssueKind::ByteOverflow && i.severity == Severity::Error),
        "{issues:?}"
    );
}

#[test]
fn the_translation_goes_back_in_place_or_relocated() {
    let tmp = TempDir::new("reinsert");
    let (dir, rom) = tmp.project();
    let found = "Voce encontrou uma erva medicinal!";
    set_translations(
        &dir,
        &[
            (JP_GREETING, "Bem-vindo a vila, viajante."),
            (JP_YES, "Sim"),
            (JP_NO, "Nao"),
            (JP_FOUND, found),
        ],
    );
    let report = validate_project_db(&mut ProjectDb::open(&dir).unwrap()).unwrap();
    assert!(
        !report.issues.iter().any(|i| i.severity == Severity::Error),
        "{:?}",
        report.issues
    );

    let outcome = reinsert_project(&dir, false).unwrap();
    assert_eq!(outcome.apply.applied, 4);
    assert_eq!(outcome.apply.relocated, 1);
    assert!(outcome.verification.ok, "{:?}", outcome.verification);
    let working = fs::read(&outcome.working_path).unwrap();

    // No lugar, com a sobra do espaco zerada.
    let mut greeting = b"Bem-vindo a vila, viajante.".to_vec();
    greeting.resize(31, 0);
    assert_eq!(working[0x400..0x400 + 31], greeting[..]);
    assert_eq!(&working[0x440..0x445], b"Sim\0\0");
    assert_eq!(&working[0x448..0x44F], b"Nao\0\0\0\0");
    // O aviso do item nao cabia nos 23 bytes: foi pro fim do ROM e a tabela
    // aponta pra la. O original fica intacto.
    let moved = pointer_at(&working, 0x808);
    assert_eq!(moved, rom.len());
    assert_eq!(
        &working[moved..moved + found.len() + 1],
        format!("{found}\0").as_bytes()
    );
    assert_eq!(working[0x460..0x480], rom[0x460..0x480]);
    assert_eq!(pointer_at(&working, 0x800), 0x440);

    // A copia traduzida re-extrai em portugues.
    let again = GbaAdapter.extract_structured(&working).unwrap();
    by_text(&again, "Bem-vindo a vila, viajante.");
    by_text(&again, found);
}

#[tokio::test]
async fn the_ai_gets_the_space_up_front_and_accents_become_plain_letters() {
    let tmp = TempDir::new("translate");
    let (dir, _) = tmp.project();
    let provider = FakeProvider {
        answers: [
            (JP_GREETING, "Olá, viajante. Bem-vindo à vila!"),
            (JP_YES, "Sim"),
            (JP_NO, "Não"),
        ]
        .into_iter()
        .collect(),
        requests: Mutex::new(Vec::new()),
    };
    let opts = TranslateOptions {
        source_language: Some("ja".to_string()),
        ..TranslateOptions::new("pt-BR")
    };
    let mut db = ProjectDb::open(&dir).unwrap();
    run_translation(
        &mut db,
        None,
        &provider,
        "fake-model",
        &opts,
        &AtomicBool::new(false),
        &|_| {},
    )
    .await
    .unwrap();

    let requests = provider.requests.lock().unwrap().clone();
    let item = |text: &str| {
        requests
            .iter()
            .flat_map(|r| r.items.iter())
            .find(|i| i.text == text)
            .unwrap_or_else(|| panic!("{text} nao foi pedida"))
            .clone()
    };
    // Frase presa ao espaco dela: o teto vai ja no primeiro pedido.
    assert_eq!(item(JP_GREETING).max_chars, Some(31));
    // Com ponteiro em tabela ela pode crescer: sem teto.
    assert_eq!(item(JP_FOUND).max_chars, None);
    // O pedido diz que nao ha letra acentuada, como em texto ASCII.
    let with_japanese = requests
        .iter()
        .find(|r| r.items.iter().any(|i| i.text == JP_GREETING))
        .unwrap();
    let charset = with_japanese.charset.as_deref().expect("conjunto ASCII");
    assert!(charset.contains('A') && !charset.contains('á'));
    assert_eq!(with_japanese.source_language.as_deref(), Some("ja"));

    let entries = db.load_entries().unwrap();
    assert_eq!(
        by_text(&entries, JP_GREETING).translated_text.as_deref(),
        Some("Ola, viajante. Bem-vindo a vila!")
    );
    assert_eq!(
        by_text(&entries, JP_NO).translated_text.as_deref(),
        Some("Nao")
    );
    assert!(db
        .accent_swaps()
        .unwrap()
        .contains_key(&by_text(&entries, JP_NO).id));
}

#[test]
fn a_disc_file_in_shift_jis_is_read_and_written_back() {
    let iso = synth::make_ps2_iso_japanese();
    let mut entries = Ps2Adapter.extract_structured(&iso).unwrap();
    let greeting = by_text(&entries, JP_GREETING);
    assert_eq!(greeting.encoding, TextEncoding::ShiftJis);
    assert_eq!(greeting.resource_path.as_deref(), Some("DATA/MSG.BIN"));
    assert_eq!(by_text(&entries, JP_YES).encoding, TextEncoding::ShiftJis);
    assert_eq!(by_text(&entries, "CONTINUE").encoding, TextEncoding::Ascii);

    let at = greeting.offset.unwrap() as usize;
    for e in entries.iter_mut() {
        if e.source_text == JP_GREETING {
            e.translated_text = Some("Bem-vindo a vila, viajante.".to_string());
        }
    }
    let applied = Ps2Adapter.apply_text(&iso, &entries).unwrap();
    assert_eq!(applied.report.applied, 1);
    assert_eq!(&applied.bytes[at..at + 27], b"Bem-vindo a vila, viajante.");
    assert_eq!(applied.bytes[at + 27..at + 31], [0; 4]);
    assert_eq!(applied.bytes.len(), iso.len());
}

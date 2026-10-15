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

/// Provider que tira a quebra de linha na primeira resposta e so poe de
/// volta quando o pedido diz que faltou codigo.
struct ForgetfulProvider {
    asked_again: Mutex<usize>,
}

#[async_trait]
impl TranslationProvider for ForgetfulProvider {
    fn id(&self) -> &'static str {
        "forgetful"
    }

    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse> {
        let translations = request
            .items
            .iter()
            .map(|item| {
                let again = item
                    .context
                    .as_deref()
                    .is_some_and(|c| c.contains("dropped or added codes"));
                *self.asked_again.lock().unwrap() += usize::from(again);
                let translation = match (item.text.contains("[FF01]"), again) {
                    (false, _) => "Mel",
                    (true, false) => "◆Oi tchau.",
                    (true, true) => "◆Oi[FF01]tchau.",
                };
                TranslatedItem {
                    id: item.id.clone(),
                    translation: translation.to_string(),
                }
            })
            .collect();
        Ok(TranslationResponse { translations })
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

// --- Texto em codigos de 16 bits na ordem do JIS (`kuten`) ---

mod jis16 {
    use super::*;
    use romtranslate_core::kuten;
    use romtranslate_core::reinsert::{reinsert_project_with, ErrorPolicy};
    use romtranslate_core::synth::{JIS16_GLYPH_NAME, JIS16_NAMES, JIS16_SCRIPT_AT};
    use romtranslate_core::tbl::{ProjectTables, TblTable};

    fn table_encoding() -> TextEncoding {
        TextEncoding::Table(kuten::TABLE_ID.to_string())
    }

    fn project(tmp: &TempDir) -> (PathBuf, Vec<u8>) {
        let rom = synth::make_gba_rom_jis16();
        let source = tmp.0.join("game.gba");
        fs::write(&source, &rom).unwrap();
        let dir = tmp.0.join("game.rtsproj");
        create_project(CreateProjectArgs {
            source_path: source,
            platform: Platform::Gba,
            adapter_id: GbaAdapter.id().to_string(),
            source_language: Some("ja".to_string()),
            target_language: "pt-BR".to_string(),
            project_dir: dir.clone(),
        })
        .unwrap();
        (dir, rom)
    }

    /// As unidades de 16 bits de `bytes`, lidas de volta pela tabela do JIS.
    fn read_units(bytes: &[u8]) -> String {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .map(|code| match code {
                synth::JIS16_SPACE => " ".to_string(),
                code if code >= 0xFF00 => format!("[{code:04X}]"),
                code => kuten::decode(code).map_or("?".to_string(), String::from),
            })
            .collect()
    }

    #[test]
    fn the_messages_come_out_whole_with_the_table_that_reads_them() {
        let rom = synth::make_gba_rom_jis16();
        let out = GbaAdapter.extract(&rom).unwrap();
        let japanese: Vec<&TextEntry> = out
            .entries
            .iter()
            .filter(|e| e.encoding == table_encoding())
            .collect();
        assert_eq!(japanese.len(), 120 + JIS16_NAMES.len() + 1);

        // A mensagem inteira, com a quebra de linha como codigo no meio; os
        // codigos do fim (espera o botao, fim) ficam fora da entry.
        let first = by_text(&out.entries, &synth::jis16_message(0));
        assert_eq!(first.offset, Some(JIS16_SCRIPT_AT as u64));
        assert_eq!(first.max_bytes, Some(first.original_bytes.len()));
        assert_eq!(first.original_bytes.len(), 27 * 2);
        assert_eq!(read_units(&first.original_bytes), synth::jis16_message(0));
        by_text(&out.entries, &synth::jis16_message(119));
        // Nome curto entre dois 0xFFFF tambem sai.
        for name in JIS16_NAMES {
            assert_eq!(by_text(&out.entries, name).encoding, table_encoding());
        }
        // Desenho proprio do jogo no meio do nome: fica no texto como codigo.
        assert_eq!(
            by_text(&out.entries, JIS16_GLYPH_NAME).original_bytes.len(),
            4 * 2
        );
        // O texto do jogo vem na frente, e a tabela vem junto.
        assert_eq!(out.entries[0].encoding, table_encoding());
        assert_eq!(out.tables.len(), 1);
        assert_eq!(out.tables[0].0, kuten::TABLE_ID);
        let table = TblTable::parse(&out.tables[0].1).unwrap();
        assert_eq!(
            table.encode(&synth::jis16_message(0)).unwrap(),
            first.original_bytes
        );

        // Comando com parametro antes do losango (mensagem 10) e numero de
        // cabecalho (mensagem 20): nenhum dos dois entra na frase, e a
        // unidade logo antes da entry e o parametro ou o cabecalho, intacto.
        for (i, before) in [(10, 0x0003u16), (20, 0x0005)] {
            let e = by_text(&out.entries, &synth::jis16_message(i));
            let at = e.offset.unwrap() as usize;
            assert_eq!(read_units(&rom[at..at + 2]), "◆", "mensagem {i}");
            assert_eq!(rom[at - 2..at], before.to_le_bytes(), "mensagem {i}");
        }
        // Comando com parametro no meio da fala (mensagem 30): uma peca so,
        // que a tabela le e grava.
        let named = by_text(&out.entries, &synth::jis16_message(30));
        assert!(named.source_text.contains("は[FF23:0001][FF01]"));
        assert!(out.tables[0].1.contains("\n23FF0100=[FF23:0001]\n"));
        assert_eq!(
            table.encode(&named.source_text).unwrap(),
            named.original_bytes
        );

        // Num jogo assim o que o scanner de 8 bits le sai da fila.
        let ascii = by_text(&out.entries, "WELCOME TO THE VILLAGE!");
        assert_eq!(ascii.status, TranslationStatus::Internal);
        assert!(japanese
            .iter()
            .all(|e| e.status == TranslationStatus::Untranslated));
    }

    #[tokio::test]
    async fn a_translation_that_lost_a_game_code_is_asked_again_with_the_list() {
        let tmp = TempDir::new("jis16-codes");
        let (dir, _) = project(&tmp);
        extract_project(&dir).unwrap();
        let provider = ForgetfulProvider {
            asked_again: Mutex::new(0),
        };
        let mut db = ProjectDb::open(&dir).unwrap();
        run_translation(
            &mut db,
            None,
            &provider,
            "fake-model",
            &TranslateOptions::new("pt-BR"),
            &AtomicBool::new(false),
            &|_| {},
        )
        .await
        .unwrap();
        assert!(*provider.asked_again.lock().unwrap() > 0);
        let entries = db.load_entries().unwrap();
        let first = by_text(&entries, &synth::jis16_message(0));
        assert_eq!(first.translated_text.as_deref(), Some("◆Oi[FF01]tchau."));
    }

    #[test]
    fn extracting_again_drops_the_old_reading_of_the_same_bytes() {
        let tmp = TempDir::new("jis16-stale");
        let (dir, _) = project(&tmp);
        extract_project(&dir).unwrap();
        let mut db = ProjectDb::open(&dir).unwrap();
        let first = by_text(&db.load_entries().unwrap(), &synth::jis16_message(0)).clone();
        // Uma leitura antiga dos mesmos bytes, deslocada e ja traduzida (como
        // a que comecava pelo numero de um campo vizinho), e uma frase do
        // Escanear com a tabela da pessoa em cima dos mesmos bytes.
        let at = first.offset.unwrap();
        let old = TextEntry {
            id: format!("scan-{:08x}", at - 2),
            offset: Some(at - 2),
            original_bytes: vec![0; first.original_bytes.len()],
            translated_text: Some("leitura antiga".to_string()),
            ..first.clone()
        };
        let mine = TextEntry {
            id: "minha-tabela-1".to_string(),
            encoding: TextEncoding::Table("minha".to_string()),
            translated_text: Some("da pessoa".to_string()),
            ..old.clone()
        };
        db.upsert_entries(&[old.clone(), mine.clone()]).unwrap();
        drop(db);
        let outcome = extract_project(&dir).unwrap();
        assert!(outcome.removed >= 1, "{}", outcome.removed);
        let entries = ProjectDb::open(&dir).unwrap().load_entries().unwrap();
        assert!(
            !entries.iter().any(|e| e.id == old.id),
            "a leitura antiga saiu"
        );
        assert!(entries.iter().any(|e| e.id == mine.id), "a da pessoa ficou");
        assert!(entries.iter().any(|e| e.id == first.id), "a nova ficou");
    }

    #[test]
    fn a_game_without_this_encoding_gets_no_table() {
        for rom in [
            synth::make_gba_rom_japanese(),
            synth::make_gba_rom_with_junk(),
        ] {
            let out = GbaAdapter.extract(&rom).unwrap();
            assert!(out.tables.is_empty());
            assert!(!out.entries.iter().any(|e| e.encoding == table_encoding()));
        }
        assert!(kuten::extract(&synth::make_gba_rom_japanese(), &Default::default()).is_none());
    }

    #[test]
    fn plain_letters_are_written_in_their_full_width_form() {
        let table = TblTable::parse(&kuten::table_text(
            Some(synth::JIS16_SPACE),
            &Default::default(),
        ))
        .unwrap();
        // "a" nao existe na tabela japonesa; "ａ" existe, e e o que e gravado.
        let bytes = table.encode("Ola, vila!").unwrap();
        assert_eq!(bytes.len(), 10 * 2);
        assert_eq!(read_units(&bytes), "Ｏｌａ， ｖｉｌａ！");
        // Acento sai; o que nem em largura inteira existe e erro que diz qual.
        assert_eq!(
            table.drop_missing_accents("coração").as_deref(),
            Some("coracao")
        );
        assert_eq!(table.drop_missing_accents("vila"), None);
        let err = table.encode("ação").unwrap_err().to_string();
        assert!(err.contains("'ç'"), "{err}");
        // A lista pro prompt e o ASCII que a tabela escreve, nao os 7 mil
        // caracteres dela.
        let charset = table.charset();
        assert!(charset.chars().count() < 130, "{charset}");
        assert!(charset.contains('a') && charset.contains('Z') && charset.contains('!'));
        assert!(!charset.contains('あ'));
        // As marcas de fala vao junto: fora da lista, o modelo as tira.
        assert!(charset.contains('◆') && charset.contains('♪'));
        // 2 bytes por letra: num espaco de 20 bytes cabem 10.
        assert_eq!(table.chars_that_fit(20), 10);

        // Tabela com a letra comum fica com ela, sem trocar nada.
        let small = TblTable::parse("41=A\n42=a\n43=ａ\n20= ").unwrap();
        assert_eq!(small.encode("Aa").unwrap(), [0x41, 0x42]);
        assert_eq!(small.charset(), " Aaａ");
    }

    #[test]
    fn a_translation_that_fits_goes_back_and_the_rest_is_an_error() {
        let tmp = TempDir::new("jis16");
        let (dir, rom) = project(&tmp);
        let outcome = extract_project(&dir).unwrap();
        assert_eq!(outcome.jis16, 120 + JIS16_NAMES.len() + 1);
        assert!(dir.join("tables/jis16.tbl").is_file());
        // Uma segunda extracao regrava a tabela e mantem, no fim, o que a
        // pessoa mudou nela.
        let generated = fs::read_to_string(dir.join("tables/jis16.tbl")).unwrap();
        fs::write(
            dir.join("tables/jis16.tbl"),
            generated.replace("\nAD00=[00AD]\n", "\nAD00=父\n"),
        )
        .unwrap();
        extract_project(&dir).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("tables/jis16.tbl")).unwrap(),
            format!("{generated}# Linhas mudadas a mao, mantidas da tabela anterior:\nAD00=父\n")
        );
        fs::remove_file(dir.join("tables/jis16.tbl")).unwrap();
        extract_project(&dir).unwrap();

        let fits = "◆O povo[FF01]trabalha feliz.";
        let long = "◆O povo todo da vila[FF01]trabalha muito feliz hoje.";
        set_translations(
            &dir,
            &[
                (&synth::jis16_message(0), fits),
                (&synth::jis16_message(1), long),
                (&synth::jis16_message(2), "◆O povo canta."),
            ],
        );
        let report = validate_project_db(&mut ProjectDb::open(&dir).unwrap()).unwrap();
        let entries = ProjectDb::open(&dir).unwrap().load_entries().unwrap();
        let issues_of = |text: &str| -> Vec<IssueKind> {
            let id = &by_text(&entries, text).id;
            report
                .issues
                .iter()
                .filter(|i| &i.entry_id == id && i.severity == Severity::Error)
                .map(|i| i.kind)
                .collect()
        };
        assert_eq!(issues_of(&synth::jis16_message(0)), []);
        // Nao cabe: cada letra ocupa 2 bytes e a frase nao muda de lugar.
        assert!(issues_of(&synth::jis16_message(1)).contains(&IssueKind::ByteOverflow));
        // Sem a quebra de linha do original.
        assert!(issues_of(&synth::jis16_message(2)).contains(&IssueKind::PlaceholderMismatch));

        let outcome = reinsert_project_with(&dir, ErrorPolicy::Skip).unwrap();
        assert_eq!(outcome.apply.applied, 1);
        assert_eq!(outcome.skipped_errors, 2);
        let working = fs::read(&outcome.working_path).unwrap();
        let at = JIS16_SCRIPT_AT;
        // A mensagem tinha 27 unidades: a traducao ocupa 23 e 4 espacos do
        // jogo completam o campo; os codigos do fim continuam logo depois.
        assert_eq!(
            read_units(&working[at..at + 60]),
            "◆Ｏ ｐｏｖｏ[FF01]ｔｒａｂａｌｈａ ｆｅｌｉｚ．    [FF03][FF00][FFFF]"
        );
        assert_eq!(working[at + 60..], rom[at + 60..]);
        assert_eq!(working[0x1000..at], rom[0x1000..at]);

        // A tabela do projeto e a que valida: acento sai pela mesma regra.
        let mut tables = ProjectTables::new(&dir);
        let table = tables.get(kuten::TABLE_ID).unwrap();
        assert_eq!(table.drop_missing_accents("está").as_deref(), Some("esta"));
    }

    #[tokio::test]
    async fn the_ai_is_asked_in_plain_letters_with_the_space_of_each_line() {
        let tmp = TempDir::new("jis16-ia");
        let (dir, _) = project(&tmp);
        extract_project(&dir).unwrap();
        let provider = FakeProvider {
            answers: HashMap::new(),
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
        let request = requests
            .iter()
            .find(|r| r.items.iter().any(|i| i.text == synth::jis16_message(0)))
            .expect("a primeira mensagem foi pedida");
        let charset = request.charset.as_deref().expect("lista de caracteres");
        assert!(charset.chars().count() < 130 && charset.contains('a') && !charset.contains('あ'));
        assert!(romtranslate_core::provider::build_system_prompt(request).contains("keep each one"));
        // 27 unidades: 26 letras e a quebra de linha, que o modelo conta como
        // os 6 caracteres de "[FF01]".
        let item = request
            .items
            .iter()
            .find(|i| i.text == synth::jis16_message(0))
            .unwrap();
        assert_eq!(item.max_chars, Some(26 + 6));
        // O nome com um kanji que a tabela nao leu vai com o aviso de que o
        // codigo e texto, e a traducao sem ele nao e erro. Comando que some
        // continua sendo.
        let glyph = requests
            .iter()
            .flat_map(|r| r.items.iter())
            .find(|i| i.text == synth::JIS16_GLYPH_NAME)
            .expect("o nome com o desenho foi pedido");
        assert!(glyph.context.as_deref().unwrap().contains("could not read"));
        assert_eq!(
            item.context
                .as_deref()
                .map(|c| c.contains("could not read")),
            Some(false)
        );
        let entries = ProjectDb::open(&dir).unwrap().load_entries().unwrap();
        let table = romtranslate_core::tbl::TblTable::parse(
            &fs::read_to_string(dir.join("tables/jis16.tbl")).unwrap(),
        )
        .unwrap();
        let name = by_text(&entries, synth::JIS16_GLYPH_NAME);
        let issues = validate_entry(&translated(name, "Mel"), Some(&table));
        assert!(issues.is_empty(), "{issues:?}");
        let first = by_text(&entries, &synth::jis16_message(0));
        let issues = validate_entry(&translated(first, "◆Oi."), Some(&table));
        assert!(issues[0].message.contains("[FF01]"), "{issues:?}");
        // "<...>" no lugar do "＜...＞" do original e letra, nao comando; o
        // comando que a tabela grava continua contando.
        let quoted = translated(first, "◆<Aviso> \"Oi\"[FF01]tchau.");
        let issues = validate_entry(&quoted, Some(&table));
        assert!(issues.is_empty(), "{issues:?}");
        let invented = translated(first, "◆Oi[FF01]tchau[FF03].");
        let issues = validate_entry(&invented, Some(&table));
        assert!(
            issues.iter().any(|i| i.message.contains("[FF03]")),
            "{issues:?}"
        );
        // O texto de 8 bits do cartucho nao vai pra IA.
        assert!(!requests
            .iter()
            .flat_map(|r| r.items.iter())
            .any(|i| i.text == "WELCOME TO THE VILLAGE!"));
    }
}

// --- Banco de mensagens em blocos: a traducao pode crescer ---

mod bank {
    use super::*;
    use romtranslate_core::kuten;
    use romtranslate_core::reinsert::{reinsert_project_with, ErrorPolicy};
    use romtranslate_core::synth::{JIS16_BANK_AT, JIS16_BANK_PADDING, JIS16_BANK_PAIRS};

    fn u16_at(rom: &[u8], at: usize) -> usize {
        u16::from_le_bytes([rom[at], rom[at + 1]]) as usize
    }

    fn u32_at(rom: &[u8], at: usize) -> usize {
        u32::from_le_bytes(rom[at..at + 4].try_into().unwrap()) as usize
    }

    /// (tabela, texto) da sala `room`, pelo diretorio do banco.
    fn room_at(rom: &[u8], room: usize) -> (usize, usize) {
        let entry = JIS16_BANK_AT + 4 + 8 * JIS16_BANK_PAIRS[room];
        (
            JIS16_BANK_AT + u32_at(rom, entry),
            JIS16_BANK_AT + u32_at(rom, entry + 4),
        )
    }

    /// A mensagem `k` da sala `room`, lida como o jogo le: diretorio, tabela,
    /// e dali ate o 0xFFFF. Comandos e numeros aparecem como codigo.
    fn message(rom: &[u8], room: usize, k: usize) -> String {
        let (table, text) = room_at(rom, room);
        let mut at = text + u16_at(rom, table + 2 * k);
        let mut out = String::new();
        while u16_at(rom, at) != 0xFFFF {
            let code = u16_at(rom, at) as u16;
            out.push_str(&match code {
                synth::JIS16_SPACE => " ".to_string(),
                code if code >= 0xFF00 => format!("[{code:04X}]"),
                code => kuten::decode(code).map_or(format!("<{code:04X}>"), String::from),
            });
            at += 2;
        }
        out
    }

    fn project(tmp: &TempDir, padding: bool) -> (PathBuf, Vec<u8>) {
        let rom = synth::make_gba_rom_jis16_bank(padding);
        let source = tmp.0.join("game.gba");
        fs::write(&source, &rom).unwrap();
        let dir = tmp.0.join("game.rtsproj");
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

    /// Uma traducao maior que o original (36 unidades contra 27), com as
    /// duas linhas dentro da caixa de 18 caracteres.
    const LONG: &str = "◆Todo o povo aqui[FF01]segue no trabalho.";

    #[test]
    fn messages_in_a_bank_can_grow_and_know_the_width_of_the_box() {
        let rom = synth::make_gba_rom_jis16_bank(true);
        let out = GbaAdapter.extract(&rom).unwrap();
        let first = by_text(&out.entries, &synth::jis16_message(0));
        assert_eq!(first.max_bytes, None);
        assert!(first.can_grow());
        assert_eq!(first.metadata["bank"]["at"], JIS16_BANK_AT);
        assert_eq!(first.metadata["bank"]["pair"], JIS16_BANK_PAIRS[0]);
        // A maior linha do roteiro: "おいしい スープを つくっている。" e o
        // sujeito mais comprido com o losango.
        let longest = (0..120)
            .map(|i| {
                synth::jis16_message(i)
                    .split(['[', ']'])
                    .step_by(2)
                    .map(|line| line.chars().count())
                    .max()
                    .unwrap()
            })
            .max()
            .unwrap();
        assert_eq!(first.metadata["bank"]["line"], longest);
        // As 120 mensagens, das seis salas, todas marcadas.
        let banked = out.entries.iter().filter(|e| e.can_grow()).count();
        assert_eq!(banked, 120);
        by_text(&out.entries, &synth::jis16_message(119));

        // Maior que o original nao e erro, e dentro da caixa nem aviso de
        // linha; linha maior que a caixa e aviso.
        let table = romtranslate_core::tbl::TblTable::parse(&out.tables[0].1).unwrap();
        let issues = validate_entry(&translated(first, LONG), Some(&table));
        assert!(
            !issues
                .iter()
                .any(|i| i.severity == Severity::Error || i.message.contains("caixa de texto")),
            "{issues:?}"
        );
        let wide = "◆O povo todo desta vila aqui[FF01]trabalha.";
        let issues = validate_entry(&translated(first, wide), Some(&table));
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Warning && i.message.contains("caixa de texto")),
            "{issues:?}"
        );
    }

    #[test]
    fn a_block_that_grows_moves_to_the_free_space_and_the_game_still_finds_it() {
        let tmp = TempDir::new("bank-grow");
        let (dir, rom) = project(&tmp, true);
        set_translations(&dir, &[(&synth::jis16_message(0), LONG)]);
        let outcome = reinsert_project_with(&dir, ErrorPolicy::Block).unwrap();
        assert_eq!(outcome.apply.applied, 1);
        assert_eq!(outcome.apply.relocated, 1);
        assert_eq!(outcome.apply.no_room, 0);
        let working = fs::read(&outcome.working_path).unwrap();
        assert_eq!(working.len(), rom.len());

        // A sala 0 foi pro preenchimento, tabela e texto colados.
        let (old_table, old_text) = room_at(&rom, 0);
        let (table, text) = room_at(&working, 0);
        assert!(JIS16_BANK_PADDING.contains(&table), "0x{table:X}");
        assert_eq!(text - table, old_text - old_table);
        // Lida pelo diretorio: a traducao, em letras de largura inteira, com a
        // quebra de linha e os comandos do fim no lugar.
        assert_eq!(
            message(&working, 0, 0),
            "◆Ｔｏｄｏ ｏ ｐｏｖｏ ａｑｕｉ[FF01]ｓｅｇｕｅ ｎｏ ｔｒａｂａｌｈｏ．[FF03][FF00]"
        );
        // As outras 19 mensagens da sala continuam inteiras, no lugar novo.
        for k in 1..20 {
            assert_eq!(message(&working, 0, k), message(&rom, 0, k), "mensagem {k}");
        }
        // O lugar antigo foi limpo, e o resto do banco nao foi tocado.
        let (next_table, _) = room_at(&rom, 1);
        assert!(working[old_table..next_table].iter().all(|b| *b == 0xFF));
        for room in 1..6 {
            assert_eq!(room_at(&working, room), room_at(&rom, room));
            for k in 0..20 {
                assert_eq!(message(&working, room, k), message(&rom, room, k));
            }
        }
        assert_eq!(working[..JIS16_BANK_AT], rom[..JIS16_BANK_AT]);
        assert_eq!(
            working[JIS16_BANK_PADDING.end..],
            rom[JIS16_BANK_PADDING.end..]
        );
    }

    #[test]
    fn a_block_that_still_fits_stays_where_it_was() {
        let tmp = TempDir::new("bank-stay");
        let (dir, rom) = project(&tmp, true);
        // Mais curta que o original, na mensagem 11 (sala 0, com o comando e
        // o parametro antes do losango) e na 20 (a primeira da sala 1, com o
        // numero da sala e o numero de cabecalho antes dela).
        set_translations(
            &dir,
            &[
                (&synth::jis16_message(11), "◆Eles[FF01]cantam."),
                (&synth::jis16_message(20), "◆Ela[FF01]dorme."),
            ],
        );
        let outcome = reinsert_project_with(&dir, ErrorPolicy::Block).unwrap();
        assert_eq!((outcome.apply.applied, outcome.apply.relocated), (2, 0));
        let working = fs::read(&outcome.working_path).unwrap();
        for room in 0..2 {
            assert_eq!(room_at(&working, room), room_at(&rom, room));
        }
        // O comando e o parametro dele (lido aqui como "，") continuam antes
        // do losango; o numero de cabecalho da mensagem 20 tambem.
        assert!(message(&working, 0, 11).starts_with("[FF08]，◆Ｅｌｅｓ[FF01]ｃａｎｔａｍ．"));
        assert!(message(&working, 1, 0).starts_with("・◆Ｅｌａ[FF01]ｄｏｒｍｅ．"));
        let (_, text) = room_at(&working, 1);
        assert_eq!(
            working[text..text + 4],
            [0xFF, 0xFF, 0x01, 0x00],
            "cabecalho da sala"
        );
        assert_eq!(message(&working, 0, 10), message(&rom, 0, 10));
        assert_eq!(message(&working, 1, 19), message(&rom, 1, 19));
        assert_eq!(working[JIS16_BANK_PADDING], rom[JIS16_BANK_PADDING]);
    }

    #[test]
    fn without_free_space_the_block_is_left_alone_and_the_count_says_so() {
        let tmp = TempDir::new("bank-full");
        let (dir, rom) = project(&tmp, false);
        set_translations(&dir, &[(&synth::jis16_message(0), LONG)]);
        let err = reinsert_project_with(&dir, ErrorPolicy::Block)
            .unwrap_err()
            .to_string();
        assert!(err.contains("sem espaco"), "{err}");
        let outcome = reinsert_project_with(&dir, ErrorPolicy::Skip).unwrap();
        assert_eq!((outcome.apply.applied, outcome.apply.no_room), (0, 1));
        assert_eq!(fs::read(&outcome.working_path).unwrap(), rom);
    }

    #[tokio::test]
    async fn the_ai_is_told_the_width_of_the_box_instead_of_a_size_limit() {
        let tmp = TempDir::new("bank-ia");
        let (dir, _) = project(&tmp, true);
        let provider = FakeProvider {
            answers: HashMap::new(),
            requests: Mutex::new(Vec::new()),
        };
        let mut db = ProjectDb::open(&dir).unwrap();
        run_translation(
            &mut db,
            None,
            &provider,
            "fake-model",
            &TranslateOptions::new("pt-BR"),
            &AtomicBool::new(false),
            &|_| {},
        )
        .await
        .unwrap();
        let requests = provider.requests.lock().unwrap().clone();
        let item = requests
            .iter()
            .flat_map(|r| r.items.iter())
            .find(|i| i.text == synth::jis16_message(0))
            .expect("a primeira mensagem foi pedida");
        // Sem teto de tamanho: a largura da linha, no item, e a regra dela uma
        // vez so no prompt.
        assert_eq!((item.max_chars, item.max_line), (None, Some(18)));
        assert_eq!(item.context, None);
        let request = requests
            .iter()
            .find(|r| r.items.iter().any(|i| i.max_line.is_some()))
            .unwrap();
        let prompt = romtranslate_core::provider::build_system_prompt(request);
        assert_eq!(prompt.matches("maxLine").count(), 2, "{prompt}");
        assert!(serde_json::to_string(item)
            .unwrap()
            .contains("\"maxLine\":18"));
    }

    #[test]
    fn repeated_messages_are_stored_once_when_the_blocks_do_not_fit() {
        let tmp = TempDir::new("bank-pool");
        let rom = synth::make_gba_rom_jis16_bank_repeated();
        let source = tmp.0.join("game.gba");
        fs::write(&source, &rom).unwrap();
        let dir = tmp.0.join("game.rtsproj");
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
        // A mensagem 0 esta nas salas 0 e 3; a 45, nas salas 2 e 5. Sem
        // espaco livre no arquivo, bloco a bloco nenhuma das quatro cabe.
        set_translations(
            &dir,
            &[
                (&synth::jis16_message(0), LONG),
                (&synth::jis16_message(45), LONG),
            ],
        );
        let outcome = reinsert_project_with(&dir, ErrorPolicy::Block).unwrap();
        assert_eq!((outcome.apply.applied, outcome.apply.no_room), (4, 0));
        let working = fs::read(&outcome.working_path).unwrap();
        assert_eq!(working.len(), rom.len());

        // Lido pelo diretorio, cada sala tem as mensagens dela: as traduzidas
        // nas duas salas que as tem, e todas as outras como eram.
        let long =
            "◆Ｔｏｄｏ ｏ ｐｏｖｏ ａｑｕｉ[FF01]ｓｅｇｕｅ ｎｏ ｔｒａｂａｌｈｏ．[FF03][FF00]";
        for room in 0..6 {
            for k in 0..20 {
                let expected = match (room % 3, k) {
                    (0, 0) | (2, 5) => long.to_string(),
                    _ => message(&rom, room, k),
                };
                assert_eq!(
                    message(&working, room, k),
                    expected,
                    "sala {room}, mensagem {k}"
                );
            }
            // A tabela continua logo antes do texto, e o texto abre com o
            // cabecalho da propria sala.
            let (old_table, old_text) = room_at(&rom, room);
            let (table, text) = room_at(&working, room);
            assert_eq!(text - table, old_text - old_table, "sala {room}");
            assert_eq!(
                working[text..text + 4],
                rom[old_text..old_text + 4],
                "sala {room}"
            );
        }
        // As salas que repetem outra apontam pras mesmas mensagens dela.
        let offset = |room: usize, k: usize| {
            let (table, text) = room_at(&working, room);
            text + u16_at(&working, table + 2 * k)
        };
        assert_eq!(offset(0, 7), offset(3, 7));
        assert_eq!(offset(2, 5), offset(5, 5));
        // Nada fora do banco mudou.
        let (first, _) = room_at(&rom, 0);
        assert_eq!(working[..JIS16_BANK_AT], rom[..JIS16_BANK_AT]);
        let end = (0..6).map(|room| room_at(&rom, room).1).max().unwrap() + 0x1000;
        assert!(first > JIS16_BANK_AT);
        assert_eq!(working[end..], rom[end..]);
    }

    // --- Caixa de texto que rola: o app quebra as linhas sozinho ---

    fn scrolling_project(tmp: &TempDir) -> PathBuf {
        let source = tmp.0.join("game.gba");
        fs::write(&source, synth::make_gba_rom_jis16_bank_scrolling()).unwrap();
        let dir = tmp.0.join("game.rtsproj");
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
        dir
    }

    #[test]
    fn a_box_that_scrolls_is_measured_and_one_that_does_not_is_left_alone() {
        // Sem paragrafo mais alto que o comum nao ha prova de que a caixa
        // rola: nada de quebra automatica.
        let fixed = GbaAdapter
            .extract(&synth::make_gba_rom_jis16_bank(true))
            .unwrap();
        let first = by_text(&fixed.entries, &synth::jis16_message(0));
        assert!(first.metadata["bank"].get("break").is_none());

        let rom = synth::make_gba_rom_jis16_bank_scrolling();
        let out = GbaAdapter.extract(&rom).unwrap();
        let first = by_text(&out.entries, &synth::jis16_scrolling_message(0));
        let bank = &first.metadata["bank"];
        assert_eq!(
            (&bank["break"], &bank["indent"]),
            (&"[FF01]".into(), &true.into())
        );
        // A caixa: a maior linha, que e a de baixo com o espaco na frente.
        assert_eq!(bank["line"], 19);
        assert_eq!(bank["wide"], serde_json::json!({}));
        // O comando do meio da linha leva a folga que os autores deixaram: a
        // caixa menos a linha mais cheia que o tem ("◆まちの パンやは", 9).
        let named = by_text(&out.entries, &synth::jis16_scrolling_message(30));
        assert_eq!(
            named.metadata["bank"]["wide"],
            serde_json::json!({"[FF23": 10})
        );
        assert!(first.context.as_deref().unwrap().contains("quebra"));

        // A traducao nao precisa da quebra de linha do original, e a frase
        // inteira nao e aviso: quem quebra e o app. Palavra maior que a
        // caixa, sim.
        let table = romtranslate_core::tbl::TblTable::parse(&out.tables[0].1).unwrap();
        let whole = "◆O povo todo desta vila aqui trabalha bem contente.";
        let issues = validate_entry(&translated(first, whole), Some(&table));
        assert!(issues.is_empty(), "{issues:?}");
        let word = "◆Inconstitucionalissimamente!";
        let issues = validate_entry(&translated(first, word), Some(&table));
        assert!(
            issues.len() == 1 && issues[0].message.contains("vai passar da borda"),
            "{issues:?}"
        );
        // Os outros codigos continuam contando.
        let issues = validate_entry(&translated(named, whole), Some(&table));
        assert!(issues[0].message.contains("[FF23:0001]"), "{issues:?}");
    }

    #[test]
    fn the_translation_is_wrapped_to_the_box_with_the_break_of_the_game() {
        let tmp = TempDir::new("bank-wrap");
        let dir = scrolling_project(&tmp);
        set_translations(
            &dir,
            &[
                // Frase inteira, sem quebra nenhuma.
                (
                    &synth::jis16_scrolling_message(0),
                    "◆O povo todo desta vila segue no trabalho, sempre contente.",
                ),
                // Com uma quebra posta por quem traduziu e o comando que
                // escreve um nome: a quebra fica, o comando conta 10.
                (
                    &synth::jis16_scrolling_message(30),
                    "◆Os velhos[FF01] daqui e [FF23:0001] seguem trabalhando.",
                ),
            ],
        );
        let outcome = reinsert_project_with(&dir, ErrorPolicy::Block).unwrap();
        assert_eq!(outcome.apply.applied, 2);
        let working = fs::read(&outcome.working_path).unwrap();
        let lines = |room: usize, k: usize| -> Vec<String> {
            message(&working, room, k)
                .trim_end_matches("[FF03][FF00]")
                .split("[FF01]")
                .map(str::to_string)
                .collect()
        };
        assert_eq!(
            lines(0, 0),
            [
                "◆Ｏ ｐｏｖｏ ｔｏｄｏ ｄｅｓｔａ",
                " ｖｉｌａ ｓｅｇｕｅ ｎｏ",
                " ｔｒａｂａｌｈｏ， ｓｅｍｐｒｅ",
                " ｃｏｎｔｅｎｔｅ．",
            ]
        );
        assert!(lines(0, 0).iter().all(|line| line.chars().count() <= 19));
        // " daqui e" (8) com o nome (10) cabe em 19; " seguem" ja nao.
        assert_eq!(
            lines(1, 10),
            [
                "◆Ｏｓ ｖｅｌｈｏｓ",
                " ｄａｑｕｉ ｅ [FF23]、",
                " ｓｅｇｕｅｍ",
                " ｔｒａｂａｌｈａｎｄｏ．",
            ]
        );
    }

    #[tokio::test]
    async fn the_ai_gets_the_whole_sentence_where_the_app_breaks_the_lines() {
        let tmp = TempDir::new("bank-flow");
        let dir = scrolling_project(&tmp);
        let provider = FakeProvider {
            answers: HashMap::new(),
            requests: Mutex::new(Vec::new()),
        };
        let mut db = ProjectDb::open(&dir).unwrap();
        run_translation(
            &mut db,
            None,
            &provider,
            "fake-model",
            &TranslateOptions::new("pt-BR"),
            &AtomicBool::new(false),
            &|_| {},
        )
        .await
        .unwrap();
        let requests = provider.requests.lock().unwrap().clone();
        let items: Vec<_> = requests.iter().flat_map(|r| r.items.iter()).collect();
        // A quebra do meio da frase virou espaco, nas duas e nas tres linhas.
        let flowed = synth::jis16_scrolling_message(9).replace("[FF01] ", " ");
        let item = items
            .iter()
            .find(|i| i.text == flowed)
            .expect("a mensagem de tres linhas foi pedida inteira");
        assert!(!item.text.contains('['), "{}", item.text);
        assert_eq!((item.max_chars, item.max_line), (None, None));
        assert!(!items.iter().any(|i| i.text.contains("[FF01]")));
        let prompt = romtranslate_core::provider::build_system_prompt(&requests[0]);
        assert!(!prompt.contains("maxLine"), "{prompt}");
    }
}

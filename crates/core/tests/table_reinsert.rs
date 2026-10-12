//! Texto lido por tabela de caracteres (`.tbl`) volta pro jogo: busca
//! relativa -> tabela no projeto -> scan -> traducao -> validacao ->
//! reinsercao -> releitura da copia de trabalho com a mesma tabela.

use std::fs;
use std::path::{Path, PathBuf};

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::nes::NesAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::reinsert_project;
use romtranslate_core::relative::{build_tbl, save_tbl, search_project};
use romtranslate_core::scan::{scan_file, scan_project, ScanConfig, ScanEncoding};
use romtranslate_core::synth::{
    self, encode_custom, CUSTOM_LINES, CUSTOM_LOWER, CUSTOM_SPACE, CUSTOM_UPPER,
};
use romtranslate_core::types::{Platform, TextEncoding};
use romtranslate_core::validate::{
    apply_manual_translation, validate_project_db, IssueKind, Severity, ValidationIssue,
};

const ROM_NAME: &str = "game.nes";

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-tblre-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// Grava `rom` e cria um projeto de NES apontando pra ele.
    fn project(&self, rom: &[u8]) -> PathBuf {
        let source = self.0.join(ROM_NAME);
        fs::write(&source, rom).unwrap();
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
    fn scanned_project(&self, rom: &[u8]) -> (PathBuf, PathBuf) {
        let dir = self.project(rom);
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

/// Traduz, pelo caminho da edicao manual, a entry cujo original e `source`.
fn translate(dir: &Path, source: &str, translation: &str) -> Vec<ValidationIssue> {
    let mut db = ProjectDb::open(dir).unwrap();
    let id = db
        .load_entries()
        .unwrap()
        .into_iter()
        .find(|e| e.source_text == source)
        .unwrap_or_else(|| panic!("sem entry \"{source}\""))
        .id;
    apply_manual_translation(&mut db, None, &id, translation, "en-US", "pt-BR").unwrap()
}

/// (offset, texto) do que a tabela le em `path`.
fn read_back(path: &Path, tbl: &Path) -> Vec<(u64, String)> {
    scan_file(path, &table_config(tbl))
        .unwrap()
        .entries
        .into_iter()
        .map(|e| (e.offset.unwrap(), e.source_text))
        .collect()
}

fn working_copy(dir: &Path) -> PathBuf {
    dir.join("working").join(ROM_NAME)
}

#[test]
fn table_text_goes_back_into_the_game() {
    let tmp = TempDir::new("roundtrip");
    let rom = synth::make_custom_table_rom();
    let (dir, tbl) = tmp.scanned_project(&rom);
    assert_eq!(tbl, dir.join("tables").join("letras-80-A0.tbl"));

    // Menor, do mesmo tamanho e menor com minusculas.
    let translations = [
        (0, "START GAME", "NOVO JOGO"),
        (1, "PRESS START", "APERTE AQUI"),
        (
            5,
            "the village is quiet tonight",
            "a vila dorme em paz hoje",
        ),
    ];
    for (_, source, translation) in translations {
        let issues = translate(&dir, source, translation);
        assert!(issues.is_empty(), "{source}: {issues:?}");
    }

    let outcome = reinsert_project(&dir, false).unwrap();
    assert_eq!(outcome.apply.applied, 3);
    assert_eq!(outcome.apply.kept_original, CUSTOM_LINES.len() - 3);
    assert_eq!(outcome.apply.ignored_generic, 0);
    assert!(outcome.verification.ok, "{:?}", outcome.verification);
    assert_eq!(outcome.working_path, working_copy(&dir));

    // Relida com a mesma tabela, a copia de trabalho traz as traducoes nos
    // mesmos lugares e o resto como estava.
    let mut expected: Vec<(u64, String)> = CUSTOM_LINES
        .iter()
        .enumerate()
        .map(|(i, line)| (0x100 + i as u64 * 0x40, line.to_string()))
        .collect();
    for (i, _, translation) in translations {
        expected[i].1 = translation.to_string();
    }
    assert_eq!(read_back(&outcome.working_path, &tbl), expected);

    // Byte a byte: a sobra da string terminada vira terminador, e fora das
    // tres strings nada mudou.
    let working = fs::read(&outcome.working_path).unwrap();
    let mut patched = rom.clone();
    for (i, source, translation) in translations {
        let at = 0x100 + i * 0x40;
        patched[at..at + source.len()].fill(0);
        patched[at..at + translation.len()].copy_from_slice(&encode_custom(translation));
    }
    assert!(
        working == patched,
        "a copia de trabalho mudou fora das strings"
    );

    // O original nunca e tocado.
    assert_eq!(fs::read(tmp.0.join(ROM_NAME)).unwrap(), rom);
}

#[test]
fn character_outside_the_table_blocks_reinsertion_and_is_named() {
    let tmp = TempDir::new("char");
    let (dir, _) = tmp.scanned_project(&synth::make_custom_table_rom());

    // A tabela da busca relativa so tem letras sem acento e o espaco.
    let issues = translate(&dir, "START GAME", "JOGAR!");
    assert!(
        issues.iter().any(|i| i.kind == IssueKind::Unencodable
            && i.severity == Severity::Error
            && i.message.contains("'!'")
            && i.message.contains("posicao 6")),
        "{issues:?}"
    );
    let issues = translate(&dir, "CONTINUE", "AÇÃO");
    assert!(
        issues.iter().any(|i| i.message.contains("'Ç'")),
        "{issues:?}"
    );

    let blocked = reinsert_project(&dir, false).unwrap_err().to_string();
    assert!(blocked.contains("2 entries com erro"), "{blocked}");
    assert!(blocked.contains("'!'"), "{blocked}");

    // Nem o modo avancado grava byte inventado: e tudo ou nada.
    let forced = reinsert_project(&dir, true).unwrap_err().to_string();
    assert!(forced.contains("'!'"), "{forced}");
    assert!(!working_copy(&dir).exists());
}

#[test]
fn longer_translation_is_refused_asking_for_shorter_text() {
    let tmp = TempDir::new("overflow");
    let (dir, _) = tmp.scanned_project(&synth::make_custom_table_rom());

    // O validador mede em bytes da tabela e avisa antes.
    let issues = translate(&dir, "CONTINUE", "CONTINUAR JOGO");
    assert!(
        issues.iter().any(|i| i.kind == IssueKind::ByteOverflow
            && i.message.contains("14 bytes")
            && i.message.contains("tem 8")),
        "{issues:?}"
    );

    let err = reinsert_project(&dir, false).unwrap_err().to_string();
    assert!(err.contains("encurte"), "{err}");
    assert!(err.contains("14 bytes") && err.contains("tem 8"), "{err}");
    assert!(!working_copy(&dir).exists());
}

#[test]
fn missing_table_is_a_clear_error() {
    let tmp = TempDir::new("missing");
    let (dir, tbl) = tmp.scanned_project(&synth::make_custom_table_rom());
    assert!(translate(&dir, "START GAME", "NOVO JOGO").is_empty());
    fs::remove_file(&tbl).unwrap();

    let expect_clear = |err: String| {
        assert!(err.contains("\"letras-80-A0\""), "{err}");
        assert!(err.contains(&tbl.display().to_string()), "{err}");
    };
    expect_clear(reinsert_project(&dir, false).unwrap_err().to_string());
    expect_clear(reinsert_project(&dir, true).unwrap_err().to_string());
    let mut db = ProjectDb::open(&dir).unwrap();
    expect_clear(validate_project_db(&mut db).unwrap_err().to_string());
    assert!(!working_copy(&dir).exists());
}

/// O adapter nao conhece tabelas: chamado por fora da orquestracao, erra em
/// vez de pular a entry (era o que acontecia: `ignored_generic`).
#[test]
fn adapter_alone_never_skips_table_text() {
    let rom = synth::make_custom_table_rom();
    let tmp = TempDir::new("adapter");
    let (dir, _) = tmp.scanned_project(&rom);
    translate(&dir, "START GAME", "NOVO JOGO");
    let entries = ProjectDb::open(&dir).unwrap().load_entries().unwrap();

    let err = NesAdapter
        .apply_text(&rom, &entries)
        .unwrap_err()
        .to_string();
    assert!(err.contains("letras-80-A0"), "{err}");
}

#[test]
fn table_from_outside_is_copied_into_the_project() {
    let tmp = TempDir::new("adopt");
    let rom = synth::make_custom_table_rom();
    let dir = tmp.project(&rom);
    let text = build_tbl(&rom, Some(CUSTOM_UPPER), Some(CUSTOM_LOWER)).unwrap();
    let outside = tmp.0.join("downloads").join("minha.tbl");
    fs::create_dir_all(outside.parent().unwrap()).unwrap();
    fs::write(&outside, &text).unwrap();

    scan_into_db(&dir, &outside);
    let copy = dir.join("tables").join("minha.tbl");
    assert_eq!(fs::read_to_string(&copy).unwrap(), text);
    let entries = ProjectDb::open(&dir).unwrap().load_entries().unwrap();
    assert_eq!(entries[0].encoding, TextEncoding::Table("minha".into()));

    // O projeto nao depende mais do arquivo de fora.
    fs::remove_file(&outside).unwrap();
    assert!(translate(&dir, "CONTINUE", "SEGUIR").is_empty());
    let outcome = reinsert_project(&dir, false).unwrap();
    assert_eq!(read_back(&outcome.working_path, &copy)[2].1, "SEGUIR");

    // Escanear de novo pela copia do projeto nao copia nada; outra tabela de
    // mesmo nome nao apaga a do projeto (que pode ter sido completada a mao).
    scan_project(&dir, &table_config(&copy)).unwrap();
    fs::write(&outside, "80=A\n81=B\n").unwrap();
    let err = scan_project(&dir, &table_config(&outside)).unwrap_err();
    assert!(err.to_string().contains("outro conteudo"), "{err}");
    assert_eq!(fs::read_to_string(&copy).unwrap(), text);
}

#[test]
fn fixed_width_string_is_filled_with_the_table_space() {
    let tmp = TempDir::new("fixed");
    let mut rom = synth::make_custom_table_rom();
    // Depois de "START GAME" vem um byte que nao e terminador nem esta na
    // tabela: a string passa a ser de largura fixa.
    let end = 0x100 + CUSTOM_LINES[0].len();
    rom[end] = 0xFF;
    let (dir, _) = tmp.scanned_project(&rom);

    assert!(translate(&dir, "START GAME", "NOVO JOGO").is_empty());
    let outcome = reinsert_project(&dir, false).unwrap();
    let working = fs::read(&outcome.working_path).unwrap();
    // Sobra com o espaco DA TABELA (0x40) — nao 0x20, nem 0x00.
    assert_eq!(working[0x100..end], encode_custom("NOVO JOGO "));
    assert_eq!(working[end - 1], CUSTOM_SPACE);
    assert_eq!(working[end], 0xFF);
}

#[test]
fn fixed_width_without_a_table_space_needs_the_exact_size() {
    let tmp = TempDir::new("nospace");
    let rom = synth::make_custom_table_rom();
    let dir = tmp.project(&rom);
    // So letras: o 0x40 entre as palavras deixa de ser texto, e "START"
    // vira uma string de largura fixa (o byte seguinte nao e 0x00).
    let letters: String = build_tbl(&rom, Some(CUSTOM_UPPER), Some(CUSTOM_LOWER))
        .unwrap()
        .lines()
        .filter(|line| !line.ends_with("= "))
        .map(|line| format!("{line}\n"))
        .collect();
    let outside = tmp.0.join("letras.tbl");
    fs::write(&outside, letters).unwrap();
    scan_into_db(&dir, &outside);
    let tbl = dir.join("tables").join("letras.tbl");

    // Menor que o espaco e sem com que preencher: erro ja na validacao.
    let issues = translate(&dir, "START", "JOGO");
    assert!(
        issues.iter().any(|i| i.severity == Severity::Error
            && i.kind == IssueKind::ByteOverflow
            && i.message.contains("exatamente 5")),
        "{issues:?}"
    );
    let err = reinsert_project(&dir, true).unwrap_err().to_string();
    assert!(err.contains("exatamente 5"), "{err}");
    assert!(!working_copy(&dir).exists());

    // Do tamanho exato, entra.
    assert!(translate(&dir, "START", "JOGAR").is_empty());
    let outcome = reinsert_project(&dir, false).unwrap();
    assert_eq!(
        read_back(&outcome.working_path, &tbl)[0],
        (0x100, "JOGAR".to_string())
    );
}

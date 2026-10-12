//! Descoberta de texto fora do alcance do scanner ASCII: busca relativa (jogo
//! com tabela de caracteres propria) e raio-X (texto comprimido, e quanto do
//! que ja foi extraido pode crescer).

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::relative::{build_tbl, relative_search, save_tbl, search_project};
use romtranslate_core::scan::{scan_bytes, ScanConfig, ScanEncoding};
use romtranslate_core::synth::{self, CUSTOM_LINES, CUSTOM_LOWER, CUSTOM_UPPER, LZ77_SCRIPT};
use romtranslate_core::types::Platform;
use romtranslate_core::xray::{xray, xray_project};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-discovery-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// Grava `rom` e cria um projeto apontando pra ele.
    fn project(&self, name: &str, rom: &[u8], platform: Platform, adapter_id: &str) -> PathBuf {
        let source = self.0.join(name);
        fs::write(&source, rom).unwrap();
        let dir = self.0.join("game.rtsproj");
        create_project(CreateProjectArgs {
            source_path: source,
            platform,
            adapter_id: adapter_id.to_string(),
            source_language: None,
            target_language: "pt-BR".to_string(),
            project_dir: dir.clone(),
        })
        .unwrap();
        dir
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ascii_texts(rom: &[u8]) -> Vec<String> {
    scan_bytes(rom, &ScanConfig::default())
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.source_text)
        .collect()
}

// ---- busca relativa ----

#[test]
fn ascii_scanner_is_blind_to_a_custom_table() {
    assert!(ascii_texts(&synth::make_custom_table_rom()).is_empty());
}

#[test]
fn one_known_word_reveals_where_the_alphabet_starts() {
    let rom = synth::make_custom_table_rom();

    let upper = &relative_search(&rom, "START").unwrap()[0];
    assert_eq!(upper.upper_base, Some(CUSTOM_UPPER));
    assert_eq!(upper.lower_base, None);
    // "START GAME" e "PRESS START"; "Start" tem outra relacao entre as letras.
    assert_eq!(upper.offsets, vec![0x100, 0x140 + 6]);
    assert_eq!(upper.hits, 2);
    assert!(!upper.ascii);

    // Palavra com os dois casos entrega as duas bases de uma vez, e o trecho
    // de conferencia ja vem legivel, com o espaco inferido.
    let both = &relative_search(&rom, "Press").unwrap()[0];
    assert_eq!(both.upper_base, Some(CUSTOM_UPPER));
    assert_eq!(both.lower_base, Some(CUSTOM_LOWER));
    assert!(
        both.preview.contains("Press Start to begin your quest"),
        "{}",
        both.preview
    );
}

#[test]
fn generated_table_makes_the_scanner_read_the_whole_game() {
    let tmp = TempDir::new("tbl");
    let rom = synth::make_custom_table_rom();
    let tbl_path = tmp.0.join("game.tbl");
    fs::write(
        &tbl_path,
        build_tbl(&rom, Some(CUSTOM_UPPER), Some(CUSTOM_LOWER)).unwrap(),
    )
    .unwrap();

    let outcome = scan_bytes(
        &rom,
        &ScanConfig {
            encoding: ScanEncoding::Table,
            tbl_path: Some(tbl_path),
            ..ScanConfig::default()
        },
    )
    .unwrap();
    let texts: Vec<&str> = outcome
        .entries
        .iter()
        .map(|e| e.source_text.as_str())
        .collect();
    assert_eq!(texts, CUSTOM_LINES);
    assert_eq!(outcome.entries[0].offset, Some(0x100));
    assert!(outcome
        .entries
        .iter()
        .all(|e| e.metadata["terminated"] == true));
}

#[test]
fn words_that_would_match_garbage_are_refused() {
    let rom = synth::make_custom_table_rom();
    for word in ["AB", "AAAA", "ABAB", "ST4RT", "NEW GAME", "olá!"] {
        assert!(relative_search(&rom, word).is_err(), "{word}");
    }
    assert!(build_tbl(&rom, None, None).is_err());
    assert!(build_tbl(&rom, Some(0xF0), None).is_err()); // 'Z' nao cabe num byte
    assert!(build_tbl(&rom, Some(0x80), Some(0x90)).is_err()); // casos sobrepostos
}

#[test]
fn ascii_game_is_reported_as_ascii() {
    let hit = &relative_search(&synth::make_nes_rom(), "GAME").unwrap()[0];
    assert_eq!(hit.upper_base, Some(b'A'));
    assert!(hit.ascii);
    // A relacao entre as letras nao distingue caso: a mesma palavra em
    // minusculas acha o mesmo texto, e continua sendo ASCII.
    assert!(relative_search(&synth::make_nes_rom(), "game").unwrap()[0].ascii);
}

#[test]
fn noise_and_tiny_inputs_never_panic_or_invent_a_table() {
    assert!(relative_search(&synth::make_random(1 << 20, 5), "START")
        .unwrap()
        .is_empty());
    assert!(relative_search(&[], "START").unwrap().is_empty());
    assert!(relative_search(&[0x92, 0x93], "START").unwrap().is_empty());
    assert!(relative_search(&[0xFF; 64], "START").unwrap().is_empty());
}

#[test]
fn project_flow_saves_the_table_and_keeps_hand_edits() {
    let tmp = TempDir::new("flow");
    let dir = tmp.project(
        "game.nes",
        &synth::make_custom_table_rom(),
        Platform::Nes,
        "nes.ines",
    );

    assert_eq!(
        search_project(&dir, "CONTINUE").unwrap()[0].upper_base,
        Some(CUSTOM_UPPER)
    );
    let path = save_tbl(&dir, Some(CUSTOM_UPPER), Some(CUSTOM_LOWER)).unwrap();
    assert_eq!(path, dir.join("tables").join("letras-80-A0.tbl"));
    assert!(fs::read_to_string(&path).unwrap().contains("80=A\n"));

    // O usuario completa a pontuacao a mao; salvar de novo nao pode apagar.
    fs::write(&path, "80=A\n5B=!\n").unwrap();
    assert_eq!(
        save_tbl(&dir, Some(CUSTOM_UPPER), Some(CUSTOM_LOWER)).unwrap(),
        path
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "80=A\n5B=!\n");
}

// ---- raio-x ----

#[test]
fn xray_reports_the_compressed_script_no_scanner_sees() {
    let (rom, at) = synth::make_gba_rom_with_lz77();
    // Comprimido, o roteiro vira pedacos: nenhuma fala sai inteira no scanner.
    let scanned = ascii_texts(&rom);
    assert!(LZ77_SCRIPT
        .iter()
        .all(|line| !scanned.iter().any(|s| s == line)));

    let report = xray(&rom, &[]);
    assert_eq!(report.size, 64 * 1024);
    assert_eq!(report.lz77.blocks, 1);
    assert_eq!(report.lz77.blocks_with_text, 1);
    assert_eq!(report.lz77.strings, LZ77_SCRIPT.len());
    assert!(!report.lz77.truncated);
    let block = &report.lz77.first[0];
    assert_eq!(block.offset, at as u64);
    let script_len: usize = LZ77_SCRIPT.iter().map(|l| l.len() + 1).sum();
    assert_eq!(block.unpacked_len, script_len as u64);
    assert!(block.packed_len < block.unpacked_len);

    // Os 16 KiB de ruido sao o unico trecho denso; o resto e quase so zero.
    assert_eq!(report.dense_bytes, 0x4000);
    assert!(report.blank_bytes >= 0x8000);
}

#[test]
fn xray_tells_how_many_strings_can_grow() {
    let rom = synth::make_gba_rom_with_pointers();
    let entries = GbaAdapter.extract_structured(&rom).unwrap();
    let report = xray(&rom, &entries);
    assert_eq!(report.entries, entries.len());
    // NEW GAME, CONTINUE e OPTIONS estao em tabela; GAME OVER so tem um
    // ponteiro solto, que o app nao reaponta.
    assert_eq!(report.relocatable, 3);
    assert_eq!(report.lz77.blocks, 0);
}

#[test]
fn xray_finds_nothing_compressed_where_nothing_is() {
    let plain = [
        synth::make_gba_rom("PLAIN"),
        synth::make_nes_rom(),
        synth::make_snes_lorom("PLAIN"),
        synth::make_nds_rom(),
        synth::make_custom_table_rom(),
        synth::make_random(1 << 20, 11),
        Vec::new(),
        vec![0x10; 4096],
    ];
    for rom in &plain {
        let report = xray(rom, &[]);
        assert_eq!(report.lz77.blocks, 0, "{} bytes", rom.len());
    }
    assert_eq!(xray(&[], &[]).dense_bytes, 0);
}

#[test]
fn xray_of_a_project_crosses_the_file_with_its_entries() {
    let tmp = TempDir::new("xray");
    let rom = synth::make_gba_rom_with_pointers();
    let dir = tmp.project("game.gba", &rom, Platform::Gba, GbaAdapter.id());
    let entries = GbaAdapter.extract_structured(&rom).unwrap();
    ProjectDb::open(&dir)
        .unwrap()
        .upsert_entries(&entries)
        .unwrap();

    assert_eq!(xray_project(&dir).unwrap(), xray(&rom, &entries));
}

//! Texto comprimido (LZ77 tipo 0x10) no GBA: as frases de dentro do bloco sao
//! extraidas, e a traducao volta com o bloco recomprimido — no lugar se
//! couber, no fim do ROM se o bloco tiver ponteiro em tabela, e fora (com a
//! contagem dita) se nao tiver nenhum dos dois.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::extract_project;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::lz77;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::{reinsert_project, reinsert_project_with, ErrorPolicy};
use romtranslate_core::scan::{scan_bytes, ScanConfig};
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, TextEntry, TranslationStatus};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-packed-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
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

/// Cria o projeto, extrai e grava as traducoes de `pairs` no banco.
fn project(tmp: &TempDir, rom: &[u8], pairs: &[(&str, &str)]) -> PathBuf {
    let source = tmp.0.join("game.gba");
    fs::write(&source, rom).unwrap();
    let dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: source,
        platform: Platform::Gba,
        adapter_id: GbaAdapter.id().to_string(),
        source_language: None,
        target_language: "pt-BR".to_string(),
        project_dir: dir.clone(),
    })
    .unwrap();
    extract_project(&dir).unwrap();
    let mut db = ProjectDb::open(&dir).unwrap();
    let mut entries = db.load_entries().unwrap();
    for e in entries.iter_mut() {
        if let Some((source, translation)) = pairs.iter().find(|(s, _)| *s == e.source_text) {
            assert!(translation.len() <= source.len(), "{translation}");
            e.translated_text = Some(translation.to_string());
            e.status = TranslationStatus::Machine;
        }
    }
    db.upsert_entries(&entries).unwrap();
    dir
}

/// As falas (separadas por 0x00) do bloco que comeca em `at`.
fn lines_at(rom: &[u8], at: usize) -> Vec<String> {
    lz77::unpack(&rom[at..])
        .expect("bloco valido")
        .data
        .split(|&b| b == 0)
        .filter(|line| !line.is_empty())
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect()
}

fn pointer_at(rom: &[u8], at: usize) -> usize {
    (u32::from_le_bytes(rom[at..at + 4].try_into().unwrap()) - 0x0800_0000) as usize
}

const GROWING: [(&str, &str); 3] = [
    (
        synth::LZ77_TABLE_LINES[0],
        "Que bom ver gente nova por aqui!",
    ),
    (
        synth::LZ77_TABLE_LINES[1],
        "Faz tempo que ninguem vem a vila.",
    ),
    (
        synth::LZ77_TABLE_LINES[2],
        "Durma na estalagem e siga cedo.",
    ),
];

const LOOSE_GROWING: [(&str, &str); 3] = [
    (synth::LZ77_LOOSE_LINES[0], "O muro some na neblina do rio."),
    (
        synth::LZ77_LOOSE_LINES[1],
        "Leve agua, pao e um casaco bom.",
    ),
    (synth::LZ77_LOOSE_LINES[2], "Quem bate o sino paga a festa."),
];

#[test]
fn compressed_lines_are_extracted_and_the_raw_fragments_are_not() {
    let (rom, block) = synth::make_gba_rom_with_lz77();
    let packed_len = lz77::unpack(&rom[block..]).unwrap().packed_len;
    let inside = |e: &TextEntry| {
        e.resource_path.is_none()
            && e.offset
                .is_some_and(|o| (block..block + packed_len).contains(&(o as usize)))
    };

    // O scanner cru le os literais do stream como se fossem texto do jogo.
    let raw = scan_bytes(&rom, &ScanConfig::default()).unwrap().entries;
    assert!(raw.iter().any(inside), "o teste precisa de fragmento cru");

    let out = GbaAdapter.extract(&rom).unwrap();
    assert!(!out.entries.iter().any(inside));
    assert!(out.discarded > 0);
    for line in synth::LZ77_SCRIPT {
        let e = by_text(&out.entries, line);
        assert!(e.id.starts_with("lz-00001000-"), "{}", e.id);
        assert_eq!(e.resource_path.as_deref(), Some("lz77:00001000"));
        assert_eq!(e.max_bytes, Some(line.len()));
        assert_eq!(e.metadata["lz77"]["block"], block);
        assert_eq!(e.metadata["lz77"]["packedLen"], packed_len);
    }
    assert!(GbaAdapter.capabilities().compression);
}

#[test]
fn a_block_of_tiles_with_one_phrase_is_not_text() {
    let rom = synth::make_gba_rom_with_lz77_table();
    let entries = GbaAdapter.extract_structured(&rom).unwrap();
    assert!(!entries
        .iter()
        .any(|e| e.source_text.contains(synth::LZ77_TILE_PHRASE)));
    // Os de texto saem, com ou sem ponteiro pro bloco.
    let tabled = by_text(&entries, synth::LZ77_TABLE_LINES[0]);
    assert_eq!(
        tabled.metadata["lz77"]["pointers"],
        serde_json::json!([0x800])
    );
    let loose = by_text(&entries, synth::LZ77_LOOSE_LINES[0]);
    assert!(loose.metadata["lz77"].get("pointers").is_none());
    // O ponteiro e do bloco, nao da frase: ela continua presa ao espaco dela.
    assert!(tabled.pointers().is_empty());
    assert_eq!(tabled.max_bytes, Some(synth::LZ77_TABLE_LINES[0].len()));
}

#[test]
fn shorter_translation_goes_back_into_the_block_in_place() {
    let tmp = TempDir::new("inplace");
    let (rom, block) = synth::make_gba_rom_with_lz77();
    let pairs = [
        (synth::LZ77_SCRIPT[1], "A ponte do norte caiu."),
        (synth::LZ77_SCRIPT[2], "Volte com a chave de prata."),
    ];
    let dir = project(&tmp, &rom, &pairs);

    let outcome = reinsert_project(&dir, false).unwrap();
    assert_eq!(outcome.apply.applied, 2);
    assert_eq!(outcome.apply.relocated, 0);
    assert_eq!(outcome.apply.packed_left_out, 0);
    assert!(outcome.verification.ok, "{:?}", outcome.verification);

    let working = fs::read(&outcome.working_path).unwrap();
    assert_eq!(working.len(), rom.len());
    assert_eq!(
        lines_at(&working, block),
        [
            synth::LZ77_SCRIPT[0],
            pairs[0].1,
            pairs[1].1,
            synth::LZ77_SCRIPT[3]
        ]
    );
    // O tamanho descomprimido e o mesmo, e nada muda fora do bloco.
    let before = lz77::unpack(&rom[block..]).unwrap();
    let after = lz77::unpack(&working[block..]).unwrap();
    assert_eq!(after.data.len(), before.data.len());
    assert!(after.packed_len <= before.packed_len);
    let end = block + before.packed_len;
    assert_eq!(working[..block], rom[..block]);
    assert_eq!(working[end..], rom[end..]);

    // A copia de trabalho re-extrai com as traducoes no lugar das originais.
    let again = GbaAdapter.extract_structured(&working).unwrap();
    assert!(by_text(&again, pairs[0].1).id.starts_with("lz-"));
}

#[test]
fn a_block_that_grows_moves_when_a_table_points_to_it() {
    let tmp = TempDir::new("table");
    let rom = synth::make_gba_rom_with_lz77_table();
    let dir = project(&tmp, &rom, &GROWING);

    let outcome = reinsert_project(&dir, false).unwrap();
    assert_eq!(outcome.apply.applied, 3);
    assert_eq!(outcome.apply.relocated, 3, "o bloco tinha que crescer");
    assert_eq!(outcome.apply.packed_left_out, 0);

    let working = fs::read(&outcome.working_path).unwrap();
    let moved = pointer_at(&working, 0x800);
    assert_eq!(moved, rom.len(), "anexado no fim do ROM, alinhado");
    assert_eq!(lines_at(&working, moved), GROWING.map(|(_, t)| t));
    // O outro alvo da tabela nao muda, e o bloco antigo fica intacto.
    assert_eq!(pointer_at(&working, 0x804), 0x2000);
    assert_eq!(working[0x1000..0x2000], rom[0x1000..0x2000]);
    assert_eq!(lines_at(&working, 0x2000), synth::LZ77_TABLE_OTHER);
}

#[test]
fn a_block_that_grows_without_a_table_is_left_alone_and_counted() {
    let tmp = TempDir::new("loose");
    let rom = synth::make_gba_rom_with_lz77_table();
    let dir = project(&tmp, &rom, &LOOSE_GROWING);

    // Politica padrao: um bloco que nao cabe bloqueia, dizendo o motivo.
    let err = reinsert_project(&dir, false).unwrap_err().to_string();
    assert!(err.contains("blocos comprimidos"), "{err}");

    // Pulando: o bloco fica como no original e a contagem diz quantas ficaram.
    let outcome = reinsert_project_with(&dir, ErrorPolicy::Skip).unwrap();
    assert_eq!(outcome.apply.packed_left_out, 3);
    assert_eq!(outcome.apply.applied, 0);
    let working = fs::read(&outcome.working_path).unwrap();
    assert_eq!(working, rom);
}

#[test]
fn old_translation_of_a_raw_fragment_inside_a_block_loses_to_the_block() {
    // Projeto extraido antes do texto comprimido: o scanner cru deixou uma
    // entry dentro do stream, e ela tem traducao.
    let (rom, block) = synth::make_gba_rom_with_lz77();
    let mut fragment = scan_bytes(&rom, &ScanConfig::default())
        .unwrap()
        .entries
        .into_iter()
        .find(|e| {
            e.offset
                .is_some_and(|o| o as usize > block && (o as usize) < block + 64)
        })
        .expect("fragmento cru dentro do bloco");
    fragment.translated_text = Some("X".repeat(fragment.original_bytes.len()));

    let mut entries = GbaAdapter.extract_structured(&rom).unwrap();
    for e in entries.iter_mut() {
        if e.source_text == synth::LZ77_SCRIPT[2] {
            e.translated_text = Some("Volte com a chave de prata.".to_string());
        }
    }
    entries.push(fragment);

    let applied = GbaAdapter.apply_text(&rom, &entries).unwrap();
    assert_eq!(applied.report.applied, 1);
    assert_eq!(applied.report.ignored_generic, 1);
    assert_eq!(
        lines_at(&applied.bytes, block)[2],
        "Volte com a chave de prata."
    );
}

#[test]
fn a_changed_block_is_refused_instead_of_patched_blind() {
    let (rom, block) = synth::make_gba_rom_with_lz77();
    let mut entries = GbaAdapter.extract_structured(&rom).unwrap();
    for e in entries.iter_mut() {
        if e.source_text == synth::LZ77_SCRIPT[2] {
            e.translated_text = Some("Volte depois.".to_string());
        }
    }
    let mut other = rom.clone();
    other[block] = 0x00; // nao e mais um bloco
    let err = GbaAdapter.apply_text(&other, &entries).unwrap_err();
    assert!(err.to_string().contains("Re-extraia"), "{err}");
}

//! Relocacao de ponteiros no GBA: deteccao so de TABELAS (a spec proibe
//! busca/troca global de bytes), traducao maior realocada pro fim do ROM com
//! a tabela reapontada, e recusa clara quando a string nao tem tabela.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::patch::{apply_bps, apply_ips, export_patch};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::reinsert_project;
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, TextEntry, TranslationStatus};
use romtranslate_core::validate::{validate_entry, IssueKind, Severity};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-reloc-{label}-{nanos}"));
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

fn translate(entries: &mut [TextEntry], pairs: &[(&str, &str)]) {
    for e in entries.iter_mut() {
        if let Some((_, t)) = pairs.iter().find(|(s, _)| *s == e.source_text) {
            e.translated_text = Some(t.to_string());
            e.status = TranslationStatus::Machine;
        }
    }
}

/// Offset no arquivo pra onde o ponteiro em `at` aponta.
fn target_of(rom: &[u8], at: usize) -> usize {
    (u32::from_le_bytes(rom[at..at + 4].try_into().unwrap()) - 0x0800_0000) as usize
}

#[test]
fn detects_tables_but_not_isolated_pointers() {
    let rom = synth::make_gba_rom_with_pointers();
    let entries = GbaAdapter.extract_structured(&rom).unwrap();

    assert_eq!(by_text(&entries, "NEW GAME").pointer_offsets(), vec![0x800]);
    let mut cont = by_text(&entries, "CONTINUE").pointer_offsets();
    cont.sort();
    assert_eq!(cont, vec![0x804, 0x880], "referenciada por duas tabelas");

    // Ponteiro isolado (literal pool) nao e tabela: fica so in-place.
    let over = by_text(&entries, "GAME OVER");
    assert!(over.pointer_offsets().is_empty());
    assert_eq!(over.max_bytes, Some(9));

    // Realocavel nao tem teto de bytes — a IA traduz sem espremer o texto.
    assert_eq!(by_text(&entries, "NEW GAME").max_bytes, None);
    assert!(by_text(&entries, "POTION").pointer_offsets().is_empty());
}

#[test]
fn longer_translation_is_relocated_and_tables_repointed() {
    let rom = synth::make_gba_rom_with_pointers();
    let mut entries = GbaAdapter.extract_structured(&rom).unwrap();
    translate(
        &mut entries,
        &[
            ("NEW GAME", "NOVO JOGO"), // 9 > 8: realoca
            ("CONTINUE", "CONTINUAR"), // 9 > 8: realoca, 2 ponteiros
            ("OPTIONS", "OPCOES"),     // 6 <= 7: cabe, in-place
        ],
    );
    let applied = GbaAdapter.apply_text(&rom, &entries).unwrap();
    let out = &applied.bytes;
    assert_eq!(applied.report.applied, 3);
    assert_eq!(applied.report.relocated, 2);
    assert!(
        out.len() > rom.len(),
        "o ROM cresce pra acomodar o texto novo"
    );

    let new_game = target_of(out, 0x800);
    assert!(new_game >= rom.len(), "aponta pra area anexada no fim");
    assert_eq!(new_game % 4, 0, "string realocada alinhada em 4");
    assert_eq!(&out[new_game..new_game + 10], b"NOVO JOGO\0");

    // As duas tabelas que apontavam pra CONTINUE foram reapontadas juntas.
    let cont = target_of(out, 0x804);
    assert_eq!(target_of(out, 0x880), cont);
    assert_eq!(&out[cont..cont + 10], b"CONTINUAR\0");

    // OPTIONS coube: ponteiros intactos, texto trocado no lugar.
    assert_eq!(target_of(out, 0x808), 0x420);
    assert_eq!(target_of(out, 0x884), 0x420);
    assert_eq!(&out[0x420..0x427], b"OPCOES\0");

    // Original das realocadas fica intacto: referencia nao detectada ve o
    // texto antigo em vez de lixo.
    assert_eq!(&out[0x400..0x408], b"NEW GAME");

    let verification = GbaAdapter.verify(out).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);
    let reread = GbaAdapter.extract_structured(out).unwrap();
    assert!(reread.iter().any(|e| e.source_text == "NOVO JOGO"));
}

#[test]
fn overflow_without_table_is_refused_with_guidance() {
    let rom = synth::make_gba_rom_with_pointers();
    let mut entries = GbaAdapter.extract_structured(&rom).unwrap();
    translate(&mut entries, &[("GAME OVER", "FIM DE JOGO")]); // 11 > 9, ponteiro isolado
    let err = GbaAdapter
        .apply_text(&rom, &entries)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("encurte") && err.contains("realocada"),
        "{err}"
    );
}

#[test]
fn pointer_changed_since_extraction_is_refused() {
    let rom = synth::make_gba_rom_with_pointers();
    let mut entries = GbaAdapter.extract_structured(&rom).unwrap();
    translate(&mut entries, &[("NEW GAME", "NOVO JOGO")]);
    let mut drifted = rom.clone();
    drifted[0x800..0x804].copy_from_slice(&0x0800_0410u32.to_le_bytes());
    let err = GbaAdapter
        .apply_text(&drifted, &entries)
        .unwrap_err()
        .to_string();
    assert!(err.contains("0x800"), "{err}");
}

#[test]
fn validation_warns_on_relocatable_but_blocks_fixed_overflow() {
    let rom = synth::make_gba_rom_with_pointers();
    let mut entries = GbaAdapter.extract_structured(&rom).unwrap();
    translate(
        &mut entries,
        &[("NEW GAME", "NOVO JOGO"), ("GAME OVER", "FIM DE JOGO")],
    );

    let relocatable = validate_entry(by_text(&entries, "NEW GAME"));
    assert!(
        relocatable.iter().all(|i| i.severity == Severity::Warning),
        "{relocatable:?}"
    );
    assert!(relocatable
        .iter()
        .any(|i| i.kind == IssueKind::ByteOverflow && i.message.contains("realocada")));

    let fixed = validate_entry(by_text(&entries, "GAME OVER"));
    assert!(fixed
        .iter()
        .any(|i| i.kind == IssueKind::ByteOverflow && i.severity == Severity::Error));
}

#[test]
fn full_project_flow_relocates_and_patch_reproduces_working_copy() {
    let tmp = TempDir::new("flow");
    let rom = synth::make_gba_rom_with_pointers();
    let rom_path = tmp.0.join("ptr.gba");
    fs::write(&rom_path, &rom).unwrap();
    let project_dir = tmp.0.join("ptr.rtsproj");
    create_project(CreateProjectArgs {
        source_path: rom_path.clone(),
        platform: Platform::Gba,
        adapter_id: "gba.generic".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();

    let mut entries = GbaAdapter.extract_structured(&rom).unwrap();
    translate(
        &mut entries,
        &[("NEW GAME", "NOVO JOGO"), ("CONTINUE", "CONTINUAR")],
    );
    ProjectDb::open(&project_dir)
        .unwrap()
        .upsert_entries(&entries)
        .unwrap();

    // Validacao fresca so avisa (nao bloqueia) e a reinsercao reloca.
    let outcome = reinsert_project(&project_dir, false).unwrap();
    assert_eq!(outcome.apply.relocated, 2);
    assert!(outcome.verification.ok);
    assert_eq!(fs::read(&rom_path).unwrap(), rom, "original intacto");

    // Patch de um ROM que cresceu reproduz a working copy exata.
    let working = fs::read(&outcome.working_path).unwrap();
    let exported = export_patch(&project_dir, None).unwrap();
    let patch = fs::read(&exported.patch_path).unwrap();
    let rebuilt = match exported.patch_format {
        "IPS" => apply_ips(&rom, &patch).unwrap(),
        "BPS" => apply_bps(&rom, &patch).unwrap(),
        other => panic!("formato inesperado: {other}"),
    };
    assert_eq!(rebuilt, working);
}

//! Perfil do jogo: o banco de frases que nenhuma regra automatica le — letras
//! numa tabela propria, cabecalho e terminador, ponteiro com o endereco de
//! memoria da frase — descrito num `perfil.toml`, lido e remontado pelo app.

use std::fs;
use std::path::{Path, PathBuf};

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::extract_project;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::{reinsert_project, reinsert_project_with, ErrorPolicy};
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, TextEntry, TranslationStatus};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-perfil-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Projeto do banco sintetico, com perfil e tabela, extraido.
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
    fs::write(dir.join("tables/letras.tbl"), synth::profile_table()).unwrap();
    fs::write(dir.join("perfil.toml"), synth::profile_toml()).unwrap();
    extract_project(&dir).unwrap();
    dir
}

fn translate(dir: &Path, pairs: &[(&str, &str)]) {
    let mut db = ProjectDb::open(dir).unwrap();
    let mut entries = db.load_entries().unwrap();
    for e in entries.iter_mut() {
        if let Some((_, t)) = pairs.iter().find(|(s, _)| *s == e.source_text) {
            e.translated_text = Some(t.to_string());
            e.status = TranslationStatus::Machine;
        }
    }
    db.upsert_entries(&entries).unwrap();
}

/// O texto da frase pra onde o ponteiro `slot` da tabela leva.
fn read(rom: &[u8], slot: usize) -> String {
    let k = synth::PROFILE_TABLE + 4 * slot;
    let value = u32::from_le_bytes(rom[k..k + 4].try_into().unwrap());
    let at = (value - synth::PROFILE_MEMORY) as usize + synth::PROFILE_ORIGIN;
    assert_eq!(
        rom[at..at + 2],
        [1, 0],
        "cabecalho no destino do ponteiro {slot}"
    );
    let end = at + 2 + rom[at + 2..].iter().position(|&b| b == 0xEA).unwrap();
    let mut out = String::new();
    let mut i = at + 2;
    while i < end {
        match rom[i] {
            b @ 0..=0x5E => out.push((b + 0x20) as char),
            0xE3 => {
                out.push_str(&format!("[E3:{:02X}]", rom[i + 1]));
                i += 1;
            }
            b => out.push_str(&format!("[{b:02X}]")),
        }
        i += 1;
    }
    out
}

fn pointer(rom: &[u8], slot: usize) -> u32 {
    let k = synth::PROFILE_TABLE + 4 * slot;
    u32::from_le_bytes(rom[k..k + 4].try_into().unwrap())
}

fn entries(dir: &Path) -> Vec<TextEntry> {
    ProjectDb::open(dir).unwrap().load_entries().unwrap()
}

#[test]
fn the_profile_reads_the_bank_through_the_pointers() {
    let tmp = TempDir::new("ler");
    let dir = project(&tmp);
    let entries = entries(&dir);

    // As frases de verdade; o dado que abre com o mesmo cabecalho fica de fora.
    let mut texts: Vec<&str> = entries.iter().map(|e| e.source_text.as_str()).collect();
    texts.sort();
    let mut want = synth::PROFILE_LINES.to_vec();
    want.sort();
    assert_eq!(texts, want);

    // O codigo com parametro e um so, e a linha dele entrou na tabela.
    let table = fs::read_to_string(dir.join("tables/letras.tbl")).unwrap();
    assert!(table.contains("E310=[E3:10]"), "{table}");
    assert!(table.contains("E6=[E6]"));

    // A frase com quebra de linha ganha a largura dela; as curtas, a sobra
    // que a repetida libera.
    let speech = entries
        .iter()
        .find(|e| e.source_text.contains("castle"))
        .unwrap();
    assert_eq!(speech.metadata["perfil"]["linha"], 28);
    let bat = entries.iter().find(|e| e.source_text == "Bat").unwrap();
    assert!(bat.max_bytes.unwrap() > 5, "{:?}", bat.max_bytes);
    assert!(entries.iter().all(|e| e.can_grow()));

    // Extrair de novo nao duplica a tabela gerada.
    extract_project(&dir).unwrap();
    let again = fs::read_to_string(dir.join("tables/letras.tbl")).unwrap();
    assert_eq!(again, table);
}

#[test]
fn translations_grow_and_shrink_inside_the_bank_and_every_pointer_follows() {
    let tmp = TempDir::new("gravar");
    let dir = project(&tmp);
    translate(
        &dir,
        &[
            ("Bat", "Morcego"),
            (
                "[E3:10]The castle is quiet tonight.[E6]Too quiet.",
                "[E3:10]O castelo esta calmo hoje.[E6]Demais.",
            ),
            ("Restores 100 HP.", "Cura 100 HP."),
            ("Skeleton", "Esqueleto"),
        ],
    );
    let outcome = reinsert_project(&dir, false).unwrap();
    assert_eq!(outcome.apply.no_room, 0);
    assert!(outcome.verification.ok);

    let rom = fs::read(&outcome.working_path).unwrap();
    let original = synth::make_profile_bank();
    assert_eq!(rom.len(), original.len(), "o arquivo nao cresce");
    assert_eq!(read(&rom, 0), "Morcego");
    assert_eq!(
        read(&rom, 1),
        "[E3:10]O castelo esta calmo hoje.[E6]Demais."
    );
    assert_eq!(read(&rom, 2), "Cura 100 HP.");
    assert_eq!(read(&rom, 4), "Esqueleto");
    // A repetida e gravada uma vez so: os dois ponteiros vao pro mesmo lugar.
    assert_eq!(pointer(&rom, 0), pointer(&rom, 3));
    // O dado que nao e texto fica onde estava, e nada muda fora do banco e
    // da tabela de ponteiros.
    assert_eq!(pointer(&rom, 5), pointer(&original, 5));
    for (i, (a, b)) in rom.iter().zip(&original).enumerate() {
        let inside = synth::PROFILE_BANK.contains(&i)
            || (synth::PROFILE_TABLE..synth::PROFILE_TABLE + 24).contains(&i)
            || i == 0xBD;
        assert!(inside || a == b, "byte 0x{i:X} mudou fora do banco");
    }
}

#[test]
fn a_long_line_is_broken_where_the_game_breaks_it() {
    let tmp = TempDir::new("linha");
    let dir = project(&tmp);
    // A IA escreveu uma linha so, de 37 letras: passa da caixa (28) e e
    // quebrada no espaco, com o codigo do jogo — duas linhas, como o original.
    translate(
        &dir,
        &[(
            "[E3:10]The castle is quiet tonight.[E6]Too quiet.",
            "[E3:10]O castelo e quieto nesta noite demais.",
        )],
    );
    let outcome = reinsert_project_with(&dir, ErrorPolicy::Skip).unwrap();
    assert_eq!(outcome.skipped_errors, 0);
    let rom = fs::read(&outcome.working_path).unwrap();
    assert_eq!(
        read(&rom, 1),
        "[E3:10]O castelo e quieto nesta[E6]noite demais."
    );
}

#[test]
fn a_page_cannot_get_more_lines_than_the_box_had() {
    let tmp = TempDir::new("pagina");
    let dir = project(&tmp);
    let speech = "[E3:10]The castle is quiet tonight.[E6]Too quiet.";
    // Linhas refluidas: o [E6] pode sair, o app quebra de novo.
    translate(
        &dir,
        &[(speech, "[E3:10]O castelo esta quieto. Quieto demais.")],
    );
    let entry = entries(&dir)
        .into_iter()
        .find(|e| e.source_text == speech)
        .unwrap();
    let table = romtranslate_core::tbl::TblTable::load(&dir.join("tables/letras.tbl")).unwrap();
    let issues = romtranslate_core::validate::validate_entry(&entry, Some(&table));
    assert!(issues.is_empty(), "{issues:?}");
    // Mais linhas do que a caixa tinha: erro.
    translate(
        &dir,
        &[(
            speech,
            "[E3:10]O castelo esta quieto esta noite,[E6]quieto demais, quieto demais.",
        )],
    );
    let entry = entries(&dir)
        .into_iter()
        .find(|e| e.source_text == speech)
        .unwrap();
    let issues = romtranslate_core::validate::validate_entry(&entry, None);
    assert!(
        issues
            .iter()
            .any(|i| i.message.contains("linhas na pagina 1")),
        "{issues:?}"
    );
}

#[test]
fn a_translation_over_its_ceiling_stays_original_and_is_counted() {
    let tmp = TempDir::new("teto");
    let dir = project(&tmp);
    translate(
        &dir,
        &[("Restores 100 HP.", "Restaura cem pontos de vida.")],
    );

    // Padrao: a frase acima do teto e erro de validacao e bloqueia.
    let err = reinsert_project(&dir, false).unwrap_err().to_string();
    assert!(err.contains("limite"), "{err}");

    // Pulando: fica o original.
    let outcome = reinsert_project_with(&dir, ErrorPolicy::Skip).unwrap();
    assert_eq!(outcome.skipped_errors, 1);
    let rom = fs::read(&outcome.working_path).unwrap();
    assert_eq!(read(&rom, 2), "Restores 100 HP.");
}

#[test]
fn a_different_file_is_refused_instead_of_patched_blind() {
    let tmp = TempDir::new("outro");
    let dir = project(&tmp);
    translate(&dir, &[("Skeleton", "Esqueleto")]);
    let entries = entries(&dir);
    let profile = romtranslate_core::profile::load(&dir).unwrap().unwrap();
    let mut other = synth::make_profile_bank();
    other[synth::PROFILE_BANK.start + 2] ^= 0x01;
    let err = romtranslate_core::profile::apply(&dir, &profile, &mut other, &entries)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Re-extraia"), "{err}");
}

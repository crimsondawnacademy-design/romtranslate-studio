//! Assistente do perfil do jogo: com uma palavra da tela, o rascunho do
//! `perfil.toml` sai com a tabela, o cabecalho, o terminador, o alinhamento e
//! os ponteiros — e a extracao com ele le o banco.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapters::extract_project;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::profile_draft::suggest;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::synth;
use romtranslate_core::types::Platform;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-rascunho-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn project(tmp: &TempDir, rom: Vec<u8>, platform: Platform, adapter: &str) -> PathBuf {
    let source = tmp.0.join("jogo.bin");
    fs::write(&source, rom).unwrap();
    let dir = tmp.0.join("jogo.rtsproj");
    create_project(CreateProjectArgs {
        source_path: source,
        platform,
        adapter_id: adapter.to_string(),
        source_language: Some("en".to_string()),
        target_language: "pt-BR".to_string(),
        project_dir: dir.clone(),
    })
    .unwrap();
    dir
}

/// As frases que a extracao com o rascunho le. O codigo com parametro
/// (`[E3:10]`) o rascunho nao declara: a frase dele sai lida diferente.
fn texts(dir: &std::path::Path) -> Vec<String> {
    let mut texts: Vec<String> = ProjectDb::open(dir)
        .unwrap()
        .load_entries()
        .unwrap()
        .into_iter()
        .map(|e| e.source_text)
        .collect();
    texts.sort();
    texts
}

#[test]
fn a_word_of_a_ds_overlay_gives_a_profile_that_reads_the_bank() {
    let tmp = TempDir::new("ds");
    let dir = project(
        &tmp,
        synth::make_nds_profile_bank(),
        Platform::Nds,
        "nds.generic",
    );

    let draft = suggest(&dir, "Skeleton").unwrap();
    assert_eq!(draft.ascii_shift, Some(0x20));
    assert_eq!(draft.module, "overlay 0");
    assert_eq!(draft.memory, synth::NDS_OVERLAY_MEMORY);
    assert_eq!(
        (
            draft.header.as_str(),
            draft.terminator.as_str(),
            draft.align
        ),
        ("0100", "EA", 4)
    );
    // As 5 frases com ponteiro; o dado que abre igual e descartado.
    assert_eq!((draft.sentences, draft.discarded), (5, 1));
    assert!(
        draft.codes.iter().any(|(code, _)| code == "E6"),
        "{:?}",
        draft.codes
    );
    assert!(draft.path.ends_with("perfil.toml"));

    let toml = fs::read_to_string(&draft.path).unwrap();
    assert!(toml.contains("CONFERIR"), "{toml}");
    assert!(toml.contains("# quebra = "), "{toml}");
    extract_project(&dir).unwrap();
    let got = texts(&dir);
    for line in ["Bat", "Restores 100 HP.", "Skeleton"] {
        assert!(got.iter().any(|t| t == line), "{line} em {got:?}");
    }
    assert_eq!(got.len(), synth::PROFILE_LINES.len());

    // De novo: o perfil que existe nao e tocado, e a tabela fica.
    let before = fs::read_to_string(dir.join("perfil.toml")).unwrap();
    let again = suggest(&dir, "Skeleton").unwrap();
    assert!(again.path.ends_with("perfil-rascunho.toml"));
    assert!(!again.table_created);
    assert_eq!(fs::read_to_string(dir.join("perfil.toml")).unwrap(), before);
}

#[test]
fn a_word_of_a_gba_cartridge_finds_the_rom_pointers() {
    let tmp = TempDir::new("gba");
    let dir = project(
        &tmp,
        synth::make_gba_profile_bank(),
        Platform::Gba,
        "gba.generic",
    );

    let draft = suggest(&dir, "Skeleton").unwrap();
    assert_eq!((draft.memory, draft.origin), (0x0800_0000, 0));
    assert_eq!(
        (draft.header.as_str(), draft.terminator.as_str()),
        ("0100", "EA")
    );
    assert_eq!(draft.sentences, 5);
    assert!(draft.start >= synth::GBA_PROFILE_AT as u64);
    extract_project(&dir).unwrap();
    assert_eq!(texts(&dir).len(), synth::PROFILE_LINES.len());
}

#[test]
fn a_word_that_is_not_in_the_game_is_an_error_not_a_profile() {
    let tmp = TempDir::new("nada");
    let dir = project(
        &tmp,
        synth::make_gba_profile_bank(),
        Platform::Gba,
        "gba.generic",
    );
    assert!(suggest(&dir, "Zombie").is_err());
    assert!(!dir.join("perfil.toml").exists());
    // Arquivo truncado ou aleatorio: erro, sem panico.
    let tmp = TempDir::new("lixo");
    let dir = project(
        &tmp,
        synth::make_random(4096, 9),
        Platform::Nds,
        "nds.generic",
    );
    assert!(suggest(&dir, "Skeleton").is_err());
}

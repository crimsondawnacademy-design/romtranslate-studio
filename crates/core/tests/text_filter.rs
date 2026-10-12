//! Filtro de "parece texto": o lixo imprimivel (tile, codigo, tabela) cai
//! antes do teto de entries, contado; texto, palavra de menu e string com
//! ponteiro em tabela ficam.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::extract_project;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::scan::{looks_like_text, scan_bytes, ScanConfig};
use romtranslate_core::synth::{
    self, JUNK_ROM_LABELS, JUNK_ROM_MENU, JUNK_ROM_NOISE, JUNK_ROM_PHRASES, JUNK_ROM_SHORT,
};
use romtranslate_core::types::{Platform, TextEntry, TranslationStatus};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-filter-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn filtered() -> ScanConfig {
    ScanConfig {
        only_text: true,
        ..ScanConfig::default()
    }
}

fn texts(entries: &[TextEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.source_text.as_str()).collect()
}

fn in_noise(e: &TextEntry) -> bool {
    e.offset.is_some_and(|o| JUNK_ROM_NOISE.contains(&o))
}

#[test]
fn filter_keeps_phrases_and_menu_words_and_counts_what_it_drops() {
    let rom = synth::make_gba_rom_with_junk();
    let raw = scan_bytes(&rom, &ScanConfig::default()).unwrap();
    let out = scan_bytes(&rom, &filtered()).unwrap();

    let junk = raw.entries.iter().filter(|e| in_noise(e)).count();
    assert!(
        junk >= 300,
        "a fixture tem centenas de sequencias de lixo: {junk}"
    );
    let survivors = out.entries.iter().filter(|e| in_noise(e)).count();
    assert!(
        survivors * 20 <= junk,
        "{survivors} de {junk} sequencias de ruido passaram"
    );

    let kept = texts(&out.entries);
    for text in JUNK_ROM_PHRASES.iter().chain(&JUNK_ROM_MENU) {
        assert!(kept.contains(text), "{text} sumiu: {kept:?}");
    }
    // O contador bate: o que nao veio foi contado, e nada alem disso.
    assert_eq!(out.discarded, raw.entries.len() - out.entries.len());
    assert!(!out.truncated);
    // Quem ficou nao mudou: mesmo id, mesmos bytes, mesma ordem.
    let subset: Vec<&TextEntry> = raw
        .entries
        .iter()
        .filter(|e| kept.contains(&e.source_text.as_str()))
        .collect();
    assert_eq!(
        serde_json::to_string(&subset).unwrap(),
        serde_json::to_string(&out.entries.iter().collect::<Vec<_>>()).unwrap()
    );

    // Com minimo de 2 caracteres o ruido vira milhares de sequencias, e as
    // palavras de 2 e 3 letras continuam la.
    let short = scan_bytes(
        &rom,
        &ScanConfig {
            min_chars: 2,
            ..filtered()
        },
    )
    .unwrap();
    let kept = texts(&short.entries);
    for text in JUNK_ROM_SHORT {
        assert!(kept.contains(&text), "{text} sumiu");
    }
    assert!(short.discarded > 1000, "{}", short.discarded);
}

#[test]
fn scan_without_the_filter_returns_what_it_always_did() {
    let rom = synth::make_gba_rom_with_junk();
    // Referencia independente: toda sequencia de 4+ bytes imprimiveis.
    let mut expected = Vec::new();
    let mut at = 0;
    while at < rom.len() {
        let len = rom[at..]
            .iter()
            .take_while(|b| (0x20..0x7F).contains(*b))
            .count();
        if len >= 4 {
            expected.push((
                at as u64,
                String::from_utf8(rom[at..at + len].to_vec()).unwrap(),
            ));
        }
        at += len.max(1);
    }

    let out = scan_bytes(&rom, &ScanConfig::default()).unwrap();
    let got: Vec<(u64, String)> = out
        .entries
        .iter()
        .map(|e| (e.offset.unwrap(), e.source_text.clone()))
        .collect();
    assert_eq!(got, expected);
    assert_eq!((out.discarded, out.truncated), (0, false));
    assert!(out.entries.iter().any(|e| !looks_like_text(&e.source_text)));

    // Config gravada antes de o filtro existir: continua sem filtrar.
    let old: ScanConfig = serde_json::from_str(r#"{"encoding":"ascii","minChars":4}"#).unwrap();
    assert!(!old.only_text);
    assert_eq!(
        serde_json::to_string(&scan_bytes(&rom, &old).unwrap().entries).unwrap(),
        serde_json::to_string(&out.entries).unwrap()
    );
}

#[test]
fn cap_counts_after_the_filter() {
    let rom = synth::make_gba_rom_with_junk();
    let capped = |only_text| {
        scan_bytes(
            &rom,
            &ScanConfig {
                max_entries: 40,
                only_text,
                ..ScanConfig::default()
            },
        )
        .unwrap()
    };

    // Sem filtro o lixo do comeco enche o teto e o texto do fim nao aparece.
    let raw = capped(false);
    assert!(raw.truncated);
    assert!(!texts(&raw.entries).contains(&JUNK_ROM_PHRASES[0]));

    let out = capped(true);
    assert!(!out.truncated, "{} entries", out.entries.len());
    for text in JUNK_ROM_PHRASES.iter().chain(&JUNK_ROM_MENU) {
        assert!(texts(&out.entries).contains(text), "{text} sumiu");
    }

    // Teto do tamanho exato do texto: o lixo que vem depois da ultima string
    // nao conta, entao nao ha corte a avisar. Uma a menos, ha.
    let exact = |max_entries| {
        scan_bytes(
            &rom,
            &ScanConfig {
                max_entries,
                ..filtered()
            },
        )
        .unwrap()
    };
    let all = out.entries.len();
    assert_eq!(exact(all).entries.len(), all);
    assert!(!exact(all).truncated);
    assert!(exact(all - 1).truncated);
}

#[test]
fn string_with_a_pointer_in_a_table_is_never_discarded() {
    let rom = synth::make_gba_rom_with_junk();
    let out = GbaAdapter.extract(&rom).unwrap();
    let find = |text: &str| out.entries.iter().find(|e| e.source_text == text);

    // Nenhum dos tres rotulos parece texto; os dois da tabela ficam.
    for label in JUNK_ROM_LABELS {
        assert!(!looks_like_text(label), "{label}");
    }
    for label in &JUNK_ROM_LABELS[..2] {
        let entry = find(label).unwrap_or_else(|| panic!("{label} foi descartado"));
        assert_eq!(entry.pointers().len(), 1, "{label}");
        assert_eq!(entry.max_bytes, None, "realocavel");
    }
    assert!(
        find(JUNK_ROM_LABELS[2]).is_none(),
        "sem ponteiro, cai no filtro"
    );

    for text in JUNK_ROM_PHRASES.iter().chain(&JUNK_ROM_MENU) {
        assert!(find(text).is_some(), "{text} sumiu");
    }
    let raw = scan_bytes(&rom, &ScanConfig::default()).unwrap().entries;
    assert_eq!(out.discarded, raw.len() - out.entries.len());
    assert!(out.discarded >= 300, "{}", out.discarded);
    assert!(!out.truncated);
    // A extracao por `extract_structured` e a mesma lista.
    assert_eq!(
        GbaAdapter.extract_structured(&rom).unwrap().len(),
        out.entries.len()
    );
}

#[test]
fn reextracting_a_project_keeps_ids_and_translations() {
    let tmp = TempDir::new("reextract");
    let rom_path = tmp.0.join("game.gba");
    let rom = synth::make_gba_rom_with_junk();
    fs::write(&rom_path, &rom).unwrap();
    let project_dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: rom_path,
        platform: Platform::Gba,
        adapter_id: "gba.generic".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();

    // Projeto de antes do filtro: o banco tem TUDO que o scanner cru achava,
    // lixo incluso, e uma frase ja traduzida.
    let old = scan_bytes(&rom, &ScanConfig::default()).unwrap().entries;
    let phrase = old
        .iter()
        .find(|e| e.source_text == JUNK_ROM_PHRASES[1])
        .unwrap()
        .id
        .clone();
    let mut db = ProjectDb::open(&project_dir).unwrap();
    db.upsert_entries(&old).unwrap();
    db.update_translation(
        &phrase,
        "Voce achou uma chave!",
        TranslationStatus::Reviewed,
    )
    .unwrap();
    drop(db);

    let first = extract_project(&project_dir).unwrap();
    assert!(first.discarded >= 300 && !first.truncated, "{first:?}");
    let load = || {
        ProjectDb::open(&project_dir)
            .unwrap()
            .load_entries()
            .unwrap()
    };
    let after = load();
    // Mesmo id (`scan-<offset>`): a traducao e o status ficaram na mesma entry.
    let entry = after.iter().find(|e| e.id == phrase).unwrap();
    assert_eq!(
        entry.translated_text.as_deref(),
        Some("Voce achou uma chave!")
    );
    assert_eq!(entry.status, TranslationStatus::Reviewed);
    assert_eq!(
        entry.max_bytes,
        Some(JUNK_ROM_PHRASES[1].len()),
        "virou estruturada"
    );
    // Nada novo no banco: a extracao so devolveu ids que o scanner ja dava.
    assert_eq!(after.len(), old.len());

    // Extrair de novo e idempotente.
    db = ProjectDb::open(&project_dir).unwrap();
    let menu = after
        .iter()
        .find(|e| e.source_text == JUNK_ROM_MENU[0])
        .unwrap()
        .id
        .clone();
    db.update_translation(&menu, "SALVAR", TranslationStatus::Machine)
        .unwrap();
    drop(db);
    assert_eq!(extract_project(&project_dir).unwrap(), first);
    let again = load();
    let ids = |entries: &[TextEntry]| entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&again), ids(&after));
    let translated: Vec<(&str, &str)> = again
        .iter()
        .filter_map(|e| Some((e.source_text.as_str(), e.translated_text.as_deref()?)))
        .collect();
    assert_eq!(
        translated,
        [
            (JUNK_ROM_PHRASES[1], "Voce achou uma chave!"),
            (JUNK_ROM_MENU[0], "SALVAR"),
        ]
    );
}

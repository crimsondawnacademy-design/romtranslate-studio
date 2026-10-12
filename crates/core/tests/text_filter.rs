//! Filtro de "parece texto": o lixo imprimivel (tile, codigo, tabela) cai
//! antes do teto de entries, contado; texto, palavra de menu e string com
//! ponteiro em tabela ficam. E o lixo sem traducao que um projeto antigo ja
//! tinha sai na extracao estruturada, contado tambem.

use std::fs;
use std::path::{Path, PathBuf};

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::adapters::{extract_project, ExtractOutcome};
use romtranslate_core::db::ProjectDb;
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::scan::{looks_like_text, scan_bytes, ScanConfig};
use romtranslate_core::synth::{
    self, JUNK_ROM_LABELS, JUNK_ROM_MENU, JUNK_ROM_NOISE, JUNK_ROM_PHRASES, JUNK_ROM_SHORT,
};
use romtranslate_core::types::{Platform, TextEncoding, TextEntry, TranslationStatus};
use serde_json::json;

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

fn ids(entries: &[TextEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.id.as_str()).collect()
}

/// O que o scanner cru achava no `make_gba_rom_with_junk` antes de o filtro
/// existir: lixo incluso.
fn raw_scan() -> Vec<TextEntry> {
    scan_bytes(&synth::make_gba_rom_with_junk(), &ScanConfig::default())
        .unwrap()
        .entries
}

/// Onde estao, em `entries`, as sequencias do ruido que nao parecem texto.
/// A extracao estruturada nao devolve nenhuma delas.
fn junk(entries: &[TextEntry]) -> Vec<usize> {
    let is_junk = |e: &TextEntry| in_noise(e) && !looks_like_text(&e.source_text);
    (0..entries.len())
        .filter(|&i| is_junk(&entries[i]))
        .collect()
}

/// Projeto do `make_gba_rom_with_junk` com `old` ja no banco, como um projeto
/// criado antes de o filtro existir.
fn old_project(label: &str, old: &[TextEntry]) -> (TempDir, PathBuf) {
    let tmp = TempDir::new(label);
    let rom_path = tmp.0.join("game.gba");
    fs::write(&rom_path, synth::make_gba_rom_with_junk()).unwrap();
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
    ProjectDb::open(&project_dir)
        .unwrap()
        .upsert_entries(old)
        .unwrap();
    (tmp, project_dir)
}

fn load(project_dir: &Path) -> Vec<TextEntry> {
    ProjectDb::open(project_dir)
        .unwrap()
        .load_entries()
        .unwrap()
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
    // Projeto de antes do filtro: o banco tem TUDO que o scanner cru achava,
    // lixo incluso, e uma frase ja traduzida.
    let old = raw_scan();
    let (_tmp, project_dir) = old_project("reextract", &old);
    let phrase = old
        .iter()
        .find(|e| e.source_text == JUNK_ROM_PHRASES[1])
        .unwrap()
        .id
        .clone();
    let mut db = ProjectDb::open(&project_dir).unwrap();
    db.update_translation(
        &phrase,
        "Voce achou uma chave!",
        TranslationStatus::Reviewed,
    )
    .unwrap();
    drop(db);

    let first = extract_project(&project_dir).unwrap();
    assert!(first.discarded >= 300 && !first.truncated, "{first:?}");
    let after = load(&project_dir);
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
    // Nada novo no banco (a extracao so devolveu ids que o scanner ja dava),
    // e o lixo antigo sem traducao saiu, contado.
    assert_eq!(after.len(), first.saved);
    assert_eq!(first.removed, old.len() - after.len());

    // Extrair de novo e idempotente, e nao ha mais o que tirar.
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
    assert_eq!(
        extract_project(&project_dir).unwrap(),
        ExtractOutcome {
            removed: 0,
            ..first
        }
    );
    let again = load(&project_dir);
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

#[test]
fn reextracting_an_old_project_removes_untranslated_junk_and_counts_it() {
    let old = raw_scan();
    let (_tmp, project_dir) = old_project("prune", &old);

    let outcome = extract_project(&project_dir).unwrap();
    let after = load(&project_dir);

    // O numero devolvido e a diferenca, e o banco ficou com o que a extracao
    // devolve: nenhuma sequencia de lixo, "LV03" (sem ponteiro) inclusive.
    assert!(outcome.removed >= 300, "{outcome:?}");
    assert_eq!(outcome.removed, old.len() - after.len());
    let extracted = GbaAdapter
        .extract(&synth::make_gba_rom_with_junk())
        .unwrap()
        .entries;
    assert_eq!(ids(&after), ids(&extracted));
    assert!(!texts(&after).contains(&JUNK_ROM_LABELS[2]));
    assert!(after
        .iter()
        .all(|e| !e.pointers().is_empty() || looks_like_text(&e.source_text)));

    // Texto, menu e rotulo com ponteiro continuam, com o id de antes.
    let id_of = |entries: &[TextEntry], text: &str| {
        let found = entries.iter().find(|e| e.source_text == text);
        found.map(|e| e.id.clone())
    };
    for text in JUNK_ROM_PHRASES
        .iter()
        .chain(&JUNK_ROM_MENU)
        .chain(&JUNK_ROM_LABELS[..2])
    {
        assert!(id_of(&after, text).is_some(), "{text} sumiu");
        assert_eq!(id_of(&after, text), id_of(&old, text), "{text}");
    }
}

#[test]
fn junk_with_a_translation_is_not_removed() {
    let old = raw_scan();
    let junk = &old[junk(&old)[0]];
    let (_tmp, project_dir) = old_project("translated", &old);
    ProjectDb::open(&project_dir)
        .unwrap()
        .update_translation(&junk.id, "ja mexi aqui", TranslationStatus::Machine)
        .unwrap();

    let outcome = extract_project(&project_dir).unwrap();
    let after = load(&project_dir);

    let kept = after.iter().find(|e| e.id == junk.id);
    let kept = kept.expect("lixo com traducao saiu do projeto");
    assert_eq!(kept.translated_text.as_deref(), Some("ja mexi aqui"));
    assert_eq!(kept.status, TranslationStatus::Machine);
    // So ela ficou alem do que a extracao devolveu.
    assert_eq!(after.len(), outcome.saved + 1);
    assert_eq!(outcome.removed, old.len() - after.len());
}

#[test]
fn stale_entry_with_a_pointer_in_a_table_is_not_removed() {
    // Projeto em que uma extracao antiga marcou ponteiro (u32 ou u16) numa
    // string que a de hoje nao devolve: ponteiro e evidencia mais forte que
    // aparencia, entao ela fica mesmo sem parecer texto e sem traducao.
    let mut old = raw_scan();
    let at = junk(&old);
    old[at[0]].metadata["pointers"] = json!([0x8400]);
    old[at[1]].metadata["pointers16"] = json!([0x8410]);
    let (_tmp, project_dir) = old_project("pointer", &old);

    let outcome = extract_project(&project_dir).unwrap();
    let after = load(&project_dir);

    for i in &at[..2] {
        let kept = after.iter().find(|e| e.id == old[*i].id);
        let kept = kept.unwrap_or_else(|| panic!("{} saiu do projeto", old[*i].id));
        assert_eq!(kept.pointers().len(), 1);
        assert_eq!(kept.translated_text, None);
        assert!(!looks_like_text(&kept.source_text));
    }
    assert_eq!(after.len(), outcome.saved + 2);
    assert_eq!(outcome.removed, old.len() - after.len());
}

#[test]
fn stale_entry_read_through_a_character_table_is_not_removed() {
    // Texto lido por `.tbl` vem de um scan que o usuario pediu, e a regra de
    // "parece texto" e fraca pra ele.
    let mut old = raw_scan();
    let at = junk(&old)[0];
    old[at].encoding = TextEncoding::Table("letras-41-61".into());
    let (_tmp, project_dir) = old_project("table", &old);

    let outcome = extract_project(&project_dir).unwrap();
    let after = load(&project_dir);

    let kept = after.iter().find(|e| e.id == old[at].id);
    let kept = kept.expect("entry de tabela saiu do projeto");
    assert!(matches!(&kept.encoding, TextEncoding::Table(id) if id == "letras-41-61"));
    assert_eq!(kept.translated_text, None);
    assert_eq!(after.len(), outcome.saved + 1);
    assert_eq!(outcome.removed, old.len() - after.len());
}

#[test]
fn text_the_extraction_does_not_return_is_not_removed() {
    // Escanear com minimo de 2 caracteres acha "Yes", "No" e "HP". A extracao
    // estruturada le de 4 em diante e nao devolve nenhuma das tres: ficam,
    // porque parecem texto. O lixo de 2 e 3 caracteres sai.
    let rom = synth::make_gba_rom_with_junk();
    let short = ScanConfig {
        min_chars: 2,
        ..ScanConfig::default()
    };
    let old = scan_bytes(&rom, &short).unwrap().entries;
    let (_tmp, project_dir) = old_project("short", &old);

    let outcome = extract_project(&project_dir).unwrap();
    let after = load(&project_dir);

    for text in JUNK_ROM_SHORT {
        assert!(texts(&after).contains(&text), "{text} saiu do projeto");
    }
    assert!(after.len() > outcome.saved);
    assert!(outcome.removed > 1000, "{outcome:?}");
    assert_eq!(outcome.removed, old.len() - after.len());
    assert!(after
        .iter()
        .all(|e| !e.pointers().is_empty() || looks_like_text(&e.source_text)));
}

#[test]
fn what_the_extraction_returned_is_never_removed() {
    // Formato declarado (RTSF) nao filtra: devolve o que o jogo declara,
    // pareca texto ou nao. O que acabou de ser gravado nao pode sair em
    // seguida.
    let tmp = TempDir::new("returned");
    let old = raw_scan();
    let at = junk(&old);
    let pair = [old[at[0]].clone(), old[at[1]].clone()];
    let mut db = ProjectDb::open(&tmp.0).unwrap();
    db.upsert_entries(&pair).unwrap();

    assert_eq!(db.remove_stale_junk(&pair[..1]).unwrap(), 1);
    assert_eq!(ids(&db.load_entries().unwrap()), [pair[0].id.as_str()]);
}

#[test]
fn second_extraction_removes_nothing() {
    let (_tmp, project_dir) = old_project("twice", &raw_scan());

    let first = extract_project(&project_dir).unwrap();
    let after = load(&project_dir);
    let second = extract_project(&project_dir).unwrap();

    assert!(first.removed > 0);
    assert_eq!(
        second,
        ExtractOutcome {
            removed: 0,
            ..first
        }
    );
    assert_eq!(
        serde_json::to_string(&load(&project_dir)).unwrap(),
        serde_json::to_string(&after).unwrap()
    );
}

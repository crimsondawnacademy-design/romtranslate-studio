//! Relocacao dentro de ARQUIVOS (ponteiro = offset relativo ao arquivo):
//! NDS cresce o arquivo e reaponta a FAT; PS1 usa a sobra do setor final.
//! Em offset relativo o run minimo da tabela e 3 (numero pequeno e comum em
//! dado binario) — a tabela de 2 da fixture nao pode ser aceita.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::nds::NdsAdapter;
use romtranslate_core::adapters::ps1::Ps1Adapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::patch::{apply_bps, apply_ips, export_patch};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::reinsert_project;
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, ResourceDescriptor, TextEntry, TranslationStatus};
use romtranslate_core::validate::{validate_entry, IssueKind, Severity};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-relocf-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Entry com esse texto dentro desse arquivo.
fn find<'a>(entries: &'a [TextEntry], file: &str, text: &str) -> &'a TextEntry {
    entries
        .iter()
        .find(|e| e.source_text == text && e.resource_path.as_deref() == Some(file))
        .unwrap_or_else(|| panic!("\"{text}\" nao extraida de {file}"))
}

fn translate_in(entries: &mut [TextEntry], file: &str, pairs: &[(&str, &str)]) {
    for e in entries.iter_mut() {
        if e.resource_path.as_deref() != Some(file) {
            continue;
        }
        if let Some((_, t)) = pairs.iter().find(|(s, _)| *s == e.source_text) {
            e.translated_text = Some(t.to_string());
            e.status = TranslationStatus::Machine;
        }
    }
}

fn resource(adapter: &dyn GameAdapter, image: &[u8], path: &str) -> ResourceDescriptor {
    adapter
        .list_resources(image)
        .unwrap()
        .into_iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} nao listado"))
}

/// Ponteiro i da tabela da fixture (u32 LE em 4 + 4i, relativo ao arquivo).
fn table_entry(file: &[u8], i: usize) -> usize {
    u32::from_le_bytes(file[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize
}

// ------------------------------------------------------------------ NDS ----

#[test]
fn nds_detects_three_entry_table_but_not_two_entry_one() {
    let rom = synth::make_nds_rom_with_message_table();
    let entries = NdsAdapter.extract_structured(&rom).unwrap();
    let msg = resource(&NdsAdapter, &rom, "msg.bin").offset as usize;

    // So a tabela de 3 (em +0x04) conta; a de 2 (em +0x2C) nao.
    let new_game = find(&entries, "msg.bin", "NEW GAME");
    assert_eq!(new_game.pointer_offsets(), vec![msg + 0x04]);
    assert_eq!(new_game.max_bytes, None, "arquivo de DS cresce: sem teto");
    assert!(find(&entries, "intro.txt", "PRESS START BUTTON")
        .pointer_offsets()
        .is_empty());
}

#[test]
fn nds_relocation_grows_file_and_repoints_fat() {
    let rom = synth::make_nds_rom_with_message_table();
    let mut entries = NdsAdapter.extract_structured(&rom).unwrap();
    translate_in(
        &mut entries,
        "msg.bin",
        &[
            ("NEW GAME", "NOVO JOGO"), // 9 > 8: realoca
            ("CONTINUE", "CONTINUAR"), // 9 > 8: realoca
            ("OPTIONS", "OPCOES"),     // cabe: in-place
        ],
    );
    let applied = NdsAdapter.apply_text(&rom, &entries).unwrap();
    let out = &applied.bytes;
    assert_eq!(applied.report.relocated, 2);

    // A FAT aponta pra copia nova, no fim do ROM e alinhada em 0x200.
    let moved = resource(&NdsAdapter, out, "msg.bin");
    let start = moved.offset as usize;
    assert!(
        start >= rom.len() && start.is_multiple_of(0x200),
        "0x{start:X}"
    );
    let file = &out[start..start + moved.size as usize];
    assert_eq!(&file[table_entry(file, 0)..][..10], b"NOVO JOGO\0");
    assert_eq!(&file[table_entry(file, 1)..][..10], b"CONTINUAR\0");
    assert_eq!(
        table_entry(file, 2),
        0x22,
        "OPTIONS coube: ponteiro intacto"
    );
    assert_eq!(&file[0x22..0x29], b"OPCOES\0");

    // Os outros arquivos nao se mexeram.
    assert_eq!(
        resource(&NdsAdapter, out, "intro.txt").offset,
        resource(&NdsAdapter, &rom, "intro.txt").offset
    );

    // Header: tamanho usado cobre o arquivo novo; CRC-16 recalculado.
    let used = u32::from_le_bytes(out[0x80..0x84].try_into().unwrap()) as usize;
    assert!(used >= start + file.len());
    let verification = NdsAdapter.verify(out).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);
    let reread = NdsAdapter.extract_structured(out).unwrap();
    assert_eq!(
        find(&reread, "msg.bin", "NOVO JOGO").offset,
        Some((start + table_entry(file, 0)) as u64)
    );
}

#[test]
fn nds_full_project_flow_patch_reproduces_grown_rom() {
    let tmp = TempDir::new("nds");
    let rom = synth::make_nds_rom_with_message_table();
    let rom_path = tmp.0.join("msg.nds");
    fs::write(&rom_path, &rom).unwrap();
    let project_dir = tmp.0.join("msg.rtsproj");
    create_project(CreateProjectArgs {
        source_path: rom_path.clone(),
        platform: Platform::Nds,
        adapter_id: "nds.generic".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();
    let mut entries = NdsAdapter.extract_structured(&rom).unwrap();
    translate_in(&mut entries, "msg.bin", &[("NEW GAME", "NOVO JOGO")]);
    ProjectDb::open(&project_dir)
        .unwrap()
        .upsert_entries(&entries)
        .unwrap();

    let outcome = reinsert_project(&project_dir, false).unwrap();
    assert_eq!(outcome.apply.relocated, 1);
    assert_eq!(fs::read(&rom_path).unwrap(), rom, "original intacto");

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

// ------------------------------------------------------------------ PS1 ----

#[test]
fn ps1_marks_table_strings_with_sector_slack_budget() {
    let bin = synth::make_ps1_bin_with_message_table();
    let entries = Ps1Adapter.extract_structured(&bin).unwrap();

    // MSG.DAT: 0x38 bytes num setor de 2048 -> sobra de 2048 - 0x38 - 1
    // (terminador) pra string realocada.
    let msg = find(&entries, "MSG.DAT", "NEW GAME");
    assert_eq!(msg.pointer_offsets().len(), 1, "so a tabela de 3");
    assert_eq!(msg.max_bytes, Some(2048 - 0x38 - 1));

    // FULL.DAT: mesma estrutura com 2048 bytes exatos -> sem sobra, fica in-place.
    let full = find(&entries, "FULL.DAT", "NEW GAME");
    assert!(full.pointer_offsets().is_empty());
    assert_eq!(full.max_bytes, Some(8));
}

#[test]
fn ps1_relocates_into_sector_slack_and_keeps_edc_valid() {
    let bin = synth::make_ps1_bin_with_message_table();
    let mut entries = Ps1Adapter.extract_structured(&bin).unwrap();
    translate_in(
        &mut entries,
        "MSG.DAT",
        &[("NEW GAME", "NOVO JOGO"), ("CONTINUE", "CONTINUAR")],
    );
    let applied = Ps1Adapter.apply_text(&bin, &entries).unwrap();
    let out = &applied.bytes;
    assert_eq!(applied.report.relocated, 2);
    assert_eq!(out.len(), bin.len(), "nada anexado: tudo cabe no setor");

    // EDC/ECC dos setores tocados (arquivo + diretorio) regenerados.
    let verification = Ps1Adapter.verify(out).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);

    // Arquivo de 1 setor: os bytes logicos sao contiguos no BIN.
    let msg = resource(&Ps1Adapter, out, "MSG.DAT");
    assert!(
        msg.size > 0x38 && msg.size <= 2048,
        "tamanho novo: {}",
        msg.size
    );
    let file = &out[msg.offset as usize..(msg.offset + msg.size) as usize];
    assert_eq!(&file[table_entry(file, 0)..][..10], b"NOVO JOGO\0");
    assert_eq!(&file[table_entry(file, 1)..][..10], b"CONTINUAR\0");
    let reread = Ps1Adapter.extract_structured(out).unwrap();
    find(&reread, "MSG.DAT", "CONTINUAR");
}

#[test]
fn ps1_refuses_when_slack_has_data_or_budget_runs_out() {
    let bin = synth::make_ps1_bin_with_message_table();
    let mut entries = Ps1Adapter.extract_structured(&bin).unwrap();

    // Sobra compartilhada: cada uma cabe sozinha, as duas juntas nao.
    let long = "X".repeat(1500);
    translate_in(
        &mut entries,
        "MSG.DAT",
        &[("NEW GAME", long.as_str()), ("CONTINUE", long.as_str())],
    );
    let err = Ps1Adapter
        .apply_text(&bin, &entries)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("setor final") && err.contains("sem espaco"),
        "{err}"
    );

    // Byte nao-zero na sobra (dado escondido?): recusa em vez de pisar.
    translate_in(
        &mut entries,
        "MSG.DAT",
        &[("NEW GAME", "NOVO JOGO"), ("CONTINUE", "CONTINUE")],
    );
    let msg = resource(&Ps1Adapter, &bin, "MSG.DAT");
    let mut dirty = bin.clone();
    dirty[(msg.offset + msg.size) as usize + 100] = 0xAB;
    let err = Ps1Adapter
        .apply_text(&dirty, &entries)
        .unwrap_err()
        .to_string();
    assert!(err.contains("nao esta zerado"), "{err}");

    // Arquivo sem sobra nenhuma: continua exigindo texto menor.
    let mut entries = Ps1Adapter.extract_structured(&bin).unwrap();
    translate_in(&mut entries, "FULL.DAT", &[("NEW GAME", "NOVO JOGO")]);
    let err = Ps1Adapter
        .apply_text(&bin, &entries)
        .unwrap_err()
        .to_string();
    assert!(err.contains("encurte"), "{err}");
}

#[test]
fn validator_uses_the_slack_as_ceiling_on_ps1() {
    let bin = synth::make_ps1_bin_with_message_table();
    let mut entries = Ps1Adapter.extract_structured(&bin).unwrap();
    let too_long = "X".repeat(2000);
    translate_in(
        &mut entries,
        "MSG.DAT",
        &[("NEW GAME", "NOVO JOGO"), ("CONTINUE", too_long.as_str())],
    );

    let fits = validate_entry(find(&entries, "MSG.DAT", "NEW GAME"));
    assert!(
        fits.iter().all(|i| i.severity == Severity::Warning),
        "{fits:?}"
    );
    assert!(fits.iter().any(|i| i.message.contains("realocada")));

    let over = validate_entry(find(&entries, "MSG.DAT", "CONTINUE"));
    assert!(over
        .iter()
        .any(|i| i.kind == IssueKind::ByteOverflow && i.severity == Severity::Error));
}

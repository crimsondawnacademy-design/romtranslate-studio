//! PS1/PS2/PSP: deteccao (com o discriminador BOOT/BOOT2 e PARAM.SFO),
//! filesystem ISO 9660 (2048 e raw 2352), extracao por arquivo e reinsercao.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::ps1::Ps1Adapter;
use romtranslate_core::adapters::ps2::Ps2Adapter;
use romtranslate_core::adapters::psp::{sfo_string, PspAdapter};
use romtranslate_core::db::ProjectDb;
use romtranslate_core::detect::inspect;
use romtranslate_core::patch::{apply_ips, export_patch};
use romtranslate_core::project::{create_project, CreateProjectArgs};
use romtranslate_core::reinsert::reinsert_project;
use romtranslate_core::synth;
use romtranslate_core::types::{Platform, TranslationStatus};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-ps-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn probes_distinguish_ps1_ps2_psp() {
    let tmp = TempDir::new("probe");

    let ps1_path = tmp.0.join("game.bin");
    fs::write(&ps1_path, synth::make_ps1_bin()).unwrap();
    let best = inspect(&ps1_path).unwrap().best.expect("ps1");
    assert_eq!(best.platform, Platform::Ps1);
    assert!(best.confidence >= 0.9);
    assert!(
        best.evidence.iter().any(|e| e.contains("BOOT = ")),
        "{:?}",
        best.evidence
    );
    assert!(best.evidence.iter().any(|e| e.contains("raw")));

    let ps2_path = tmp.0.join("game.iso");
    fs::write(&ps2_path, synth::make_ps2_iso()).unwrap();
    let report = inspect(&ps2_path).unwrap();
    let best = report.best.expect("ps2");
    assert_eq!(best.platform, Platform::Ps2);
    assert!(best.evidence.iter().any(|e| e.contains("BOOT2")));
    // O probe de PS1 nao pode reivindicar um disco com BOOT2.
    assert!(!report.results.iter().any(|r| r.platform == Platform::Ps1));

    let psp_path = tmp.0.join("umd.iso");
    fs::write(&psp_path, synth::make_psp_iso()).unwrap();
    let best = inspect(&psp_path).unwrap().best.expect("psp");
    assert_eq!(best.platform, Platform::Psp);
    assert!(best
        .evidence
        .iter()
        .any(|e| e.contains("SYNTHETIC PSP QUEST")));
    assert!(best.evidence.iter().any(|e| e.contains("ULUS01234")));
}

#[test]
fn iso_walker_lists_files_in_plain_and_raw() {
    let ps2 = synth::make_ps2_iso();
    let files = Ps2Adapter.list_resources(&ps2).unwrap();
    let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"SYSTEM.CNF"), "{paths:?}");
    assert!(paths.contains(&"DATA/TEXT.PAK"), "subdir: {paths:?}");

    let ps1 = synth::make_ps1_bin();
    let files = Ps1Adapter.list_resources(&ps1).unwrap();
    let game = files.iter().find(|f| f.path == "GAME.DAT").unwrap();
    assert_eq!(game.size, 3000);

    // Headers mentirosos e truncados erram limpo.
    let mut lying = synth::make_ps2_iso();
    let pvd_root = 16 * 2048 + 156;
    lying[pvd_root + 2..pvd_root + 6].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Ps2Adapter.list_resources(&lying).is_err());
    for len in (0..ps2.len()).step_by(4093) {
        let _ = Ps2Adapter.extract_structured(&ps2[..len]);
    }
}

#[test]
fn ps1_raw_extract_and_reinsert_with_edc_ecc_regeneration() {
    let bin = synth::make_ps1_bin();
    let entries = Ps1Adapter.extract_structured(&bin).unwrap();
    let texts: Vec<_> = entries.iter().map(|e| e.source_text.as_str()).collect();
    // String no primeiro setor do arquivo E no segundo (regioes por setor).
    assert!(texts.contains(&"INSERT COIN TO CONTINUE"), "{texts:?}");
    assert!(texts.contains(&"MEMORY CARD NOT FOUND"));
    let coin = entries
        .iter()
        .find(|e| e.source_text == "INSERT COIN TO CONTINUE")
        .unwrap();
    assert_eq!(coin.resource_path.as_deref(), Some("GAME.DAT"));
    // Offset absoluto correto dentro do BIN raw: os bytes estao la mesmo.
    let off = coin.offset.unwrap() as usize;
    assert_eq!(&bin[off..off + 23], b"INSERT COIN TO CONTINUE");

    // O fixture ja nasce com EDC/ECC validos — o verify confere setor a setor.
    let baseline = Ps1Adapter.verify(&bin).unwrap();
    assert!(baseline.ok, "{:?}", baseline.problems);
    assert!(
        baseline.checks.iter().any(|c| c.contains("EDC integro")),
        "{:?}",
        baseline.checks
    );

    // Roundtrip em raw: escreve in-place e regenera EDC/ECC dos setores tocados.
    let mut translated = entries.clone();
    for e in translated.iter_mut() {
        match e.source_text.as_str() {
            "INSERT COIN TO CONTINUE" => e.translated_text = Some("INSIRA FICHA".to_string()),
            "MEMORY CARD NOT FOUND" => e.translated_text = Some("SEM MEMORY CARD".to_string()),
            _ => {}
        }
    }
    let applied = Ps1Adapter.apply_text(&bin, &translated).unwrap();
    assert_eq!(applied.report.applied, 2);
    let verification = Ps1Adapter.verify(&applied.bytes).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);
    let reread = Ps1Adapter.extract_structured(&applied.bytes).unwrap();
    assert!(reread.iter().any(|e| e.source_text == "INSIRA FICHA"));
    assert!(reread.iter().any(|e| e.source_text == "SEM MEMORY CARD"));

    // Prova de que o EDC realmente foi recalculado: o mesmo texto escrito
    // sem regenerar (patch byte a byte) tem que reprovar no verify.
    let mut naive = bin.clone();
    naive[off..off + 23].copy_from_slice(b"INSIRA FICHA\0\0\0\0\0\0\0\0\0\0\0");
    let broken = Ps1Adapter.verify(&naive).unwrap();
    assert!(
        broken.problems.iter().any(|p| p.contains("EDC invalido")),
        "{:?}",
        broken.problems
    );
}

#[test]
fn ps2_and_psp_roundtrip_in_place() {
    for (adapter, image, source, translated) in [
        (
            &Ps2Adapter as &dyn GameAdapter,
            synth::make_ps2_iso(),
            "PRESS X TO JUMP",
            "APERTE X",
        ),
        (
            &PspAdapter as &dyn GameAdapter,
            synth::make_psp_iso(),
            "CONTINUE ADVENTURE",
            "CONTINUAR AVENTURA",
        ),
    ] {
        let mut entries = adapter.extract_structured(&image).unwrap();
        for e in entries.iter_mut() {
            if e.source_text == source {
                e.translated_text = Some(translated.to_string());
            }
        }
        let applied = adapter.apply_text(&image, &entries).unwrap();
        assert_eq!(applied.report.applied, 1, "{}", adapter.id());
        let verification = adapter.verify(&applied.bytes).unwrap();
        assert!(
            verification.ok,
            "{}: {:?}",
            adapter.id(),
            verification.problems
        );
        let reread = adapter.extract_structured(&applied.bytes).unwrap();
        assert!(reread.iter().any(|e| e.source_text == translated));
    }
}

#[test]
fn sfo_parser_reads_fields_and_survives_garbage() {
    let psp = synth::make_psp_iso();
    let files = PspAdapter.list_resources(&psp).unwrap();
    let sfo_file = files
        .iter()
        .find(|f| f.path == "PSP_GAME/PARAM.SFO")
        .unwrap();
    let sfo = &psp[sfo_file.offset as usize..(sfo_file.offset + sfo_file.size) as usize];
    assert_eq!(
        sfo_string(sfo, "TITLE").as_deref(),
        Some("SYNTHETIC PSP QUEST")
    );
    assert_eq!(sfo_string(sfo, "DISC_ID").as_deref(), Some("ULUS01234"));
    assert!(sfo_string(sfo, "NAO_EXISTE").is_none());
    assert!(sfo_string(b"NOTSFO", "TITLE").is_none());
    for len in 0..sfo.len().min(0x40) {
        let _ = sfo_string(&sfo[..len], "TITLE");
    }
}

#[test]
fn ps2_full_project_flow_reinsert_and_patch() {
    let tmp = TempDir::new("flow");
    let iso = synth::make_ps2_iso();
    let iso_path = tmp.0.join("game.iso");
    fs::write(&iso_path, &iso).unwrap();
    let project_dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: iso_path.clone(),
        platform: Platform::Ps2,
        adapter_id: "ps2.generic".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();

    let mut entries = Ps2Adapter.extract_structured(&iso).unwrap();
    for e in entries.iter_mut() {
        if e.source_text == "SAVE PROGRESS?" {
            e.translated_text = Some("SALVAR JOGO?".to_string());
            e.status = TranslationStatus::Machine;
        }
    }
    let mut db = ProjectDb::open(&project_dir).unwrap();
    db.upsert_entries(&entries).unwrap();
    drop(db);

    let reinserted = reinsert_project(&project_dir, false).unwrap();
    assert!(reinserted.verification.ok);
    assert_eq!(fs::read(&iso_path).unwrap(), iso, "original intacto");

    let outcome = export_patch(&project_dir, None).unwrap();
    let working = fs::read(&reinserted.working_path).unwrap();
    let patch = fs::read(&outcome.patch_path).unwrap();
    assert_eq!(apply_ips(&iso, &patch).unwrap(), working);
    assert!(working.windows(12).any(|w| w == b"SALVAR JOGO?"));
}

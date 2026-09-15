//! Integracao do Sprint 6: IPS (create/apply com edge cases), export de patch
//! com manifest, e o fluxo GBA conservador de ponta a ponta:
//! extrair -> traduzir -> reinserir -> patch -> aplicar == working copy.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::gba::GbaAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::patch::{apply_ips, create_ips, export_patch};
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
        let dir = std::env::temp_dir().join(format!("rts-patch-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn roundtrip(original: &[u8], modified: &[u8]) {
    let patch = create_ips(original, modified).unwrap();
    assert_eq!(&apply_ips(original, &patch).unwrap(), modified);
}

#[test]
fn ips_roundtrip_covers_common_shapes() {
    let base: Vec<u8> = (0..2048u32).map(|i| (i % 251) as u8).collect();

    // Sem mudanca: patch minimo, apply devolve o original.
    let patch = create_ips(&base, &base).unwrap();
    assert_eq!(patch, b"PATCHEOF");
    assert_eq!(apply_ips(&base, &patch).unwrap(), base);

    // Mudanca no comeco, no meio, no fim.
    let mut m = base.clone();
    m[0] = 0xAA;
    m[1000] = 0xBB;
    let last = m.len() - 1;
    m[last] = 0xCC;
    roundtrip(&base, &m);

    // Duas mudancas separadas por gap curto (fundem num record so).
    let mut m2 = base.clone();
    m2[100] = 1;
    m2[103] = 2;
    let patch2 = create_ips(&base, &m2).unwrap();
    roundtrip(&base, &m2);
    // 1 record: PATCH(5) + header(5) + 4 bytes de dados + EOF(3)
    assert_eq!(patch2.len(), 5 + 5 + 4 + 3);

    // Arquivo estendido alem do original.
    let mut m3 = base.clone();
    m3.extend_from_slice(b"APPENDED DATA");
    roundtrip(&base, &m3);

    // Run maior que um record (0xFFFF) quebra em varios.
    let big: Vec<u8> = vec![0u8; 0x2_0000];
    let big_mod: Vec<u8> = vec![0xFFu8; 0x2_0000];
    roundtrip(&big, &big_mod);
}

#[test]
fn ips_eof_offset_collision_is_dodged() {
    // Mudanca comecando exatamente em 0x454F46 ("EOF" lido como offset).
    let size = 0x454F46 + 64;
    let base = vec![0u8; size];
    let mut m = base.clone();
    m[0x454F46] = 0x77;
    let patch = create_ips(&base, &m).unwrap();
    // Nenhum record comeca no offset proibido...
    assert_ne!(&patch[5..8], b"EOF");
    // ...e o resultado continua exato.
    assert_eq!(apply_ips(&base, &patch).unwrap(), m);
}

#[test]
fn ips_limits_and_malformed_patches_error_cleanly() {
    // Encolher o arquivo: IPS nao representa.
    assert!(create_ips(&[1, 2, 3, 4], &[1, 2]).is_err());
    // Mudanca alem de 16 MiB: erro claro.
    let big = vec![0u8; 0x100_0010];
    let mut big_mod = big.clone();
    let last = big_mod.len() - 1;
    big_mod[last] = 1;
    assert!(create_ips(&big, &big_mod).is_err());
    // Patches malformados nunca panicam.
    assert!(apply_ips(&[0; 8], b"NOTAPATCH").is_err());
    assert!(apply_ips(&[0; 8], b"PATCH").is_err());
    assert!(apply_ips(&[0; 8], b"PATCH\x00\x00\x10\x00\x05AB").is_err());
}

#[test]
fn ips_apply_supports_rle_and_truncate_extension() {
    // Patch de terceiros com RLE: offset 4, size 0, rle_len 6, byte 0x55.
    let mut patch = Vec::from(&b"PATCH"[..]);
    patch.extend_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x06, 0x55]);
    patch.extend_from_slice(b"EOF");
    let out = apply_ips(&[0u8; 16], &patch).unwrap();
    assert_eq!(&out[4..10], &[0x55; 6]);

    // Truncate extension: 3 bytes apos EOF com o tamanho final.
    patch.extend_from_slice(&[0x00, 0x00, 0x0A]);
    let out = apply_ips(&[0u8; 16], &patch).unwrap();
    assert_eq!(out.len(), 0x0A);
}

/// DoD do Sprint 6 sobre a fixture RTSF: o patch exportado aplicado ao
/// original reproduz EXATAMENTE a working copy, e o manifest descreve o projeto.
#[test]
fn export_patch_rtsf_dod_roundtrip_and_manifest() {
    let tmp = TempDir::new("rtsf");
    let rom_path = tmp.0.join("game.rtsf");
    fs::write(&rom_path, synth::make_rtsf_fixture()).unwrap();
    let project_dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: rom_path.clone(),
        platform: Platform::Synthetic,
        adapter_id: "synthetic.rtsf".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();

    // Sem working copy: export recusa com orientacao.
    let err = export_patch(&project_dir, None).unwrap_err();
    assert!(err.to_string().contains("reinsercao"), "{err}");

    let mut db = ProjectDb::open(&project_dir).unwrap();
    use romtranslate_core::adapters::rtsf::RtsfAdapter;
    let entries = RtsfAdapter
        .extract_structured(&fs::read(&rom_path).unwrap())
        .unwrap();
    db.upsert_entries(&entries).unwrap();
    db.update_translation("fixed-0", "SALVAR JOGO", TranslationStatus::Machine)
        .unwrap();
    db.update_translation(
        "reloc-0",
        "BEM-VINDO AO VILAREJO!",
        TranslationStatus::Machine,
    )
    .unwrap();
    drop(db);

    reinsert_project(&project_dir, false).unwrap();
    let outcome = export_patch(&project_dir, None).unwrap();

    // DoD: aplicar o patch exportado ao original == working copy, byte a byte.
    let original = fs::read(&rom_path).unwrap();
    let working = fs::read(project_dir.join("working/game.rtsf")).unwrap();
    let patch = fs::read(&outcome.patch_path).unwrap();
    assert_eq!(apply_ips(&original, &patch).unwrap(), working);

    // Manifest com os campos da spec §16.
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&outcome.manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["patch_format"], "IPS");
    assert_eq!(manifest["target_locale"], "pt-BR");
    assert_eq!(manifest["adapter"], "synthetic.rtsf");
    assert_eq!(manifest["translated_entries"], 2);
    assert_eq!(manifest["source_sha256"].as_str().unwrap().len(), 64);
    assert!(outcome.csv_path.exists());
    assert!(outcome
        .patch_path
        .to_string_lossy()
        .ends_with("game.pt-BR.ips"));
}

/// A promessa "roda no emulador": fluxo GBA conservador completo.
#[test]
fn gba_conservative_flow_extract_translate_reinsert_patch() {
    let rom = synth::make_gba_rom("SYNTHRPG");

    // Extracao estruturada: mesmas strings do scanner, agora com limite real.
    let entries = GbaAdapter.extract_structured(&rom).unwrap();
    let welcome = entries
        .iter()
        .find(|e| e.source_text == "WELCOME TO THE VILLAGE!")
        .expect("string plantada");
    assert_eq!(welcome.max_bytes, Some(23));

    // Traducao que cabe + uma no titulo do header (muda o checksum!).
    let mut translated = entries.clone();
    for e in translated.iter_mut() {
        e.translated_text = match e.source_text.as_str() {
            "WELCOME TO THE VILLAGE!" => Some("BEM-VINDO AO VILAREJO!".to_string()),
            "POTION" => Some("POCAO".to_string()),
            "SYNTHRPG" => Some("RPGSINT".to_string()), // titulo em 0xA0
            _ => None,
        };
    }
    let applied = GbaAdapter.apply_text(&rom, &translated).unwrap();
    assert_eq!(applied.report.applied, 3);

    // Header checksum recalculado: probe continua forte e verify passa.
    let verification = GbaAdapter.verify(&applied.bytes).unwrap();
    assert!(verification.ok, "{:?}", verification.problems);
    let reread = GbaAdapter.extract_structured(&applied.bytes).unwrap();
    assert!(reread
        .iter()
        .any(|e| e.source_text == "BEM-VINDO AO VILAREJO!"));
    assert!(reread.iter().any(|e| e.source_text == "POCAO"));

    // Overflow e drift recusam sem escrita parcial.
    let mut too_long = entries.clone();
    too_long[0].translated_text = Some("X".repeat(200));
    assert!(GbaAdapter.apply_text(&rom, &too_long).is_err());
    let mut drifted = entries.clone();
    drifted
        .iter_mut()
        .for_each(|e| e.translated_text = Some(e.source_text.clone()));
    let mut other_rom = rom.clone();
    other_rom[0x100] ^= 0xFF; // ROM nao e mais a mesma
    assert!(GbaAdapter.apply_text(&other_rom, &drifted).is_err());

    // Ponta a ponta com projeto real: reinsercao + patch.
    let tmp = TempDir::new("gba");
    let rom_path = tmp.0.join("game.gba");
    fs::write(&rom_path, &rom).unwrap();
    let project_dir = tmp.0.join("game.rtsproj");
    create_project(CreateProjectArgs {
        source_path: rom_path.clone(),
        platform: Platform::Gba,
        adapter_id: "gba.generic".into(),
        source_language: Some("en-US".into()),
        target_language: "pt-BR".into(),
        project_dir: project_dir.clone(),
    })
    .unwrap();
    let mut db = ProjectDb::open(&project_dir).unwrap();
    db.upsert_entries(&translated).unwrap();
    drop(db);

    let reinserted = reinsert_project(&project_dir, false).unwrap();
    assert!(reinserted.verification.ok);
    assert_eq!(fs::read(&rom_path).unwrap(), rom, "original intacto");

    let outcome = export_patch(&project_dir, None).unwrap();
    let working = fs::read(&reinserted.working_path).unwrap();
    let patch = fs::read(&outcome.patch_path).unwrap();
    assert_eq!(apply_ips(&rom, &patch).unwrap(), working);
    assert!(working.windows(22).any(|w| w == b"BEM-VINDO AO VILAREJO!"));
}

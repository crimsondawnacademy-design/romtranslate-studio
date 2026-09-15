//! Integracao do backend BPS: round-trips (incl. truncamento e >16 MiB, os
//! dois casos que o IPS nao cobre), validacao de CRC e export com formato.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::rtsf::RtsfAdapter;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::patch::{
    apply_bps, create_bps, export_patch, verify_bps_against, PatchFormat,
};
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
        let dir = std::env::temp_dir().join(format!("rts-bps-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn roundtrip(original: &[u8], modified: &[u8]) -> Vec<u8> {
    let patch = create_bps(original, modified).unwrap();
    assert_eq!(&apply_bps(original, &patch).unwrap(), modified);
    patch
}

#[test]
fn bps_roundtrip_covers_shapes_ips_cannot() {
    let base: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();

    // Identicos.
    roundtrip(&base, &base);

    // Mudancas no comeco/meio/fim.
    let mut m = base.clone();
    m[0] = 0xAA;
    m[2000] = 0xBB;
    let last = m.len() - 1;
    m[last] = 0xCC;
    roundtrip(&base, &m);

    // TRUNCAMENTO: target menor que o source (IPS nao representa).
    roundtrip(&base, &base[..1000]);
    roundtrip(&base, &[]);

    // Extensao.
    let mut ext = base.clone();
    ext.extend_from_slice(b"APPENDED");
    roundtrip(&base, &ext);

    // ALEM DE 16 MiB (o caso NDS real que estourava o IPS): 20 MiB com
    // mudanca no comeco e no fim.
    let big = vec![0x11u8; 20 << 20];
    let mut big_mod = big.clone();
    big_mod[5] = 0x99;
    let last = big_mod.len() - 1;
    big_mod[last] = 0x77;
    let patch = roundtrip(&big, &big_mod);
    assert!(
        patch.len() < 200,
        "patch linear compacto: {} bytes",
        patch.len()
    );
}

#[test]
fn bps_rejects_wrong_source_and_corruption() {
    let base = b"HELLO WORLD OF PATCHES".to_vec();
    let mut modified = base.clone();
    modified[0] = b'J';
    let patch = create_bps(&base, &modified).unwrap();

    // Source errado: erro claro dizendo que o patch e para outro arquivo.
    let mut other = base.clone();
    other[10] ^= 0xFF;
    let err = apply_bps(&other, &patch).unwrap_err();
    assert!(err.to_string().contains("OUTRO arquivo"), "{err}");

    // Patch corrompido no corpo.
    let mut corrupted = patch.clone();
    corrupted[6] ^= 0xFF;
    assert!(apply_bps(&base, &corrupted).is_err());

    // Truncado e lixo: erro limpo, sem panic.
    assert!(apply_bps(&base, &patch[..patch.len() - 5]).is_err());
    assert!(apply_bps(&base, b"BPS1").is_err());
    assert!(apply_bps(&base, b"NOTBPS_AT_ALL___").is_err());
}

#[test]
fn export_auto_picks_ips_for_small_and_forced_bps_works() {
    let tmp = TempDir::new("export");
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
    let mut db = ProjectDb::open(&project_dir).unwrap();
    let entries = RtsfAdapter
        .extract_structured(&fs::read(&rom_path).unwrap())
        .unwrap();
    db.upsert_entries(&entries).unwrap();
    db.update_translation("fixed-0", "SALVAR JOGO", TranslationStatus::Machine)
        .unwrap();
    drop(db);
    reinsert_project(&project_dir, false).unwrap();

    // Auto em fixture pequena que nao encolhe -> IPS (compatibilidade).
    let auto = export_patch(&project_dir, None).unwrap();
    assert_eq!(auto.patch_format, "IPS");
    assert!(auto.patch_path.to_string_lossy().ends_with(".ips"));

    // Forcado -> BPS aplicavel, com manifest coerente.
    let forced = export_patch(&project_dir, Some(PatchFormat::Bps)).unwrap();
    assert_eq!(forced.patch_format, "BPS");
    assert!(forced.patch_path.to_string_lossy().ends_with(".bps"));
    let original = fs::read(&rom_path).unwrap();
    let working = fs::read(project_dir.join("working/game.rtsf")).unwrap();
    let patch = fs::read(&forced.patch_path).unwrap();
    assert_eq!(apply_bps(&original, &patch).unwrap(), working);
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&forced.manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["patch_format"], "BPS");
}

#[test]
fn verify_bps_against_mirrors_apply_without_materializing() {
    let original: Vec<u8> = (0..40_000u32).map(|i| (i * 13 + 5) as u8).collect();
    let mut modified = original.clone();
    modified[1_000..1_020].copy_from_slice(b"TRADUZIDO AQUI MESMO");
    modified[30_000] ^= 0xFF;
    let patch = roundtrip(&original, &modified);

    // Target correto passa; qualquer divergencia reprova.
    verify_bps_against(&original, &patch, &modified).unwrap();
    let mut wrong = modified.clone();
    wrong[2_000] ^= 0x01;
    assert!(verify_bps_against(&original, &patch, &wrong).is_err());
    let mut truncated = modified.clone();
    truncated.pop();
    assert!(verify_bps_against(&original, &patch, &truncated)
        .unwrap_err()
        .to_string()
        .contains("bytes"));
    // Patch corrompido tambem reprova (CRC do proprio patch).
    let mut bad_patch = patch.clone();
    bad_patch[10] ^= 0xFF;
    assert!(verify_bps_against(&original, &bad_patch, &modified).is_err());
    // Divergencia so no ULTIMO trecho (regiao SourceRead final) tambem pega.
    let mut tail_wrong = modified.clone();
    let last = tail_wrong.len() - 1;
    tail_wrong[last] ^= 0x01;
    assert!(verify_bps_against(&original, &patch, &tail_wrong).is_err());
}

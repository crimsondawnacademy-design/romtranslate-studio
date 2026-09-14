//! Integracao do Sprint 2: scanners, tabela .tbl, export e reprodutibilidade.

use std::fs;
use std::path::PathBuf;

use romtranslate_core::export::{export_csv, export_json};
use romtranslate_core::scan::{scan_bytes, scan_file, ScanConfig, ScanEncoding};
use romtranslate_core::synth;
use romtranslate_core::tbl::TblTable;
use romtranslate_core::types::TextEntry;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-scan-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn config(encoding: ScanEncoding) -> ScanConfig {
    ScanConfig {
        encoding,
        ..ScanConfig::default()
    }
}

#[test]
fn ascii_scanner_reports_exact_offsets_and_bytes() {
    let mut data = vec![0xFFu8; 16];
    data.extend_from_slice(b"HELLO WORLD\0");
    data.extend_from_slice(&[0x01, 0x02]);
    data.extend_from_slice(b"HI\0"); // 2 chars: abaixo do minimo
    data.extend_from_slice(b"TRAILING TEXT"); // sem terminador, fim do buffer

    let out = scan_bytes(&data, &config(ScanEncoding::Ascii)).unwrap();
    assert_eq!(out.entries.len(), 2);

    let first = &out.entries[0];
    assert_eq!(first.offset, Some(16));
    assert_eq!(first.source_text, "HELLO WORLD");
    assert_eq!(first.original_bytes, b"HELLO WORLD");
    assert_eq!(first.metadata["terminated"], serde_json::json!(true));
    assert_eq!(first.id, "scan-00000010");

    let second = &out.entries[1];
    assert_eq!(second.source_text, "TRAILING TEXT");
    assert_eq!(second.metadata["terminated"], serde_json::json!(false));
}

#[test]
fn utf8_scanner_handles_accents_and_invalid_bytes() {
    let mut data = vec![0xC3u8]; // byte de continuacao orfao: invalido
    data.extend_from_slice("Poção Mágica".as_bytes());
    data.push(0x00);
    data.push(0xFF);

    let out = scan_bytes(&data, &config(ScanEncoding::Utf8)).unwrap();
    assert_eq!(out.entries.len(), 1);
    assert_eq!(out.entries[0].source_text, "Poção Mágica");
    assert_eq!(out.entries[0].offset, Some(1));
    assert_eq!(out.entries[0].original_bytes, "Poção Mágica".as_bytes());
}

#[test]
fn utf16_scanners_find_planted_strings() {
    for (le, enc) in [
        (true, ScanEncoding::Utf16Le),
        (false, ScanEncoding::Utf16Be),
    ] {
        let mut data = vec![0xFFu8; 8];
        for u in "MENU SCREEN".encode_utf16() {
            let b = if le { u.to_le_bytes() } else { u.to_be_bytes() };
            data.extend_from_slice(&b);
        }
        data.extend_from_slice(&[0x00, 0x00]);

        let out = scan_bytes(&data, &config(enc)).unwrap();
        assert_eq!(out.entries.len(), 1, "{enc:?}");
        assert_eq!(out.entries[0].source_text, "MENU SCREEN");
        assert_eq!(out.entries[0].offset, Some(8));
        assert_eq!(out.entries[0].original_bytes.len(), 22);
    }
}

#[test]
fn table_scan_uses_longest_match_and_custom_mapping() {
    let tmp = TempDir::new("tbl");
    let tbl_path = tmp.0.join("custom.tbl");
    fs::write(&tbl_path, "# comentario\n80=A\n81=B\nFF20= \n").unwrap();

    // "ABA A" na tabela custom, depois um byte sem mapeamento encerra.
    let data = [0x80, 0x81, 0x80, 0xFF, 0x20, 0x80, 0x42, 0x80];
    let cfg = ScanConfig {
        encoding: ScanEncoding::Table,
        tbl_path: Some(tbl_path),
        min_chars: 3,
        ..ScanConfig::default()
    };
    let out = scan_bytes(&data, &cfg).unwrap();
    assert_eq!(out.entries.len(), 1);
    assert_eq!(out.entries[0].source_text, "ABA A");
    assert_eq!(out.entries[0].original_bytes, &data[0..6]);
    match &out.entries[0].encoding {
        romtranslate_core::types::TextEncoding::Table(id) => assert_eq!(id, "custom"),
        other => panic!("encoding errado: {other:?}"),
    }
}

#[test]
fn tbl_parse_reports_line_numbers() {
    let err = TblTable::parse("80=A\nZZ=x\n").unwrap_err();
    assert!(err.to_string().contains("linha 2"), "{err}");
    assert!(TblTable::parse("").is_err());
    assert!(TblTable::parse("; so comentario\n").is_err());
}

#[test]
fn region_limits_the_scan_window() {
    let mut data = b"AAAA".to_vec();
    data.push(0);
    data.extend_from_slice(b"BBBB");
    let cfg = ScanConfig {
        region_start: Some(5),
        region_end: None,
        ..config(ScanEncoding::Ascii)
    };
    let out = scan_bytes(&data, &cfg).unwrap();
    assert_eq!(out.entries.len(), 1);
    assert_eq!(out.entries[0].source_text, "BBBB");
    assert_eq!(
        out.entries[0].offset,
        Some(5),
        "offset absoluto, nao relativo"
    );
    assert_eq!(out.scanned_bytes, 4);
}

#[test]
fn max_entries_truncates_and_flags() {
    let mut data = Vec::new();
    for _ in 0..30 {
        data.extend_from_slice(b"TEXT\0");
    }
    let cfg = ScanConfig {
        max_entries: 10,
        ..config(ScanEncoding::Ascii)
    };
    let out = scan_bytes(&data, &cfg).unwrap();
    assert_eq!(out.entries.len(), 10);
    assert!(out.truncated);
}

#[test]
fn gba_fixture_scan_is_reproducible_with_expected_strings() {
    let tmp = TempDir::new("gba");
    let rom_path = tmp.0.join("game.gba");
    fs::write(&rom_path, synth::make_gba_rom("SYNTHRPG")).unwrap();

    let ascii1 = scan_file(&rom_path, &config(ScanEncoding::Ascii)).unwrap();
    let ascii2 = scan_file(&rom_path, &config(ScanEncoding::Ascii)).unwrap();
    let texts: Vec<_> = ascii1
        .entries
        .iter()
        .map(|e| e.source_text.as_str())
        .collect();
    assert!(texts.contains(&"WELCOME TO THE VILLAGE!"), "{texts:?}");
    assert!(texts.contains(&"POTION"));
    assert!(texts.contains(&"HP {0}: 120"));
    // Deterministico: mesma config => mesmo resultado.
    assert_eq!(
        serde_json::to_string(&ascii1.entries).unwrap(),
        serde_json::to_string(&ascii2.entries).unwrap()
    );

    let utf16 = scan_file(&rom_path, &config(ScanEncoding::Utf16Le)).unwrap();
    assert!(utf16.entries.iter().any(|e| e.source_text == "SYNTH QUEST"));
}

#[test]
fn export_json_roundtrips_and_csv_escapes() {
    let tmp = TempDir::new("export");
    let data = b"SAY \"HI\", FRIEND\0".to_vec();
    let out = scan_bytes(&data, &config(ScanEncoding::Ascii)).unwrap();
    assert_eq!(out.entries.len(), 1);

    let json_path = tmp.0.join("strings.json");
    export_json(&out.entries, &json_path).unwrap();
    let back: Vec<TextEntry> =
        serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].source_text, out.entries[0].source_text);
    assert_eq!(back[0].original_bytes, out.entries[0].original_bytes);

    let csv_path = tmp.0.join("strings.csv");
    export_csv(&out.entries, &csv_path).unwrap();
    let csv = fs::read_to_string(&csv_path).unwrap();
    assert!(csv.starts_with("id,offset,encoding,byte_length,text\n"));
    assert!(csv.contains("\"SAY \"\"HI\"\", FRIEND\""), "{csv}");
    assert!(csv.contains("0x00000000"));
}

#[test]
fn scanners_never_panic_on_arbitrary_input() {
    let encodings = [
        ScanEncoding::Ascii,
        ScanEncoding::Utf8,
        ScanEncoding::Utf16Le,
        ScanEncoding::Utf16Be,
    ];
    for len in (0..256).step_by(7) {
        let data = synth::make_random(len, len as u64 + 1);
        for enc in encodings {
            scan_bytes(&data, &config(enc)).unwrap();
        }
    }
    // Regiao fora do arquivo nao panica nem estoura.
    let cfg = ScanConfig {
        region_start: Some(9999),
        region_end: Some(10_000),
        ..config(ScanEncoding::Ascii)
    };
    assert!(scan_bytes(&[1, 2, 3], &cfg).unwrap().entries.is_empty());
}

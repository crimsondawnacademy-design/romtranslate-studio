//! Arquivo de mensagens MPT0 dentro de um cartucho de DS: so ele e extraido,
//! e a traducao maior faz o arquivo ir pro fim do ROM com a FAT reapontada.

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::nds::{crc16, NdsAdapter};
use romtranslate_core::synth::make_nds_rom_with_mpt;

/// Traducoes do `make_nds_rom_with_mpt`, maiores que o original.
const TRANSLATIONS: [&str; 3] = [
    "Bob[@b]Bem-vindo a Santa Rosa, viajante![rn]Descanse bem esta noite.",
    "o [@3]um [@4]bastão[@5]os [@6]uns [@7]bastões",
    "*: [%0][%a000090] achou [%H0860][%X]uma moeda[%Y]moedas[%Z]!",
];

fn translated(rom: &[u8]) -> Vec<romtranslate_core::types::TextEntry> {
    let mut entries = NdsAdapter.extract(rom).unwrap().entries;
    for (entry, text) in entries.iter_mut().zip(TRANSLATIONS) {
        entry.translated_text = Some(text.to_string());
    }
    entries
}

fn used(rom: &[u8]) -> usize {
    u32::from_le_bytes(rom[0x80..0x84].try_into().unwrap()) as usize
}

#[test]
fn messages_grow_and_the_file_moves_to_the_end_of_the_rom() {
    let rom = make_nds_rom_with_mpt();
    let entries = NdsAdapter.extract(&rom).unwrap().entries;
    // So as tres mensagens com texto: nada do intro.txt nem do icone.
    let texts: Vec<&str> = entries.iter().map(|e| e.source_text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Bob[@b]Welcome to Santa Rosa![rn]Rest well.",
            "the [@3]a [@4]stick[@5]the [@6]some [@7]sticks",
            "*: [%0][%a000090] found [%H0860][%X]a coin[%Y]coins[%Z]!",
        ]
    );
    let entries = translated(&rom);
    let applied = NdsAdapter.apply_text(&rom, &entries).unwrap();
    assert_eq!(applied.report.applied, 3);
    assert!(
        applied.bytes.len() > rom.len(),
        "o arquivo cresceu e foi pro fim"
    );
    assert!(NdsAdapter.verify(&applied.bytes).unwrap().ok);

    let again = NdsAdapter.extract(&applied.bytes).unwrap().entries;
    let texts: Vec<&str> = again.iter().map(|e| e.source_text.as_str()).collect();
    assert_eq!(texts, TRANSLATIONS);
    // O header guarda o tamanho usado do ROM novo.
    assert_eq!(used(&applied.bytes), applied.bytes.len());
}

/// Com preenchimento (0xFF) depois do ultimo arquivo, como num cartucho de
/// verdade, o arquivo que cresceu vai ali e o ROM fica do mesmo tamanho.
#[test]
fn the_padding_after_the_last_file_is_used_before_the_rom_grows() {
    let mut rom = make_nds_rom_with_mpt();
    let end = rom.len().next_multiple_of(0x200);
    rom.resize(end + 0x4000, 0xFF);
    rom[0x80..0x84].copy_from_slice(&(end as u32).to_le_bytes());
    let crc = crc16(&rom[..0x15E]);
    rom[0x15E..0x160].copy_from_slice(&crc.to_le_bytes());

    let applied = NdsAdapter.apply_text(&rom, &translated(&rom)).unwrap();
    assert_eq!(applied.bytes.len(), rom.len());
    assert!(used(&applied.bytes) > end && used(&applied.bytes) < rom.len());
    assert_eq!(
        applied.bytes[0x14], rom[0x14],
        "a capacidade do chip nao muda"
    );
    let again = NdsAdapter.extract(&applied.bytes).unwrap().entries;
    let texts: Vec<&str> = again.iter().map(|e| e.source_text.as_str()).collect();
    assert_eq!(texts, TRANSLATIONS);
}

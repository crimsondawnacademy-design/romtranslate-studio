//! Arquivo de mensagens MPT0 dentro de um cartucho de DS: so ele e extraido,
//! e a traducao maior faz o arquivo ir pro fim do ROM com a FAT reapontada.

use romtranslate_core::adapter::GameAdapter;
use romtranslate_core::adapters::nds::NdsAdapter;
use romtranslate_core::synth::make_nds_rom_with_mpt;

#[test]
fn messages_grow_and_the_file_moves_to_the_end_of_the_rom() {
    let rom = make_nds_rom_with_mpt();
    let mut entries = NdsAdapter.extract(&rom).unwrap().entries;
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
    let translations = [
        "Bob[@b]Bem-vindo a Santa Rosa, viajante![rn]Descanse bem esta noite.",
        "o [@3]um [@4]bastão[@5]os [@6]uns [@7]bastões",
        "*: [%0][%a000090] achou [%H0860][%X]uma moeda[%Y]moedas[%Z]!",
    ];
    for (entry, text) in entries.iter_mut().zip(translations) {
        entry.translated_text = Some(text.to_string());
    }
    let applied = NdsAdapter.apply_text(&rom, &entries).unwrap();
    assert_eq!(applied.report.applied, 3);
    assert!(
        applied.bytes.len() > rom.len(),
        "o arquivo cresceu e foi pro fim"
    );
    assert!(NdsAdapter.verify(&applied.bytes).unwrap().ok);

    let again = NdsAdapter.extract(&applied.bytes).unwrap().entries;
    let texts: Vec<&str> = again.iter().map(|e| e.source_text.as_str()).collect();
    assert_eq!(texts, translations);
    // O header guarda o tamanho usado do ROM novo.
    let used = u32::from_le_bytes(applied.bytes[0x80..0x84].try_into().unwrap()) as usize;
    assert_eq!(used, applied.bytes.len());
}

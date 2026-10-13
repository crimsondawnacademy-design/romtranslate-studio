//! Texto japones em codigos de 16 bits na ordem do JIS X 0208: o codigo e a
//! posicao do caractere na tabela, `(ku - 1) * 94 + (ten - 1)`, gravado em
//! little-endian. Nao e Shift-JIS nem EUC — e a tabela achatada num numero
//! so, com os codigos de controle do jogo de 0xFF00 pra cima e 0xFFFF
//! fechando a mensagem.
//!
//! Medido em 13/10/2026 num RPG japones de GBA de 32 MiB (so contagens): o
//! scanner Shift-JIS acha 5 frases, todas mensagens de erro dos
//! programadores, e lendo assim saem as falas do jogo — numa regiao de
//! 392 KiB, 108 mil hiragana, 21 mil katakana e 20 mil ocorrencias de um
//! codigo fora da tabela (0x00AC) fazendo o papel do espaco entre palavras.
//!
//! O que sai daqui e texto de TABELA (`TextEncoding::Table(TABLE_ID)`), com
//! o `.tbl` gerado junto: validacao, traducao e reinsercao seguem o caminho
//! de qualquer tabela do projeto.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::OnceLock;

use encoding_rs::EUC_JP;

use crate::scan::is_sentence;
use crate::sjis::{is_kana, is_kanji, Script};
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

/// Id da tabela gerada (`<projeto>/tables/jis16.tbl`).
pub const TABLE_ID: &str = "jis16";

const CELLS: usize = 94;
/// O JIS X 0208 vai ate a linha 84.
const ROWS: usize = 84;
/// Onde comeca o nivel 2 do JIS (linha 48): os kanji raros.
const LEVEL_2: usize = 47 * CELLS;
/// De 0xFF00 pra cima a unidade e codigo de controle do jogo; 0xFFFF fecha a
/// mensagem.
const CONTROL: u16 = 0xFF00;
const END: u16 = 0xFFFF;
/// Frases (`scan::is_sentence`) pra dizer que o arquivo guarda texto assim.
// ponytail: corte fixo. O jogo medido passa de dezenas de milhares; ruido e
// um jogo ocidental de GBA dao zero. Um jogo pequeno com menos de cem frases
// nesta codificacao fica de fora — baixar quando aparecer um pra medir.
const MIN_SENTENCES: usize = 100;
/// O codigo fora da tabela que aparece entre dois kana ao menos isto e o
/// espaco do jogo.
const MIN_SPACES: u32 = 100;

/// Codigo -> caractere, 0 onde a tabela nao tem nada. Montada uma vez.
fn chars() -> &'static [u16] {
    static TABLE: OnceLock<Vec<u16>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = vec![0u16; ROWS * CELLS];
        for (code, slot) in table.iter_mut().enumerate() {
            // EUC-JP e o JIS X 0208 com 0xA0 somado a linha e a coluna.
            let pair = [0xA1 + (code / CELLS) as u8, 0xA1 + (code % CELLS) as u8];
            let (text, bad) = EUC_JP.decode_without_bom_handling(&pair);
            let mut found = text.chars();
            if let (Some(c), None, false) = (found.next(), found.next(), bad) {
                *slot = u16::try_from(c as u32).unwrap_or(0);
            }
        }
        // O espaco de largura inteira e o codigo 0: num cartucho isso e
        // preenchimento, nao texto.
        table[0] = 0;
        table
    })
}

/// O caractere do codigo `code`, se a tabela tem um ali.
pub fn decode(code: u16) -> Option<char> {
    let unit = *chars().get(code as usize)?;
    char::from_u32(unit as u32).filter(|_| unit != 0)
}

/// O codigo de `c` (o menor, se a tabela repete o caractere).
pub fn encode(c: char) -> Option<u16> {
    let unit = u16::try_from(c as u32).ok().filter(|u| *u != 0)?;
    chars().iter().position(|&u| u == unit).map(|i| i as u16)
}

fn token(code: u16) -> String {
    format!("[{code:04X}]")
}

/// O `.tbl` desta codificacao: uma linha por caractere (chave = os 2 bytes do
/// codigo, o menos significativo primeiro), os codigos de controle como
/// `[FFxx]` e o espaco. `space` = o codigo que o jogo usa como espaco entre
/// palavras; sem ele o espaco e o codigo 0 (o de largura inteira do JIS).
pub fn table_text(space: Option<u16>) -> String {
    let mut out = String::from(
        "# Tabela gerada pelo RomTranslate Studio: codigos de 16 bits na ordem do JIS X 0208\n\
         # (codigo = posicao do caractere na tabela, gravado em little-endian).\n\
         # Pode ser completada a mao: o app nao sobrescreve este arquivo.\n",
    );
    let space = space.unwrap_or(0);
    let _ = writeln!(out, "{:02X}{:02X}= ", space & 0xFF, space >> 8);
    for code in 0..(ROWS * CELLS) as u16 {
        if let Some(c) = decode(code).filter(|_| code != space) {
            let _ = writeln!(out, "{:02X}{:02X}={c}", code & 0xFF, code >> 8);
        }
    }
    for code in CONTROL..END {
        let _ = writeln!(out, "{:02X}FF={}", code & 0xFF, token(code));
    }
    out
}

/// Texto achado nesta codificacao.
pub struct Kuten16 {
    pub entries: Vec<TextEntry>,
    /// (posicao, bytes) de cada entry, em ordem: nenhuma outra leitura pode
    /// ficar com esses bytes.
    pub spans: Vec<(usize, usize)>,
    /// O `.tbl` que le e regrava as entries.
    pub table: String,
}

fn units(data: &[u8]) -> impl Iterator<Item = u16> + '_ {
    data.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
}

/// O codigo fora da tabela que mais aparece entre dois kana, se aparece
/// bastante: jogo que escreve em kana separa as palavras com espaco, e o
/// espaco dele nao precisa ser o do JIS.
fn find_space(data: &[u8]) -> Option<u16> {
    let kana = |code: u16| decode(code).is_some_and(is_kana);
    let mut between: HashMap<u16, u32> = HashMap::new();
    let mut window = [0u16; 2];
    for (i, code) in units(data).enumerate() {
        let [left, middle] = window;
        let odd = middle != 0 && middle < CONTROL && decode(middle).is_none();
        if i >= 2 && odd && kana(left) && kana(code) {
            *between.entry(middle).or_default() += 1;
        }
        window = [middle, code];
    }
    between
        .into_iter()
        .max_by_key(|&(code, count)| (count, code))
        .filter(|&(_, count)| count >= MIN_SPACES)
        .map(|(code, _)| code)
}

/// Uma unidade lida: caractere, codigo de controle ou nada disso.
enum Unit {
    Char(char),
    Control,
    Other,
}

fn read(code: u16, space: Option<u16>) -> Unit {
    if Some(code) == space {
        Unit::Char(' ')
    } else if (CONTROL..END).contains(&code) {
        Unit::Control
    } else {
        decode(code).map_or(Unit::Other, Unit::Char)
    }
}

/// As mensagens de `data` nesta codificacao, ou `None` se o arquivo nao
/// guarda texto assim (menos de `MIN_SENTENCES` frases). Cada entry vai do
/// primeiro ao ultimo caractere de uma sequencia de caracteres e codigos de
/// controle; os codigos do meio viram `[FFxx]` no texto, e os das pontas
/// ficam de fora (nao sao mexidos).
pub fn extract(data: &[u8]) -> Option<Kuten16> {
    let space = find_space(data);
    let codes: Vec<u16> = units(data).collect();
    let mut entries = Vec::new();
    let mut spans = Vec::new();
    let mut sentences = 0;
    let mut i = 0;
    while i < codes.len() {
        if !matches!(read(codes[i], space), Unit::Char(c) if c != ' ') {
            i += 1;
            continue;
        }
        // Do primeiro ao ultimo caractere que nao e espaco: controle e
        // espaco so entram no texto quando vem outro caractere depois (no
        // fim sao fecho de mensagem e preenchimento de campo).
        let begin = i;
        let mut end = i;
        let mut chars = 0;
        let mut script = Script::default();
        let mut text = String::new();
        let mut pending = String::new();
        while let Some(&code) = codes.get(i) {
            match read(code, space) {
                Unit::Char(' ') => pending.push(' '),
                Unit::Char(c) => {
                    for _ in pending.chars().filter(|p| *p == ' ') {
                        script.push(' ');
                        chars += 1;
                    }
                    text.push_str(&pending);
                    pending.clear();
                    text.push(c);
                    script.push(c);
                    chars += 1;
                    if is_kanji(c) && code as usize >= LEVEL_2 {
                        script.note_rare_kanji();
                    }
                    end = i + 1;
                }
                Unit::Control => pending.push_str(&token(code)),
                Unit::Other => break,
            }
            i += 1;
        }
        // Ate 3 caracteres so entre controles ou zeros: valor pequeno de
        // mapa de tiles vira "あい" o tempo todo.
        let edge = |at: Option<usize>| {
            at.and_then(|at| codes.get(at))
                .is_none_or(|&code| code == 0 || code >= CONTROL)
        };
        let anchored = chars > 3 || (edge(begin.checked_sub(1)) && edge(Some(end)));
        if !anchored || script.verdict() != Some(true) {
            continue;
        }
        sentences += usize::from(is_sentence(&text));
        let (offset, len) = (begin * 2, (end - begin) * 2);
        spans.push((offset, len));
        entries.push(TextEntry {
            id: format!("scan-{offset:08x}"),
            resource_path: None,
            offset: Some(offset as u64),
            original_bytes: data[offset..offset + len].to_vec(),
            source_text: text,
            translated_text: None,
            context: Some(
                "in-place: traducao limitada ao espaco original (cada letra ocupa 2 bytes)"
                    .to_string(),
            ),
            max_bytes: Some(len),
            encoding: TextEncoding::Table(TABLE_ID.to_string()),
            status: TranslationStatus::Untranslated,
            metadata: serde_json::json!({
                "scanner": TABLE_ID,
                "chars": chars,
                "terminated": codes.get(end) == Some(&0),
            }),
        });
    }
    (sentences >= MIN_SENTENCES).then(|| Kuten16 {
        entries,
        spans,
        table: table_text(space),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::make_random;

    #[test]
    fn codes_follow_the_jis_table() {
        // Linha 1: pontuacao; linha 2 abre com o losango; linha 4, hiragana.
        assert_eq!(decode(0x0001), Some('、'));
        assert_eq!(decode(0x0002), Some('。'));
        assert_eq!(decode(94), Some('◆'));
        assert_eq!(decode(3 * 94), Some('ぁ'));
        assert_eq!(decode(3 * 94 + 1), Some('あ'));
        assert_eq!(decode(4 * 94), Some('ァ'));
        // O codigo 0 (espaco de largura inteira) nao conta como texto, e a
        // linha 2 tem buracos.
        assert_eq!(decode(0), None);
        assert_eq!(decode(0x00AC), None);
        assert_eq!(decode(0xFF01), None);
        assert_eq!(decode((ROWS * CELLS) as u16), None);
        assert_eq!(encode('あ'), Some(3 * 94 + 1));
        assert_eq!(encode('ａ').and_then(decode), Some('ａ'));
        assert_eq!(encode('a'), None);
    }

    #[test]
    fn the_table_reads_and_writes_what_extract_reads() {
        let text = table_text(Some(0x00AC));
        assert!(text.contains("\n1B01=あ\n"), "hiragana");
        assert!(text.contains("\nAC00= \n"), "o espaco do jogo");
        assert!(text.contains("\n01FF=[FF01]\n"), "codigo de controle");
        assert!(!text.contains("\n0000="), "zero e preenchimento");
        assert!(!text.contains("FFFF="), "0xFFFF fecha a mensagem");
        let table = crate::tbl::TblTable::parse(&text).unwrap();
        assert_eq!(
            table.encode("あ [FF01]").unwrap(),
            [0x1B, 0x01, 0xAC, 0x00, 0x01, 0xFF]
        );
        // Sem espaco proprio, o espaco e o codigo 0.
        assert!(table_text(None).contains("\n0000= \n"));
    }

    #[test]
    fn noise_and_small_numbers_are_not_text() {
        assert!(extract(&make_random(1 << 20, 5)).is_none());
        // Rampa de valores pequenos, como um mapa de tiles: cai nos kana.
        let ramp: Vec<u8> = (0..40_000u32)
            .flat_map(|i| ((280 + i % 90) as u16).to_le_bytes())
            .collect();
        assert!(extract(&ramp).is_none());
        assert!(extract(&[]).is_none());
        assert!(extract(&[0x1B]).is_none());
    }
}

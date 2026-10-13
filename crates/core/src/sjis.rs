//! Shift-JIS: como a maioria dos jogos japoneses de disco (e parte dos de
//! cartucho) grava texto. Um byte para o ASCII (0x20-0x7E) e para o katakana
//! de meia largura (0xA1-0xDF); dois bytes para kana, kanji e simbolos do JIS
//! X 0208, com primeiro byte em 0x81-0x9F ou 0xE0-0xFC e segundo em 0x40-0x7E
//! ou 0x80-0xFC. A tabela e a do `encoding_rs` (o "Windows-31J" do padrao
//! WHATWG, com as extensoes da NEC e da IBM).

use std::sync::OnceLock;

use encoding_rs::SHIFT_JIS;

const TRAILS: usize = 0xFD - 0x40;

fn lead_index(byte: u8) -> Option<usize> {
    match byte {
        0x81..=0x9F => Some((byte - 0x81) as usize),
        0xE0..=0xFC => Some((byte - 0xE0) as usize + 31),
        _ => None,
    }
}

/// Par (primeiro byte, segundo byte) -> caractere, 0 onde nao ha. Montada uma
/// vez: o scanner consulta isto em cada posicao de um arquivo de gigabytes.
fn pairs() -> &'static [u16] {
    static TABLE: OnceLock<Vec<u16>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = vec![0u16; 60 * TRAILS];
        for lead in (0x81..=0x9Fu8).chain(0xE0..=0xFC) {
            for trail in (0x40..=0xFCu8).filter(|t| *t != 0x7F) {
                let pair = [lead, trail];
                let (text, bad) = SHIFT_JIS.decode_without_bom_handling(&pair);
                let mut chars = text.chars();
                let (Some(c), None, false) = (chars.next(), chars.next(), bad) else {
                    continue;
                };
                // Area de uso privado (0xF040+) nao e texto de ninguem.
                let Some(unit) = u16::try_from(c as u32)
                    .ok()
                    .filter(|u| !(0xE000..=0xF8FF).contains(u))
                else {
                    continue;
                };
                let index = lead_index(lead).unwrap_or(0) * TRAILS + (trail - 0x40) as usize;
                table[index] = unit;
            }
        }
        table
    })
}

/// O caractere que comeca em `data[i]` e quantos bytes ele ocupa.
pub fn decode_char(data: &[u8], i: usize) -> Option<(char, usize)> {
    let first = *data.get(i)?;
    match first {
        0x20..=0x7E => Some((first as char, 1)),
        0xA1..=0xDF => char::from_u32(0xFF61 + (first - 0xA1) as u32).map(|c| (c, 1)),
        _ => {
            let trail = *data.get(i + 1)?;
            let index = lead_index(first)? * TRAILS + (trail.checked_sub(0x40)? as usize);
            let unit = *pairs().get(index).filter(|_| trail <= 0xFC)?;
            char::from_u32(unit as u32)
                .filter(|_| unit != 0)
                .map(|c| (c, 2))
        }
    }
}

/// `text` em Shift-JIS. `Err` com o primeiro caractere que nao existe nele
/// (letra acentuada do portugues, por exemplo: o JIS X 0208 nao tem nenhuma).
pub fn encode(text: &str) -> Result<Vec<u8>, char> {
    let (bytes, _, unmappable) = SHIFT_JIS.encode(text);
    if !unmappable {
        return Ok(bytes.into_owned());
    }
    let mut buf = [0u8; 4];
    let missing = text
        .chars()
        .find(|c| SHIFT_JIS.encode(c.encode_utf8(&mut buf)).2);
    Err(missing.unwrap_or('\u{FFFD}'))
}

/// Hiragana ou katakana de largura inteira: a marca mais forte de que uma
/// sequencia e texto japones, e nao dado que por acaso forma kanji. So as
/// silabas: as marcas soltas dos dois blocos (゛ ゜ ゝ ゞ ・ ー ヽ ヾ) sao
/// sinais, e dado binario forma "、セ゛" e "・゛" o tempo todo.
pub fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}')
}

pub fn is_hiragana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}')
}

pub fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3005}')
}

/// Par de bytes de um kanji do nivel 2 do JIS X 0208 (0x989F em diante) ou
/// das extensoes: os 3,4 mil kanji raros, contra os 2.965 do nivel 1 (0x889F
/// a 0x9872) com que jogo escreve. Dado binario cai nos dois por igual.
pub fn is_rare_kanji(lead: u8, trail: u8) -> bool {
    (lead, trail) >= (0x98, 0x9F)
}

/// Os sinais das linhas 1 e 2 do JIS e parentes: pontuacao japonesa, as
/// marcas soltas dos blocos de kana (o traco de vogal longa entre elas), as
/// formas de largura inteira do ASCII, reticencias e travessao, setas, sinais
/// de conta, figuras (◆ ○ ★ ♪), letras gregas (nome de golpe: "サンダーβ")
/// e os simbolos de moeda. Fala de jogo e cheia
/// deles: no RPG japones medido, toda mensagem abre com "◆", e a regra que os
/// contava como caractere estranho deixava de fora as falas curtas — 5,5% do
/// kana do cartucho.
fn is_wide_sign(c: char) -> bool {
    matches!(
        c,
        '\u{3000}'..='\u{303F}'
            | '\u{3099}'..='\u{309F}'
            | '\u{30FB}'..='\u{30FF}'
            | '\u{FF01}'..='\u{FF5E}'
            | '\u{FFE0}'..='\u{FFE5}'
            | '\u{00A2}'..='\u{00B6}'
            | '\u{00D7}'
            | '\u{00F7}'
            | '\u{0391}'..='\u{03C9}'
            | '\u{2010}'..='\u{2312}'
            | '\u{25A0}'..='\u{25EF}'
            | '\u{2605}'..='\u{266F}'
    )
}

/// Ate que periodo `Script` mede repeticao.
const PERIODS: usize = 8;

/// O que uma sequencia tem de cada tipo de caractere, contado sem guardar o
/// texto: o scanner julga milhoes de sequencias e so monta a string das que
/// passam.
#[derive(Debug, Clone, Copy, Default)]
pub struct Script {
    chars: usize,
    kana: usize,
    kanji: usize,
    /// Pontuacao japonesa e formas de largura inteira.
    wide: usize,
    /// Espaco, o ASCII e o de largura inteira.
    spaces: usize,
    /// Letra, digito e a pontuacao ASCII que texto de jogo usa.
    plain: usize,
    /// Caracteres iguais ao anterior.
    repeats: usize,
    /// Caracteres vizinhos do anterior na tabela (o codigo seguinte ou o
    /// de antes).
    steps: usize,
    /// Caracteres a mesma distancia do anterior que o anterior estava do
    /// que veio antes dele: progressao de passo fixo.
    strides: usize,
    last_gap: Option<i64>,
    /// Os ultimos `PERIODS` caracteres e, pra cada periodo de 1 a `PERIODS`,
    /// quantos caracteres repetiram o que veio esse tanto antes.
    recent: [char; PERIODS],
    echoes: [usize; PERIODS],
    /// Kanji do nivel 2 do JIS, quando quem conta leu os bytes.
    rare: usize,
    last: Option<char>,
    first_kana: Option<char>,
    /// Ha pelo menos dois kana diferentes.
    varied: bool,
}

impl Script {
    pub fn push(&mut self, c: char) {
        for period in 1..=PERIODS.min(self.chars) {
            let before = self.recent[(self.chars - period) % PERIODS];
            self.echoes[period - 1] += usize::from(before == c);
        }
        self.recent[self.chars % PERIODS] = c;
        self.chars += 1;
        self.repeats += usize::from(self.last == Some(c));
        let gap = self.last.map(|last| c as i64 - last as i64);
        self.steps += usize::from(gap.is_some_and(|gap| gap.abs() == 1));
        self.strides += usize::from(gap.is_some() && gap == self.last_gap);
        self.last_gap = gap;
        self.last = Some(c);
        if is_kana(c) {
            self.kana += 1;
            self.varied |= self.first_kana.is_some_and(|first| first != c);
            self.first_kana.get_or_insert(c);
        } else if is_kanji(c) {
            self.kanji += 1;
        } else if c == ' ' || c == '\u{3000}' {
            self.spaces += 1;
        } else if is_wide_sign(c) {
            self.wide += 1;
        } else if c.is_ascii_alphanumeric()
            || matches!(
                c,
                '.' | ','
                    | '!'
                    | '?'
                    | ':'
                    | ';'
                    | '-'
                    | '\''
                    | '"'
                    | '('
                    | ')'
                    | '/'
                    | '%'
                    | '+'
                    | '&'
                    | '~'
            )
        {
            self.plain += 1;
        }
    }

    /// O caractere que acabou de entrar e kanji do nivel 2 do JIS (ver
    /// `is_rare_kanji`). So quem le os bytes sabe disso.
    pub fn note_rare_kanji(&mut self) {
        self.rare += 1;
    }

    /// Ver `looks_japanese`.
    pub fn verdict(&self) -> Option<bool> {
        let (n, script) = (self.chars, self.kana + self.kanji);
        if script == 0 {
            return None;
        }
        // So escrita japonesa, pontuacao dela e espaco.
        let pure = script + self.wide + self.spaces == n;
        // Katakana de meia largura, grego, cirilico, traco de caixa, simbolo
        // ASCII solto: o que a tabela tem e texto de jogo quase nunca usa.
        let odd = n - script - self.wide - self.spaces - self.plain;
        Some(if n <= 3 {
            self.varied && self.kana + self.wide == n
        } else {
            self.varied
                && self.kana * 4 >= script
                && script * 3 >= n - self.wide - self.spaces
                && self.repeats * 2 <= n
                && self.steps * 2 <= n
                && self.strides * 2 <= n
                && (n < 12 || self.echoes.iter().all(|echo| echo * 5 <= n * 3))
                && self.rare * 4 <= self.kanji
                && odd * 10 <= n
                && (odd == 0 || n >= 10)
                && (self.kana >= 3 || pure)
        })
    }
}

/// `Some` quando `text` tem escrita japonesa (kana ou kanji): parece texto de
/// jogo, e nao bytes que por acaso caem na tabela? `None` = nao tem escrita
/// japonesa, quem decide e a regra do alfabeto latino.
///
/// Dado binario lido como Shift-JIS vira kanji o tempo todo (um par de bytes
/// qualquer cai na tabela 1 vez a cada 9) e quase nunca vira kana (2% da
/// tabela). Por isso a regra pede kana, e pede dois diferentes:
/// - ate 3 caracteres: so kana e pontuacao japonesa ("はい", "いいえ",
///   "セーブ"). Um kana com letra ASCII do lado ("あAB") aparece em ruido
///   uma vez a cada poucos KB;
/// - 4 ou mais: kana em pelo menos um quarto da escrita japonesa; escrita
///   japonesa em pelo menos um terco do que nao e sinal nem espaco (fala
///   curta e cheia de sinal: "◆、さん・・・。"); 3+ kana, ou entao nada alem
///   de escrita japonesa, pontuacao dela e espaco; no maximo metade dos
///   caracteres repetindo o anterior (mapa de tiles de 16 bits vira "ァァァァ"
///   por milhares de caracteres), e no maximo metade sendo o vizinho do
///   anterior na tabela (uma tabela de valores em sequencia vira
///   "ァアィイゥウェエ": num RPG japones de GBA de 32 MiB eram quase todas as
///   303 sequencias que passavam, contra 5 frases de verdade), o mesmo pra
///   progressao de passo fixo ("ぉこそでぱほゅわ" anda de 10 em 10); de 12
///   caracteres pra cima, no maximo 6
///   em cada 10 repetindo o que veio 1 a 8 posicoes antes (mapa de tiles
///   repete em periodo curto: "ぜそせぞぜそせぞ"); no maximo um quarto dos
///   kanji no nivel 2
///   do JIS (os raros, metade da tabela: so quem le os bytes confere isto);
///   e quase nada do que a tabela tem e jogo
///   nao usa (katakana de meia largura, grego, cirilico, simbolo ASCII
///   solto): nenhum ate 9 caracteres, 1 em cada 10 dali pra cima.
///
/// Medido em 13/10/2026 (so contagens): 16 MiB de ruido dao 40 falsos
/// positivos com a regra frouxa (2 kana e pronto); um jogo ocidental de GBA
/// de 16 MiB tem 119 sequencias que passavam, 28 delas nomes de recurso
/// deixados pelos desenvolvedores (japones de verdade, mas nao texto do
/// jogo) e o resto tabela de fonte e mapa de tiles. Todas caem fora das
/// regioes de texto (`scan::stray_ids`), e nenhuma frase do jogo se perde.
// ponytail: palavra so de kanji ("装備", "魔法") fica de fora — sem kana nao
// da pra separar de lixo so pela aparencia. E o que falta medir num jogo
// japones de verdade: calibrada so em arquivo sintetico e, contra falso
// positivo, em ruido e num jogo ocidental de GBA.
pub fn looks_japanese(text: &str) -> Option<bool> {
    let mut script = Script::default();
    text.chars().for_each(|c| script.push(c));
    script.verdict()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::make_random;

    #[test]
    fn decodes_what_it_encodes() {
        let text = "こんにちは、旅人さん。HPが 30 かいふくした！";
        let bytes = encode(text).unwrap();
        let mut i = 0;
        let mut back = String::new();
        while let Some((c, len)) = decode_char(&bytes, i) {
            back.push(c);
            i += len;
        }
        assert_eq!((back.as_str(), i), (text, bytes.len()));
        // Valores conferidos na tabela do JIS X 0208: "あ" = 0x82A0, "ア" = 0x8341.
        assert_eq!(encode("あア").unwrap(), [0x82, 0xA0, 0x83, 0x41]);
        assert_eq!(decode_char(&[0xB1], 0), Some(('ｱ', 1)));
    }

    #[test]
    fn rejects_what_is_not_a_character() {
        assert_eq!(decode_char(&[], 0), None);
        assert_eq!(decode_char(&[0x82], 0), None); // primeiro byte sem o segundo
        assert_eq!(decode_char(&[0x82, 0x30], 0), None); // segundo byte invalido
        assert_eq!(decode_char(&[0x82, 0x7F], 0), None);
        assert_eq!(decode_char(&[0x82, 0xFD], 0), None);
        assert_eq!(decode_char(&[0xF0, 0x40], 0), None); // uso privado
        assert_eq!(decode_char(&[0x80, 0x41], 0), None);
        assert_eq!(decode_char(&[0x1F], 0), None);
        for seed in 1..40 {
            let noise = make_random(512, seed);
            for i in 0..noise.len() {
                let _ = decode_char(&noise, i);
            }
        }
    }

    #[test]
    fn accented_letters_are_named_not_swallowed() {
        assert_eq!(encode("Ola, viajante!").unwrap(), b"Ola, viajante!");
        assert_eq!(encode("Olá"), Err('á'));
        assert_eq!(encode("coração"), Err('ç'));
    }

    #[test]
    fn kana_is_what_separates_text_from_bytes_that_form_kanji() {
        for text in [
            "はい",
            "いいえ",
            "セーブ",
            "アイテム",
            "むらへ ようこそ",
            "HPが 30 かいふくした！",
            "Lv 5 になった",
            "最大HP回復薬を手に入れた",
            "薬草を使う",
            "はい    ",
            "わくわく どきどき してきた",
            "はい はい はい はい",
            // Fala curta com os sinais de sempre: losango, reticencias, nota.
            "◆いうよ。",
            "◇はか。",
            "◆（ククッ・・・）",
            "ありがとう…",
            "★やったね♪",
            "ＰＫサンダーβ",
            "◆、さん・・・。",
        ] {
            assert_eq!(looks_japanese(text), Some(true), "{text}");
        }
        for text in [
            "装備",
            "魔法使",
            "漢字仝亜唖",
            "ｱ亜B3",
            "あ",
            "亜あ",
            "ぁ#$%&'()",
            // O que ruido e dado em sequencia formam (medido em 13/10/2026).
            "あAB",
            "ABCDEFGHあい",
            "頃パ錫パｽ",
            "Z4ぱぽ0潟",
            "}qヤプ",
            "ァァァァァァブァァァァァァシジァァァァァァ",
            "ァアィイゥウェエォオカガキクグケゲコゴ",
            "ミマポボホペベヘプブフピビヒパバハノネヌ",
            "、セ゛",
            "ぉこそでぱほゅわ",
            "ぜそせぞぜそせぞぜそせぞぜそせぞ",
            "きぎｅくきぎｅくきぎｅくおかｅがおかｅがおかｅが",
        ] {
            assert_eq!(looks_japanese(text), Some(false), "{text}");
        }
        assert_eq!(looks_japanese("NEW GAME"), None);
        // So marcas soltas: nao ha silaba nenhuma, quem julga e a regra geral.
        for marks in ["ーー", "・゛"] {
            assert_eq!(looks_japanese(marks), None, "{marks}");
            assert!(!crate::scan::looks_like_text(marks), "{marks}");
        }
        // "亜" abre o nivel 1 (0x889F) e "弌" o nivel 2 (0x989F).
        assert_eq!(
            encode("亜腕弌").unwrap(),
            [0x88, 0x9F, 0x98, 0x72, 0x98, 0x9F]
        );
        assert!(!is_rare_kanji(0x88, 0x9F) && !is_rare_kanji(0x98, 0x72));
        assert!(is_rare_kanji(0x98, 0x9F) && is_rare_kanji(0xE0, 0x40));
        assert_eq!(looks_japanese("ｱｲﾃﾑ"), None);
    }
}

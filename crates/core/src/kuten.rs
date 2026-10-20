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

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::OnceLock;

use encoding_rs::{EUC_JP, SHIFT_JIS};

use crate::scan::is_sentence;
use crate::sjis::{is_kana, is_kanji, Script};
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

/// Id da tabela gerada (`<projeto>/tables/jis16.tbl`).
pub const TABLE_ID: &str = "jis16";

const CELLS: usize = 94;
/// O JIS X 0208 vai ate a linha 84.
const ROWS: usize = 84;
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
/// Texto de jogo vem em fila: `MIN_GROUP` sequencias ou mais, cada uma a no
/// maximo `NEIGHBOR_GAP` bytes da anterior — o roteiro, a tabela de nomes de
/// itens, a de titulos de musica. Valor pequeno que por acaso cai nos kana
/// aparece sozinho, e sozinha so fica a sequencia longa (`ALONE_CHARS`
/// caracteres, metade deles hiragana). No jogo medido, a tabela de titulos
/// (240 sequencias curtas) ficava fora das regioes de texto de
/// `scan::stray_ids`, bloco sem frase nenhuma; e 118 sequencias avulsas de 6
/// a 12 caracteres, em regioes de dado, passavam por frase.
const NEIGHBOR_GAP: usize = 128;
const MIN_GROUP: usize = 3;
const ALONE_CHARS: usize = 16;
/// Usos pra julgar um comando (`find_params`) ou a marca de fala
/// (`find_bullet`).
const MIN_USES: u32 = 15;
/// Registros seguidos de mesma largura pra valer como tabela de campos fixos
/// (`record_slots`).
const MIN_RECORDS: usize = 8;
/// Registro mais largo que isto (em unidades) nao e campo de nome ou item.
const MAX_RECORD: usize = 64;

/// Um comando leva ate isto de parametros. Numero em mais posicoes seguidas
/// do que isto nao e parametro: e texto de outro alfabeto.
const MAX_PARAMS: usize = 4;
/// Abaixo disto, codigo sem caractere na tabela e desenho proprio do jogo.
const GLYPHS: u16 = (ROWS * CELLS) as u16;
/// As figuras do fim da linha 1 e do comeco da linha 2 do JIS, de "☆" a "〓":
/// e com uma delas que jogo marca o comeco de uma fala.
const SHAPES: std::ops::RangeInclusive<u16> = 88..=107;
/// Uma mensagem nao passa disto (em unidades): o que passa e dado.
const MAX_SPAN: usize = 16 * 1024;

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

/// Uma fonte guardada com o codigo Shift-JIS de cada desenho tem pelo menos
/// isto de desenhos: menos que isso e tabela de outra coisa.
const MIN_FONT: usize = 2000;

/// Uma fonte guardada com o codigo Shift-JIS de cada desenho na frente: o
/// desenho `k` mora em `start + k * stride`, os 2 bytes do codigo e depois
/// `stride - 2` bytes de desenho.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FontTable {
    pub start: usize,
    pub stride: usize,
    pub count: usize,
}

impl FontTable {
    /// Os 2 bytes do codigo Shift-JIS do desenho `k`.
    pub fn code<'a>(&self, data: &'a [u8], k: usize) -> Option<&'a [u8]> {
        let at = self.start + k * self.stride;
        (k < self.count).then(|| data.get(at..at + 2)).flatten()
    }

    /// Os bytes do desenho `k`, sem o codigo.
    pub fn glyph<'a>(&self, data: &'a [u8], k: usize) -> Option<&'a [u8]> {
        let at = self.start + k * self.stride + 2;
        (k < self.count)
            .then(|| data.get(at..at + self.stride - 2))
            .flatten()
    }
}

/// As fontes que o arquivo guarda com o codigo Shift-JIS de cada desenho na
/// frente, a maior primeiro (no empate, a que vem antes no arquivo). Cada uma
/// comeca em "　、。" (81 40, 81 41, 81 42), um desenho a cada `stride`
/// bytes, codigos sempre crescendo.
pub(crate) fn fonts(data: &[u8]) -> Vec<FontTable> {
    let lead = |a: u8, b: u8| {
        matches!(a, 0x81..=0x9F | 0xE0..=0xEF) && (0x40..=0xFC).contains(&b) && b != 0x7F
    };
    let mut found = Vec::new();
    for (at, _) in data
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| *pair == [0x81, 0x40])
    {
        for stride in 4..=256 {
            let next = |k: usize| data.get(at + k * stride..at + k * stride + 2);
            if next(1) != Some(&[0x81, 0x41]) || next(2) != Some(&[0x81, 0x42]) {
                continue;
            }
            let (mut count, mut last) = (0, 0u16);
            while let Some(&[a, b]) = next(count) {
                let code = u16::from_be_bytes([a, b]);
                if !lead(a, b) || (count > 0 && code <= last) {
                    break;
                }
                (count, last) = (count + 1, code);
            }
            if count >= MIN_FONT {
                found.push(FontTable {
                    start: at,
                    stride,
                    count,
                });
            }
        }
    }
    // Estavel: no empate fica a ordem do arquivo.
    found.sort_by_key(|font| std::cmp::Reverse(font.count));
    found
}

/// A ordem dos caracteres na fonte do proprio jogo, quando o cartucho guarda
/// cada desenho com o codigo Shift-JIS dele na frente (`fonts`, a maior): o
/// codigo do texto e a posicao do desenho na fonte. Codigo -> caractere, 0
/// onde nao ha um.
///
/// Medido em 16/10/2026 no RPG japones de 32 MiB: duas fontes assim (7.332
/// desenhos de 22 bytes e 7.332 de 10), em ordem JIS mas SEM seis das
/// linhas vazias — o kana bate com a tabela padrao, e todo kanji fica 564
/// posicoes antes dela (父 era lido 茶, "中断" saia "醇叔"). A fonte so vale se o
/// kana dela bate com o da tabela padrao, que e como o texto foi reconhecido.
pub(crate) fn font_order(data: &[u8], fonts: &[FontTable]) -> Option<Vec<u16>> {
    let FontTable {
        start,
        stride,
        count,
    } = *fonts.first()?;
    let mut font: Vec<u16> = (0..count)
        .map(|i| {
            let at = start + i * stride;
            let (text, bad) = SHIFT_JIS.decode_without_bom_handling(&data[at..at + 2]);
            let mut found = text.chars();
            match (found.next(), found.next(), bad) {
                (Some(c), None, false) => u16::try_from(c as u32).unwrap_or(0),
                _ => 0,
            }
        })
        .collect();
    // O espaco de largura inteira: preenchimento, como na tabela padrao.
    font[0] = 0;
    let (mut kana, mut same) = (0, 0);
    for (code, &unit) in font.iter().enumerate().take(8 * CELLS) {
        if let Some(c) = decode(code as u16).filter(|c| is_kana(*c)) {
            kana += 1;
            same += usize::from(unit == c as u16);
        }
    }
    (kana > 0 && same * 100 >= kana * 95).then_some(font)
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

/// Um codigo como aparece no texto, e o comando com os parametros dele numa
/// peca so: `[FF01]`, `[FF08:0003]`.
fn token_of(units: &[u16]) -> String {
    let units: Vec<String> = units.iter().map(|u| format!("{u:04X}")).collect();
    format!("[{}]", units.join(":"))
}

fn key(code: u16) -> String {
    format!("{:02X}{:02X}", code & 0xFF, code >> 8)
}

/// `token` e um desenho proprio do jogo (`[045C]`: caractere da fonte dele num
/// codigo que a tabela deixa vazio, quase sempre kanji)? E texto do original,
/// nao comando: a traducao nao precisa repetir.
pub fn is_glyph_token(token: &str) -> bool {
    let code = token
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .filter(|hex| hex.len() == 4)
        .and_then(|hex| u16::from_str_radix(hex, 16).ok());
    code.is_some_and(|code| code < CONTROL)
}

/// O que o arquivo mostrou do jeito que ESTE jogo usa a codificacao.
#[derive(Debug, Default)]
struct Dialect {
    /// O codigo que o jogo usa como espaco entre palavras, se tem um fora da
    /// tabela (`find_space`).
    space: Option<u16>,
    /// Quantos parametros leva cada comando que aparece o bastante pra medir
    /// (`find_params`). Vazio = nada medido ainda (a primeira leitura).
    params: HashMap<u16, usize>,
    /// As figuras com que as falas abrem (`find_bullets`).
    bullets: HashSet<u16>,
    /// A ordem da fonte do jogo (`font_order`), se o cartucho a guarda; sem
    /// ela, a tabela padrao.
    font: Option<Vec<u16>>,
    /// Alfabetos ensinados pelo projeto, que valem depois de um comando ate o
    /// fim da mensagem (`ExtractHints::alphabets`).
    switches: HashMap<u16, HashMap<u16, char>>,
    /// Comandos cujo ultimo parametro e uma letra (`find_lettered`): a letra
    /// e lida como texto, e a traducao pode troca-la.
    lettered: HashSet<u16>,
    /// Letras acentuadas que a reinsercao desenha em codigos livres da fonte
    /// do jogo (`images::letters::choose`): entram na tabela no lugar do
    /// `[xxxx]` do codigo.
    letters: HashMap<u16, char>,
    /// Pares de letras que a reinsercao desenha em codigos que o texto do
    /// jogo nao usa (`images::letters::choose_pairs`): entram na tabela no
    /// lugar do kanji (ou do `[xxxx]`) do codigo.
    pairs: HashMap<u16, String>,
    /// Pares que a copia lida ja tem desenhados (a de trabalho, na
    /// auditoria): lidos como as 2 letras, nao como o kanji que o codigo
    /// Shift-JIS da fonte ainda diz.
    drawn_pairs: HashMap<u16, String>,
    /// Onde cada um vale: (unidade do comando, unidade do fim da mensagem),
    /// em ordem. So dentro de mensagem de banco (`honored`): fora dela o
    /// comando e um numero qualquer, e os numeros pequenos depois dele
    /// virariam kana.
    shifted: Vec<(usize, usize)>,
    /// As mensagens que a tabela de um banco aponta: (comeco, fim), em
    /// unidades e em ordem (`messages`).
    known: Vec<(usize, usize)>,
}

impl Dialect {
    /// O caractere do codigo `code` neste jogo. O par desenhado
    /// (`drawn_pairs`) conta como uma letra; o texto dele e o de `text_of`.
    fn decode(&self, code: u16) -> Option<char> {
        if self.drawn_pairs.contains_key(&code) {
            return Some('ａ');
        }
        match &self.font {
            Some(font) => font
                .get(code as usize)
                .filter(|unit| **unit != 0)
                .and_then(|unit| char::from_u32(*unit as u32)),
            None => decode(code),
        }
    }

    /// O alfabeto que vale na unidade `i`, se ela esta depois de um comando de
    /// troca (`shifted`).
    fn alphabet_at(&self, codes: &[u16], i: usize) -> Option<&HashMap<u16, char>> {
        let k = self
            .shifted
            .partition_point(|&(at, _)| at < i)
            .checked_sub(1)?;
        let (at, end) = self.shifted[k];
        (i < end).then(|| self.switches.get(&codes[at])).flatten()
    }

    /// O comando em `codes[i]` desenha a letra do ultimo parametro: o de
    /// `lettered` sempre, e o de dois parametros ou mais quando o ultimo e
    /// kana — o nome esticado (`FF26` no jogo medido) fecha com o caractere
    /// do ultimo, ！ ou 。 quase sempre, e ッ justo na primeira fala do jogo.
    fn draws_letter(&self, codes: &[u16], i: usize) -> bool {
        let Some(&n) = self.params.get(&codes[i]).filter(|&&n| n <= MAX_PARAMS) else {
            return false;
        };
        self.lettered.contains(&codes[i])
            || (n >= 2
                && codes
                    .get(i + n)
                    .and_then(|&unit| self.decode(unit))
                    .is_some_and(is_kana))
    }

    /// A unidade `i` esta dentro de uma mensagem que a tabela de um banco
    /// aponta.
    fn in_message(&self, i: usize) -> bool {
        let k = self.known.partition_point(|&(begin, _)| begin <= i);
        k > 0 && i < self.known[k - 1].1
    }

    /// Ate onde vao os desenhos da fonte: abaixo disto, codigo sem caractere
    /// e desenho proprio do jogo.
    fn glyphs(&self) -> u16 {
        self.font
            .as_ref()
            .map_or(GLYPHS, |font| u16::try_from(font.len()).unwrap_or(CONTROL))
    }
}

/// O `.tbl` desta codificacao: uma linha por caractere (chave = os 2 bytes do
/// codigo, o menos significativo primeiro), o espaco, os codigos de controle
/// como `[FFxx]`, cada comando com os parametros dele (`commands`) como
/// `[FFxx:pppp]` e, como `[xxxx]`, os codigos em que a tabela padrao nao tem nada — e onde
/// o jogo poe os desenhos dele (kanji proprio, icone). `space` = o codigo que
/// o jogo usa como espaco entre palavras; sem ele o espaco e o codigo 0 (o de
/// largura inteira do JIS).
pub fn table_text(space: Option<u16>, commands: &BTreeSet<Vec<u16>>) -> String {
    table_with(
        &Dialect {
            space,
            ..Dialect::default()
        },
        commands,
    )
}

/// `table_text` na ordem da fonte do jogo, se o dialeto tem uma.
fn table_with(dialect: &Dialect, commands: &BTreeSet<Vec<u16>>) -> String {
    let space = dialect.space;
    let mut out = String::new();
    if dialect.font.is_some() {
        out.push_str(
            "# Ordem da fonte do proprio jogo, achada no arquivo (cada desenho guardado com o\n\
             # codigo Shift-JIS dele): o codigo e a posicao do desenho na fonte.\n",
        );
    }
    out.push_str(
        "# Tabela gerada pelo RomTranslate Studio: codigos de 16 bits na ordem do JIS X 0208\n\
         # (codigo = posicao do caractere na tabela, gravado em little-endian).\n\
         # [FFxx] e comando do jogo; [FFxx:pppp], comando com os parametros dele.\n\
         # [xxxx] e um desenho proprio do jogo (kanji, icone) num codigo que a tabela\n\
         # padrao deixa vazio: troque pelo caractere certo se souber qual e.\n\
         # Pode ser completada a mao: a extracao regrava o arquivo e poe no fim as\n\
         # linhas que voce mudou, que valem por cima das geradas.\n",
    );
    if !dialect.pairs.is_empty() {
        out.push_str(
            "# Pares de letras (\"de\", \"a \"): desenhados pela reinsercao em codigos que\n\
             # o texto do jogo nao usa, 2 letras por codigo. A lista vem de\n\
             # tables/jis16.pares.json (romtranslate pares).\n",
        );
    }
    let space = space.unwrap_or(0);
    let _ = writeln!(out, "{}= ", key(space));
    for code in 1..dialect.glyphs() {
        if code == space {
            continue;
        }
        if let Some(pair) = dialect.pairs.get(&code) {
            let _ = writeln!(out, "{}={pair}", key(code));
            continue;
        }
        match dialect.letters.get(&code).copied().or(dialect.decode(code)) {
            Some(c) => {
                let _ = writeln!(out, "{}={c}", key(code));
            }
            None => {
                let _ = writeln!(out, "{}={}", key(code), token_of(&[code]));
            }
        }
    }
    for code in CONTROL..END {
        let _ = writeln!(out, "{}={}", key(code), token_of(&[code]));
    }
    for units in commands {
        let keys: String = units.iter().map(|u| key(*u)).collect();
        let _ = writeln!(out, "{keys}={}", token_of(units));
    }
    out
}

/// Onde vale cada troca de alfabeto de `switches`: (comando, fim da
/// mensagem), so quando a mensagem comeca onde a tabela de um banco aponta
/// (`starts`, em unidades) — a do comeco de uma sala vem logo depois do
/// cabecalho (fim de mensagem e a quantidade).
fn honored(
    codes: &[u16],
    starts: &HashSet<usize>,
    switches: &HashMap<u16, HashMap<u16, char>>,
) -> Vec<(usize, usize)> {
    if switches.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, code) in codes.iter().enumerate() {
        if !switches.contains_key(code) {
            continue;
        }
        let back = i.saturating_sub(MAX_SPAN);
        let Some(begin) = (back..i).rev().find(|&k| codes[k] == END).map(|k| k + 1) else {
            continue;
        };
        if !(starts.contains(&begin) || starts.contains(&(begin + 1))) {
            continue;
        }
        let end = (i..codes.len().min(i + MAX_SPAN))
            .find(|&k| codes[k] == END)
            .unwrap_or(i + 1);
        out.push((i, end));
    }
    out
}

/// As mensagens que comecam em `starts` (unidades): (comeco, fim), o fim no
/// primeiro 0xFFFF, em ordem.
fn messages(codes: &[u16], starts: &HashSet<usize>) -> Vec<(usize, usize)> {
    let mut known: Vec<(usize, usize)> = starts
        .iter()
        .filter_map(|&begin| {
            let end = (begin..codes.len().min(begin + MAX_SPAN)).find(|&k| codes[k] == END)?;
            Some((begin, end))
        })
        .collect();
    known.sort_unstable();
    known
}

/// A letra com o sinal de sonorizacao (゛) ou de semissonorizacao (゜), se
/// existe: て゛ = で, は゜ = ぱ.
fn voiced(base: char, mark: char) -> Option<char> {
    let step = match mark {
        '゛' => 1,
        '゜' => 2,
        _ => return None,
    };
    if base == 'う' && step == 1 {
        return Some('ゔ');
    }
    let rows = if step == 1 {
        "かきくけこさしすせそたちつてとはひふへほカキクケコサシスセソタチツテトハヒフヘホ"
    } else {
        "はひふへほハヒフヘホ"
    };
    rows.contains(base)
        .then(|| char::from_u32(base as u32 + step))
        .flatten()
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

fn units(data: &[u8]) -> Vec<u16> {
    data.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect()
}

fn is_script(code: u16) -> bool {
    decode(code).is_some_and(|c| is_kana(c) || is_kanji(c))
}

/// O codigo fora da tabela que mais aparece entre dois kana, se aparece
/// bastante: jogo que escreve em kana separa as palavras com espaco, e o
/// espaco dele nao precisa ser o do JIS.
fn find_space(codes: &[u16]) -> Option<u16> {
    let kana = |code: u16| decode(code).is_some_and(is_kana);
    let mut between: HashMap<u16, u32> = HashMap::new();
    for w in codes.windows(3) {
        let odd = w[1] != 0 && w[1] < CONTROL && decode(w[1]).is_none();
        if odd && kana(w[0]) && kana(w[2]) {
            *between.entry(w[1]).or_default() += 1;
        }
    }
    between
        .into_iter()
        .max_by_key(|&(code, count)| (count, code))
        .filter(|&(_, count)| count >= MIN_SPACES)
        .map(|(code, _)| code)
}

/// Quantos parametros leva cada comando com `MIN_USES` usos ou mais: as
/// posicoes seguidas, logo depois dele, em que quase nunca (menos de 1 em 10)
/// vem algo que continua um texto. Depois da quebra de linha vem texto; depois
/// de "toca o som n" vem o n. Outro comando na posicao nao conta nem pra um
/// lado nem pro outro: comando segue comando o tempo todo. A marca de fala
/// continua um texto (`Dialect::goes_on`): sem ela na conta, o comando que
/// vem sempre antes de uma fala ficava com a marca como parametro. So contam os usos
/// dentro das mensagens de `spans` e logo antes delas: fora do texto, 0xFFxx e
/// um numero qualquer, seguido de qualquer coisa.
///
/// Medido no jogo real: `FF01` (quebra de linha, 11 mil usos) e seguido de
/// texto 95% das vezes; `FF04`, `FF23`, `FF06`, `FF22`, `FF08`, 0% — um numero
/// pequeno, e texto de novo na posicao seguinte; `FF26`, dois numeros; `FF0B`,
/// numero ate onde se olha (mais de `MAX_PARAMS`: ele troca de alfabeto, e a
/// leitura para nele). Sem isto o numero era lido como letra e entrava na
/// frase: traduzir a frase trocava o parametro do comando.
fn find_params(codes: &[u16], dialect: &Dialect, spans: &[Span]) -> HashMap<u16, usize> {
    // Por comando: os usos e, em cada posicao seguinte, (quantas vezes nao
    // era outro comando, quantas dessas continuavam um texto).
    type After = [(u32, u32); MAX_PARAMS + 1];
    let mut seen: HashMap<u16, (u32, After)> = HashMap::new();
    for span in spans {
        // O comando logo antes da mensagem conta: o parametro dele foi lido
        // como a primeira letra dela.
        for i in span.begin.saturating_sub(1)..span.end.min(codes.len() - 1) {
            if !(CONTROL..END).contains(&codes[i]) {
                continue;
            }
            let (uses, after) = seen.entry(codes[i]).or_default();
            *uses += 1;
            for (slot, &next) in after.iter_mut().zip(&codes[i + 1..]) {
                if !(CONTROL..END).contains(&next) {
                    slot.0 += 1;
                    slot.1 += u32::from(dialect.goes_on(next));
                }
            }
        }
    }
    seen.into_iter()
        .filter(|&(_, (uses, _))| uses >= MIN_USES)
        .map(|(code, (_, after))| {
            let measured = |k: usize| after.get(k).copied().filter(|&(n, _)| n >= MIN_USES);
            let number = |k: usize| {
                measured(k).is_some_and(|(n, text)| {
                    text * 10 < n
                        // Parametro que as vezes cai no codigo de uma marca de
                        // fala: pouco texto na posicao e texto quase sempre
                        // logo depois dela. Medido no jogo real: o nome de item
                        // (`FF21`) tem o id do item 15% das vezes no "◇" ou no
                        // "◆" e texto em 94% das vezes depois; lido sem o
                        // parametro, o id virava letra da frase.
                        || (text * 4 < n
                            && measured(k + 1).is_some_and(|(n, text)| text * 4 >= n * 3))
                })
            };
            (code, (0..after.len()).take_while(|&k| number(k)).count())
        })
        .collect()
}

/// Os comandos de `params` (2 parametros ou mais) cujo ultimo parametro e
/// uma letra — kana, kanji, ou letra ou numero de largura inteira — em pelo
/// menos 95% dos usos dentro de `spans`. Medido em 16/10/2026: `FF05` muda a
/// cor e desenha a letra do parametro ("Ａボタン" e `[FF05:0003:00DC]`
/// `[FF05:0000:01B3]タン`) e `FF26` estica um nome e fecha com a letra dele
/// ("リュカーーーッ"). Com a letra dentro do comando, a traducao copiava o "ボ"
/// pro jogo em portugues.
fn find_lettered(codes: &[u16], spans: &[Span], params: &HashMap<u16, usize>) -> HashSet<u16> {
    let letter = |code: u16| {
        decode(code).is_some_and(|c| {
            is_kana(c) || is_kanji(c) || (c.is_alphanumeric() && !c.is_ascii() && c != '〇')
        })
    };
    let mut seen: HashMap<u16, (u32, u32)> = HashMap::new();
    for span in spans {
        for i in span.begin.saturating_sub(1)..span.end.min(codes.len()) {
            let Some(&n) = params
                .get(&codes[i])
                .filter(|&&n| (2..=MAX_PARAMS).contains(&n))
            else {
                continue;
            };
            let Some(&last) = codes.get(i + n) else {
                continue;
            };
            let (uses, letters) = seen.entry(codes[i]).or_default();
            *uses += 1;
            *letters += u32::from(letter(last));
        }
    }
    seen.into_iter()
        .filter(|&(_, (uses, letters))| uses >= 3 && letters * 100 >= uses * 95)
        .map(|(code, _)| code)
        .collect()
}

/// Particulas que vem logo depois de um nome: e nelas que se ve que um
/// comando escreve um nome ("[FF11]は", "[FF42:0000]の").
const PARTICLES: &str = "はのがをにともへでや";
/// Usos pra julgar se um comando escreve nome (`find_names`).
const MIN_NAME_USES: u32 = 5;
/// Ate onde se olha pra tras de uma mensagem atras dos comandos que vem antes
/// dela (em unidades).
const MAX_PREFIX: usize = 16;

/// Onde comeca o trecho de comandos logo antes de `begin`: depois do fim da
/// mensagem anterior, a no maximo `MAX_PREFIX` unidades.
fn prefix_start(codes: &[u16], begin: usize) -> usize {
    let floor = begin.saturating_sub(MAX_PREFIX);
    (floor..begin)
        .rev()
        .find(|&k| codes[k] == END)
        .map_or(floor, |k| k + 1)
}

/// Os comandos que escrevem um nome: seguidos de particula (`PARTICLES`) em
/// um terco dos usos ou mais, dentro das mensagens de `spans` e logo antes
/// delas (o nome de item, `FF10` no jogo medido, tambem escreve numero: 58%
/// no texto de batalha; o marcador de opcao de menu, `FF37`, fica em 30% por
/// causa do "はい"). No texto de batalha do jogo medido o nome de quem age (`FF11`) abre
/// 729 mensagens, fora da frase; sem saber que ele ocupa a linha, a quebra do
/// app deixava a linha passar da caixa ("dano fa/tal!").
fn find_names(codes: &[u16], dialect: &Dialect, spans: &[Span]) -> HashSet<u16> {
    let mut seen: HashMap<u16, (u32, u32)> = HashMap::new();
    for span in spans {
        let mut i = prefix_start(codes, span.begin);
        while i < span.end {
            let (piece, len) = dialect.piece(codes, i);
            if matches!(piece, Piece::Control) {
                let next = codes.get(i + len).and_then(|&unit| dialect.decode(unit));
                let (uses, particle) = seen.entry(codes[i]).or_default();
                *uses += 1;
                *particle += u32::from(next.is_some_and(|c| PARTICLES.contains(c)));
            }
            i += len;
        }
    }
    seen.into_iter()
        .filter(|&(_, (uses, particle))| uses >= MIN_NAME_USES && particle * 3 >= uses)
        .map(|(code, _)| code)
        .collect()
}

/// As marcas de fala do jogo: as figuras (`SHAPES`: ☆★○●◎◇◆□■△▲▽▼※ e as
/// setas) com que pelo menos `MIN_USES` mensagens de `spans` abrem logo
/// depois do fim de outra. No jogo medido sao duas, "◆" (6.673 falas) e "◇".
fn find_bullets(codes: &[u16], spans: &[Span]) -> HashSet<u16> {
    let mut first: HashMap<u16, u32> = HashMap::new();
    for span in spans {
        if span.begin > 0 && codes[span.begin - 1] == END && SHAPES.contains(&codes[span.begin]) {
            *first.entry(codes[span.begin]).or_default() += 1;
        }
    }
    first
        .into_iter()
        .filter(|&(_, count)| count >= MIN_USES)
        .map(|(code, _)| code)
        .collect()
}

/// O que pode abrir uma mensagem: kana, kanji, letra, numero, marca de fala e
/// o que abre aspas ou parenteses. Acento solto, virgula, ponto, travessao e
/// afins nao abrem: logo depois do fim de uma mensagem, o numero de um campo
/// vizinho (no jogo medido, o `0x000C` antes de uma tabela de nomes) e lido
/// como um desses — e a traducao escrevia por cima dele.
fn opens_text(c: char) -> bool {
    c.is_alphanumeric() || "☆★○●◎◇◆□■△▲▽▼※→←↑↓♪・…‥‘“（〔［｛〈《「『【＜―～".contains(c)
}

/// Texto longo o bastante pra valer fora de uma fila de textos.
fn stands_alone(text: &str) -> bool {
    let hiragana = text
        .chars()
        .filter(|c| crate::sjis::is_hiragana(*c))
        .count();
    text.chars().count() >= ALONE_CHARS && hiragana * 2 >= ALONE_CHARS
}

/// Uma peca lida a partir de uma unidade.
enum Piece {
    /// Caractere da tabela (o espaco do jogo vem como ' ').
    Char(char),
    /// Comando que troca de alfabeto: nao aparece no texto, e a traducao,
    /// escrita na fonte normal, nao o leva.
    Switch,
    /// Desenho proprio do jogo num codigo que a tabela deixa vazio.
    Glyph,
    /// Comando do jogo, com ou sem parametro.
    Control,
    Other,
}

impl Dialect {
    /// O que continua um texto depois de um comando: kana, kanji, o espaco
    /// do jogo, a marca de fala ou o fim da mensagem. Numero (parametro)
    /// quase nunca e isso.
    fn goes_on(&self, next: u16) -> bool {
        next == END || Some(next) == self.space || self.bullets.contains(&next) || is_script(next)
    }

    /// A peca que comeca em `codes[i]`, quantas unidades ela ocupa e como
    /// ela aparece no texto.
    fn piece(&self, codes: &[u16], i: usize) -> (Piece, usize) {
        let code = codes[i];
        if self.shifted.binary_search_by_key(&i, |&(at, _)| at).is_ok() {
            return (Piece::Switch, 1);
        }
        if let Some(alphabet) = self.alphabet_at(codes, i).filter(|_| code < CONTROL) {
            let Some(&c) = alphabet.get(&code) else {
                return (Piece::Other, 1);
            };
            // Sinal de sonorizacao vem como desenho separado: junta na letra.
            let next = codes.get(i + 1).and_then(|n| alphabet.get(n));
            return match next.and_then(|&mark| voiced(c, mark)) {
                Some(v) => (Piece::Char(v), 2),
                None => (Piece::Char(c), 1),
            };
        }
        if Some(code) == self.space {
            (Piece::Char(' '), 1)
        } else if (CONTROL..END).contains(&code) {
            let rest = &codes[i + 1..];
            let params = match self.params.get(&code) {
                // Numero ate onde se mediu: o que vem depois nao e desta tabela.
                Some(&n) if n > MAX_PARAMS => return (Piece::Other, 1),
                // A letra do fim fica fora do comando: e texto.
                Some(&n) if self.draws_letter(codes, i) => (n - 1).min(rest.len()),
                // Kana seguido de mais texto no lugar do parametro e texto:
                // o comando que leva numero no resto do jogo (o nome de item,
                // `FF21`) aparece sem ele no texto de batalha do jogo medido
                // ("[FF21]のこえ"), e o の virava parametro — e ficava no
                // jogo traduzido. Kana sozinho e numero mesmo: a pausa longa
                // do fim de mensagem cai na faixa do kana ("[FF04]わ").
                Some(&n) => {
                    let text = |at: usize| {
                        rest.get(at)
                            .and_then(|&unit| self.decode(unit))
                            .is_some_and(|c| is_kana(c) || is_kanji(c))
                    };
                    (0..n.min(rest.len()))
                        .take_while(|&at| {
                            !(self.decode(rest[at]).is_some_and(is_kana) && text(at + 1))
                        })
                        .count()
                }
                None if self.params.is_empty() => 0,
                // Comando raro demais pra medir: fica com ele o que vem depois
                // e nao continua um texto.
                None => rest
                    .iter()
                    .take(MAX_PARAMS)
                    .take_while(|&&next| next < CONTROL && !self.goes_on(next))
                    .count(),
            };
            (Piece::Control, 1 + params)
        } else if let Some(c) = self.decode(code) {
            (Piece::Char(c), 1)
        } else if code == 0 && self.in_message(i) {
            // O espaco de largura inteira do JIS: fora de mensagem e
            // preenchimento, dentro e o espaco entre palavras (o do texto de
            // batalha do jogo medido, que o resto do roteiro escreve com
            // outro codigo).
            (Piece::Char(' '), 1)
        } else if code != 0 && code < self.glyphs() {
            (Piece::Glyph, 1)
        } else {
            (Piece::Other, 1)
        }
    }

    fn text_of(&self, codes: &[u16], i: usize) -> String {
        match self.piece(codes, i) {
            (Piece::Char(_), 1) if self.drawn_pairs.contains_key(&codes[i]) => {
                self.drawn_pairs[&codes[i]].clone()
            }
            (Piece::Char(c), _) => c.to_string(),
            (Piece::Switch, _) => String::new(),
            (Piece::Control, len) => token_of(&codes[i..i + len]),
            _ => token_of(&codes[i..=i]),
        }
    }
}

/// Um salto maior que isto entre dois codigos que o texto mostra separa o
/// que fica fora da curva (`reach`).
const OUTLIER_GAP: u16 = 1024;

/// Ate onde o texto do jogo mostra codigos sem nenhum fora da curva: o maior
/// codigo de `shown` antes do primeiro salto de `OUTLIER_GAP` ou mais. Medido
/// em 20/10/2026 no RPG japones de 32 MiB: o texto mostra os codigos ate o
/// 3.798, quase sem buraco, e um kanji raro no 6.912 (6 usos); a fala so
/// alcanca os 4.096 primeiros desenhos, e os pares postos acima do 6.900
/// sairam como o kanji 4.096 posicoes antes. 0 = nenhum codigo.
// ponytail: as letras acentuadas (`images::letters::choose`) ainda usam o
// maior codigo sem esse corte; no jogo medido nao havia desenho vazio acima
// do 3.853, e elas ficaram onde a fala alcanca.
fn reach(shown: &BTreeSet<u16>) -> u16 {
    let mut last = 0;
    for &code in shown {
        if last > 0 && code - last >= OUTLIER_GAP {
            break;
        }
        last = code;
    }
    last
}

/// Uma sequencia de pecas que pode ser uma mensagem: de `begin` a `end` (em
/// unidades), do primeiro ao ultimo caractere ou desenho.
struct Span {
    begin: usize,
    end: usize,
    /// Caracteres e desenhos, espacos do meio inclusive.
    chars: usize,
    script: Script,
}

/// A sequencia que comeca em `codes[at]` (um caractere que nao e espaco, ou
/// um desenho) e a unidade onde a varredura continua. Comando e espaco so
/// entram quando vem outro caractere depois: no fim sao fecho de mensagem e
/// preenchimento de campo.
fn span_at(codes: &[u16], dialect: &Dialect, starts: &HashSet<usize>, at: usize) -> (Span, usize) {
    let mut span = Span {
        begin: at,
        end: at,
        chars: 0,
        script: Script::default(),
    };
    let (mut spaces, mut glyphs) = (0, 0);
    let mut i = at;
    while i < codes.len() && i - at < MAX_SPAN {
        // Onde se sabe que outra mensagem comeca, esta acaba.
        if i > at && starts.contains(&i) {
            break;
        }
        let (piece, len) = dialect.piece(codes, i);
        match piece {
            Piece::Char(' ') => spaces += 1,
            Piece::Char(_) | Piece::Glyph => {
                for _ in 0..std::mem::take(&mut spaces) {
                    span.script.push(' ');
                    span.chars += 1;
                }
                // O desenho proprio conta como um kanji qualquer, um por
                // codigo: e o que ele quase sempre e.
                let c = match piece {
                    Piece::Char(c) => c,
                    _ => char::from_u32(0x4E00 + codes[i] as u32).unwrap_or('\u{4E00}'),
                };
                glyphs += usize::from(matches!(piece, Piece::Glyph));
                // Sem a conta de kanji raro do Shift-JIS: jogo com fonte
                // propria guarda os poucos kanji que usa na ordem dele, e
                // pela tabela padrao metade deles cai no nivel 2 (no jogo
                // medido, os resumos de capitulo, cheios de kanji, ficavam de
                // fora por isso).
                span.script.push(c);
                span.chars += 1;
                span.end = i + len;
            }
            Piece::Control | Piece::Switch => {}
            Piece::Other => break,
        }
        i += len;
    }
    // Sequencia que e quase so desenho proprio e dado, nao texto.
    if glyphs * 4 > span.chars {
        span.chars = 0;
    }
    (span, i.max(at + 1))
}

/// As sequencias de `codes` que passam por mensagem no jeito `dialect` de
/// ler: comeco limpo, a regra de `Script` e, as curtas, fim limpo tambem.
/// `starts` = unidades onde se sabe que uma mensagem comeca (a tabela de um
/// banco aponta pra la): nenhuma sequencia atravessa uma delas.
fn scan(codes: &[u16], dialect: &Dialect, starts: &HashSet<usize>) -> Vec<Span> {
    // Unidades que sao parametro de comando: nunca abrem uma mensagem.
    let mut is_param = vec![false; codes.len()];
    let mut i = 0;
    while i < codes.len() {
        let (piece, len) = dialect.piece(codes, i);
        if matches!(piece, Piece::Control) {
            is_param[i + 1..i + len].fill(true);
        }
        i += len;
    }
    // Antes de uma mensagem vem o fim de outra, um comando (ou o parametro
    // dele) ou preenchimento. Sequencia que comeca depois de outro valor
    // qualquer comeca no meio de um dado: a primeira "letra" dela pode ser o
    // ultimo numero de uma tabela.
    let clean_edge = |at: Option<usize>| {
        at.and_then(|at| codes.get(at).map(|&code| (at, code)))
            .is_none_or(|(at, code)| code == 0 || code >= CONTROL || is_param[at])
    };

    let mut found = Vec::new();
    let mut i = 0;
    while i < codes.len() {
        let opens = !is_param[i]
            && match dialect.piece(codes, i).0 {
                Piece::Glyph => true,
                Piece::Char(c) => opens_text(c),
                _ => false,
            };
        if !opens {
            i += 1;
            continue;
        }
        // Numero de cabecalho da mensagem: logo depois do fim da anterior,
        // um valor baixo (pontuacao da linha 1, se fosse texto) seguido da
        // marca de fala do jogo. Fica fora da entry. No jogo medido sao 414
        // falas assim, com valores de 1 a 52.
        let header = i > 0
            && (codes[i - 1] == END || starts.contains(&i))
            && codes[i] < *SHAPES.start()
            && codes
                .get(i + 1)
                .is_some_and(|next| dialect.bullets.contains(next));
        if header {
            i += 1;
        }
        let (span, next) = span_at(codes, dialect, starts, i);
        // O espaco do jogo antes e depois nao conta: a linha de baixo de uma
        // fala costuma abrir com um, logo depois da quebra de linha.
        let is_space = |at: &usize| Some(codes[*at]) == dialect.space;
        let before = (0..span.begin).rev().find(|at| !is_space(at));
        // Um valor que nao abre texto logo antes (o numero de um campo
        // vizinho, `opens_text`) nao suja o comeco: vale o que vem antes dele.
        let number = |at: usize| {
            codes[at] < CONTROL
                && dialect
                    .decode(codes[at])
                    .is_some_and(|c| !opens_text(c) && c != '　')
        };
        let before = match before {
            Some(at) if number(at) => at.checked_sub(1),
            other => other,
        };
        let after = (span.end..codes.len()).find(|at| !is_space(at));
        let clean = header || starts.contains(&span.begin) || clean_edge(before);
        // Ate 3 caracteres so entre comandos ou zeros dos dois lados: valor
        // pequeno de mapa de tiles vira "あい" o tempo todo.
        let anchored = span.chars > 3 || after.is_none_or(|at| clean_edge(Some(at)));
        if clean && anchored && span.chars > 0 && span.script.verdict() == Some(true) {
            found.push(span);
        }
        i = next;
    }
    found
}

/// Tabela de campos de largura fixa (nomes, itens): `MIN_RECORDS` ou mais
/// sequencias seguidas, uma a cada `stride` unidades, cada uma completada com
/// 0xFFFF ate o comeco da proxima. Pra cada sequencia que esta numa tabela
/// assim, o espaco que ela pode ocupar (em unidades): o do maior texto que o
/// proprio jogo poe nessa tabela — o que ele sabe mostrar —, sempre com pelo
/// menos um 0xFFFF fechando o campo.
///
/// Medido no jogo real: a tabela de nomes tem um campo a cada 12 unidades, e
/// "リュカ" (3) ocupava so 3: a traducao saia "Luc". O maior nome dela tem 6.
fn record_slots(codes: &[u16], found: &[Span]) -> Vec<Option<usize>> {
    let mut slots = vec![None; found.len()];
    for (i, j, stride) in record_runs(codes, found) {
        let longest = found[i..=j]
            .iter()
            .map(|span| span.end - span.begin)
            .max()
            .unwrap_or(0)
            .min(stride - 1);
        for (k, span) in found.iter().enumerate().take(j + 1).skip(i) {
            // O ultimo campo da fila tambem tem que fechar dentro da largura.
            if k < j || filled(codes, span, stride) {
                slots[k] = Some(longest.max(span.end - span.begin));
            }
        }
    }
    slots
}

/// O campo de `span` numa tabela de largura `stride` vai do fim do texto ao
/// comeco do proximo so com 0xFFFF.
fn filled(codes: &[u16], span: &Span, stride: usize) -> bool {
    codes
        .get(span.end..span.begin + stride)
        .is_some_and(|rest| !rest.is_empty() && rest.iter().all(|c| *c == END))
}

/// As filas de campos de largura fixa de `found` (`record_slots`): primeira e
/// ultima sequencia da fila e a largura, em unidades.
fn record_runs(codes: &[u16], found: &[Span]) -> Vec<(usize, usize, usize)> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i + 1 < found.len() {
        let stride = found[i + 1].begin - found[i].begin;
        let mut j = i;
        while j + 1 < found.len()
            && found[j + 1].begin - found[j].begin == stride
            && filled(codes, &found[j], stride)
        {
            j += 1;
        }
        if j + 1 - i < MIN_RECORDS || stride > MAX_RECORD {
            i += 1;
            continue;
        }
        runs.push((i, j, stride));
        // O ultimo da fila pode ser o primeiro da proxima: a distancia ate ele
        // por acaso bateu com a largura desta.
        i = j;
    }
    runs
}

/// Campos de uma fila de `record_runs` que a varredura deixou de fora, logo
/// antes da primeira e logo depois da ultima sequencia: dentro da fila, o
/// comeco limpo nao precisa valer — o campo e texto se o texto ocupa o comeco
/// dele e o resto e 0xFFFF ate o proximo. Medido no jogo real: o primeiro nome
/// de uma tabela vinha depois de dois numeros de outra estrutura, ficava fora
/// da extracao e seguia em japones na tela.
fn missed_records(
    codes: &[u16],
    dialect: &Dialect,
    starts: &HashSet<usize>,
    found: &[Span],
) -> Vec<Span> {
    let taken = |at: usize, stride: usize| {
        found
            .iter()
            .any(|span| span.begin < at + stride && at < span.end.max(span.begin + 1))
    };
    let field = |at: usize, stride: usize| -> Option<Span> {
        if at >= codes.len() {
            return None;
        }
        let opens = match dialect.piece(codes, at).0 {
            Piece::Glyph => true,
            Piece::Char(c) => opens_text(c),
            _ => false,
        };
        if !opens || taken(at, stride) {
            return None;
        }
        let (span, _) = span_at(codes, dialect, starts, at);
        (span.chars > 0
            && span.end < at + stride
            && filled(codes, &span, stride)
            && span.script.verdict() == Some(true))
        .then_some(span)
    };
    let mut extra = Vec::new();
    for (i, j, stride) in record_runs(codes, found) {
        let mut at = found[i].begin;
        while let Some(span) = at.checked_sub(stride).and_then(|prev| field(prev, stride)) {
            at = span.begin;
            extra.push(span);
        }
        let mut at = found[j].begin + stride;
        while let Some(span) = field(at, stride) {
            at += stride;
            extra.push(span);
        }
    }
    extra
}

/// As mensagens de `data` nesta codificacao, ou `None` se o arquivo nao
/// guarda texto assim (menos de `MIN_SENTENCES` frases). Cada entry vai do
/// primeiro ao ultimo caractere de uma sequencia de caracteres, desenhos e
/// comandos; os comandos do meio viram `[FFxx]` no texto (`[FFxx:pppp]` se
/// levam parametro), e os das pontas ficam de fora (nao sao mexidos).
///
/// Sao duas leituras: a primeira acha onde esta o texto, e e nele que se ve
/// quais comandos levam parametro e com que marca as falas abrem; a segunda
/// le de novo sabendo disso.
///
/// `starts` = posicoes (em bytes) onde se sabe que uma mensagem comeca, de
/// quem conhece a estrutura em volta do texto (`adapters::bank`). Pode vir
/// vazio. De `hints` valem os alfabetos depois de um comando, os pares de
/// letras que o projeto quer na fonte do jogo e os que a copia lida ja tem
/// desenhados (`ExtractHints`).
pub fn extract(
    data: &[u8],
    starts: &HashSet<usize>,
    hints: &crate::adapter::ExtractHints,
) -> Option<Kuten16> {
    let starts: HashSet<usize> = starts.iter().map(|at| at / 2).collect();
    let starts = &starts;
    let codes = units(data);
    let switches: HashMap<u16, HashMap<u16, char>> = hints.alphabets.iter().cloned().collect();
    let fonts = fonts(data);
    let mut dialect = Dialect {
        space: find_space(&codes),
        font: font_order(data, &fonts),
        shifted: honored(&codes, starts, &switches),
        switches,
        known: messages(&codes, starts),
        drawn_pairs: hints.drawn_pairs.clone(),
        ..Dialect::default()
    };
    let first = scan(&codes, &dialect, starts);
    dialect.bullets = find_bullets(&codes, &first);
    dialect.params = find_params(&codes, &dialect, &first);
    dialect.lettered = find_lettered(&codes, &first, &dialect.params);
    let mut found = scan(&codes, &dialect, starts);
    found.extend(missed_records(&codes, &dialect, starts, &found));
    found.sort_by_key(|span| span.begin);
    let names = find_names(&codes, &dialect, &found);

    // Texto de jogo vem em fila; fora de fila so fica a sequencia longa.
    let text_of = |span: &Span| -> String {
        let mut text = String::new();
        let mut i = span.begin;
        while i < span.end {
            text.push_str(&dialect.text_of(&codes, i));
            i += dialect.piece(&codes, i).1;
        }
        text
    };
    let slots = record_slots(&codes, &found);
    let mut entries = Vec::new();
    let mut spans = Vec::new();
    let mut commands = BTreeSet::new();
    let mut group = 0;
    // O maior codigo de caractere que o texto guardado mostra: e ate onde o
    // jogo desenha com certeza. E todos eles, pra achar o que fica fora da
    // curva (`reach`).
    let mut shown = 0;
    let mut shown_codes = BTreeSet::new();
    for i in 0..found.len() {
        let last = found
            .get(i + 1)
            .is_none_or(|next| (next.begin - found[i].end) * 2 > NEIGHBOR_GAP);
        if !last {
            continue;
        }
        let grouped = i + 1 - group >= MIN_GROUP;
        for (span, slot) in found[group..=i].iter().zip(&slots[group..=i]) {
            let text = text_of(span);
            if !grouped && !stands_alone(&text) {
                continue;
            }
            let mut at = span.begin;
            let (mut lettered, mut letter_only) = (BTreeSet::new(), BTreeSet::new());
            while at < span.end {
                let (piece, len) = dialect.piece(&codes, at);
                let drawn = match piece {
                    Piece::Char(c) => c != ' ' && dialect.alphabet_at(&codes, at).is_none(),
                    Piece::Glyph => true,
                    _ => false,
                };
                if drawn {
                    shown = shown.max(codes[at]);
                    shown_codes.insert(codes[at]);
                }
                if matches!(piece, Piece::Control) && len > 1 {
                    commands.insert(codes[at..at + len].to_vec());
                }
                if matches!(piece, Piece::Control) && dialect.draws_letter(&codes, at) {
                    lettered.insert(token_of(&codes[at..at + len]));
                    // O que so desenha a letra (o de `lettered`) nao ocupa nada
                    // na linha; o nome esticado ocupa o nome.
                    if dialect.lettered.contains(&codes[at]) {
                        letter_only.insert(format!("[{:04X}", codes[at]));
                    }
                }
                at += len;
            }
            // Campo de tabela de largura fixa: a entry vai ate onde o campo
            // deixa, e a sobra da traducao volta a ser 0xFFFF.
            let units = slot.unwrap_or(span.end - span.begin);
            let (offset, len) = (span.begin * 2, units * 2);
            spans.push((offset, len));
            let mut metadata = match slot {
                Some(_) => serde_json::json!({
                    "scanner": TABLE_ID,
                    "chars": span.chars,
                    "terminated": false,
                    "fill": "FFFF",
                }),
                None => serde_json::json!({
                    "scanner": TABLE_ID,
                    "chars": span.chars,
                    "terminated": codes.get(span.end) == Some(&0),
                }),
            };
            // O comando que desenha a letra seguinte: a traducao tem que por uma
            // letra logo depois dele (`validate`).
            if !lettered.is_empty() {
                metadata["lettered"] = serde_json::json!(lettered);
            }
            if !letter_only.is_empty() {
                metadata["letter_only"] = serde_json::json!(letter_only);
            }
            // Nome que o jogo escreve antes da frase: ocupa a primeira linha.
            let mut before = 0;
            let mut at = prefix_start(&codes, span.begin);
            while at < span.begin {
                let (piece, len) = dialect.piece(&codes, at);
                before +=
                    usize::from(matches!(piece, Piece::Control) && names.contains(&codes[at]));
                at += len;
            }
            if before > 0 {
                metadata["names_before"] = serde_json::json!(before);
            }
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
                metadata,
            });
        }
        group = i + 1;
    }
    let sentences = entries
        .iter()
        .filter(|e| is_sentence(&e.source_text))
        .count();
    if let Some(order) = &dialect.font {
        // Codigo que algum texto usa (ou o espaco) nao vira letra nova.
        let used: HashSet<u16> = found
            .iter()
            .flat_map(|span| codes[span.begin..span.end].iter().copied())
            .chain(dialect.space)
            .collect();
        dialect.letters = crate::images::letters::choose(data, &fonts, order, &used, shown)
            .into_iter()
            .collect();
        // Nem o codigo que ganhou letra acentuada.
        let used: HashSet<u16> = used
            .into_iter()
            .chain(dialect.letters.keys().copied())
            .collect();
        dialect.pairs = crate::images::letters::choose_pairs(
            data,
            &fonts,
            order,
            &used,
            reach(&shown_codes),
            &hints.pairs,
        )
        .into_iter()
        .collect();
    }

    (sentences >= MIN_SENTENCES).then(|| Kuten16 {
        entries,
        spans,
        table: table_with(&dialect, &commands),
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
        let commands = BTreeSet::from([vec![0xFF08, 0x0003], vec![0xFF26, 0x0003, 0x0001]]);
        let text = table_text(Some(0x00AC), &commands);
        assert!(text.contains("\n1B01=あ\n"), "hiragana");
        assert!(text.contains("\nAC00= \n"), "o espaco do jogo");
        assert!(text.contains("\n01FF=[FF01]\n"), "codigo de controle");
        assert!(
            text.contains("\n08FF0300=[FF08:0003]\n")
                && text.contains("\n26FF03000100=[FF26:0003:0001]\n"),
            "comando com parametro"
        );
        // Codigo que a tabela padrao deixa vazio: desenho proprio do jogo.
        assert!(text.contains("\nAD00=[00AD]\n"), "desenho proprio");
        assert!(!text.contains("\n0000="), "zero e preenchimento");
        assert!(is_glyph_token("[00AD]") && !is_glyph_token("[FF01]"));
        assert!(!is_glyph_token("[FF08:0003]") && !is_glyph_token("[ITEM]"));
        assert!(!text.contains("FFFF="), "0xFFFF fecha a mensagem");
        let table = crate::tbl::TblTable::parse(&text).unwrap();
        assert_eq!(
            table.encode("あ [FF01][FF08:0003][00AD]").unwrap(),
            [0x1B, 0x01, 0xAC, 0x00, 0x01, 0xFF, 0x08, 0xFF, 0x03, 0x00, 0xAD, 0x00]
        );
        // Sem espaco proprio, o espaco e o codigo 0.
        assert!(table_text(None, &BTreeSet::new()).contains("\n0000= \n"));
    }

    /// 120 mensagens "◆<kana>" + comando + `numbers` + "<kana>", uma atras da
    /// outra, e como a mensagem do meio sai lida.
    fn read_with(command: u16, numbers: &[u16]) -> Option<String> {
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        let mut codes = Vec::new();
        for i in 0..120u16 {
            codes.push(encode('◆').unwrap());
            codes.extend(kana("むらの ひとは".replace(' ', "").as_str()));
            codes.push(command);
            codes.extend(numbers.iter().map(|n| n + i % 3));
            codes.extend(kana(
                "きょうも げんきに はたらいている".replace(' ', "").as_str(),
            ));
            codes.extend([0xFF03, 0xFF00, END]);
        }
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let found = extract(&data, &HashSet::new(), &Default::default())?;
        Some(found.entries[60].source_text.clone())
    }

    /// 120 mensagens de banco "◆<kana>" + `FF0B` + `saturn` (no alfabeto do
    /// comando) + fim; com `alphabet` ensinado ou nao, e `starts` (onde as
    /// mensagens comecam) ou nao, como a mensagem do meio sai lida.
    fn read_switched(saturn: &[u16], taught: bool, banked: bool) -> String {
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        let mut codes = Vec::new();
        let mut starts = HashSet::new();
        for _ in 0..120 {
            starts.insert(codes.len() * 2);
            codes.push(encode('◆').unwrap());
            codes.extend(kana("むらのひとは"));
            codes.push(0xFF0B);
            codes.extend(saturn);
            codes.extend([0xFF03, 0xFF00, END]);
        }
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let alphabet: HashMap<u16, char> =
            [(19, 'て'), (59, '゛'), (13, 'す'), (62, '◆'), (63, '。')].into();
        let alphabets = if taught {
            vec![(0xFF0B, alphabet)]
        } else {
            Vec::new()
        };
        let starts = if banked { starts } else { HashSet::new() };
        let hints = crate::adapter::ExtractHints {
            alphabets,
            ..Default::default()
        };
        let found = extract(&data, &starts, &hints).unwrap();
        found.entries[60].source_text.clone()
    }

    #[test]
    fn an_alphabet_taught_after_a_command_is_read_inside_bank_messages() {
        let saturn = [62, 19, 59, 13, 63];
        // Ensinado e dentro de mensagem de banco: o comando some e a letra
        // junta com o sinal de sonorizacao.
        assert_eq!(read_switched(&saturn, true, true), "◆むらのひとは◆です。");
        // Sem o alfabeto, ou fora de mensagem conhecida: a leitura para no
        // comando, como antes.
        assert_eq!(read_switched(&saturn, false, true), "◆むらのひとは");
        assert_eq!(read_switched(&saturn, true, false), "◆むらのひとは");
        assert_eq!(voiced('は', '゜'), Some('ぱ'));
        assert_eq!(voiced('あ', '゛'), None);
    }

    #[test]
    fn a_number_before_a_name_is_not_the_first_letter_of_it() {
        // Fila de nomes como a do jogo medido: um numero de outra tabela
        // (0x000C, que pela tabela seria o acento "´") logo depois de um fim
        // de mensagem, e o nome em seguida.
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        let mut codes = Vec::new();
        for i in 0..120u16 {
            codes.push(encode('◆').unwrap());
            codes.extend(kana("むらのひとは"));
            codes.push(0xFF01);
            codes.extend(kana("きょうもげんきにはたらいている"));
            codes.push(END);
            if i == 60 {
                codes.extend([0x0002, END, 0x000C]);
                codes.extend(kana("こあき"));
                codes.push(END);
            }
        }
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let found = extract(&data, &HashSet::new(), &Default::default()).unwrap();
        let name = found
            .entries
            .iter()
            .find(|e| e.source_text.contains("こあき"))
            .expect("o nome foi lido");
        assert_eq!(name.source_text, "こあき");
    }

    #[test]
    fn a_table_of_fixed_fields_lends_each_name_the_room_of_the_longest() {
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        // Falas de tamanhos variados, como as de um jogo.
        let predicates = [
            "きょうもげんきにはたらいている",
            "まだねむそうにしている",
            "ゆっくりさんぽをしている",
        ];
        let mut codes = Vec::new();
        for i in 0..120 {
            codes.push(encode('◆').unwrap());
            codes.extend(kana("むらのひとは"));
            codes.push(0xFF01);
            codes.extend(kana(predicates[i % 3]));
            codes.push(END);
        }
        // Tabela de nomes: um campo a cada 8 unidades, completado com 0xFFFF.
        let names = [
            "リュカ",
            "クラウス",
            "フリント",
            "ヒナワ",
            "ボニー",
            "ダスター",
            "クマトラ",
            "サルサ",
            "ウエスト",
            "アレック",
        ];
        for name in names {
            let mut field = kana(name);
            field.resize(8, END);
            codes.extend(field);
        }
        codes.extend([0, 0, 0, 0]);
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let found = extract(&data, &HashSet::new(), &Default::default()).unwrap();
        let entry = |name: &str| {
            found
                .entries
                .iter()
                .find(|e| e.source_text == name)
                .unwrap()
        };
        // O maior nome tem 4: cada campo pode levar 4, e o resto volta a ser fim.
        assert_eq!(entry("リュカ").original_bytes.len(), 8);
        assert_eq!(&entry("リュカ").original_bytes[6..], &[0xFF, 0xFF]);
        assert_eq!(entry("リュカ").max_bytes, Some(8));
        assert_eq!(entry("リュカ").metadata["fill"], "FFFF");
        assert_eq!(entry("クラウス").original_bytes.len(), 8);
        // Fora de tabela, a frase so tem o proprio espaco.
        let line = found
            .entries
            .iter()
            .find(|e| e.source_text.starts_with('◆'))
            .unwrap();
        assert!(line.metadata.get("fill").is_none());
    }

    #[test]
    fn the_first_field_of_a_table_counts_after_numbers_of_another_record() {
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        let mut codes = Vec::new();
        for _ in 0..120 {
            codes.push(encode('◆').unwrap());
            codes.extend(kana("むらのひとは"));
            codes.push(0xFF01);
            codes.extend(kana("きょうもげんきにはたらいている"));
            codes.push(END);
        }
        // Antes do primeiro campo, dois numeros de outra estrutura: o comeco
        // dele nao e limpo, so a fila mostra que e um nome.
        codes.extend([0x0BDC, END, 0x0009, 0x0046]);
        for name in [
            "リュカ",
            "クラウス",
            "フリント",
            "ヒナワ",
            "ボニー",
            "ダスター",
            "クマトラ",
            "サルサ",
            "ウエスト",
        ] {
            let mut field = kana(name);
            field.resize(8, END);
            codes.extend(field);
        }
        codes.extend([0, 0, 0, 0]);
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let found = extract(&data, &HashSet::new(), &Default::default()).unwrap();
        let first = found
            .entries
            .iter()
            .find(|e| e.source_text == "リュカ")
            .expect("o primeiro nome da tabela foi lido");
        assert_eq!(first.original_bytes.len(), 8);
        assert_eq!(first.metadata["fill"], "FFFF");
        // Nada antes dele virou frase: os numeros continuam como estao.
        assert!(!found.entries.iter().any(|e| e.source_text.contains('∞')));
    }

    #[test]
    fn a_parameter_that_falls_on_a_speech_mark_stays_with_its_command() {
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        // Itens do jogo: um em cada sete tem o id no codigo do "◇", que abre
        // uma fala em cada cinco mensagens.
        let items = [0x005D, 0x0067, 0x006B, 0x00E6, 0x006F, 0x00C5, 0x00C1];
        let mut codes = Vec::new();
        for i in 0..140 {
            if i % 5 == 0 {
                codes.push(encode('◇').unwrap());
                codes.extend(kana("きばこがおいてある"));
            } else {
                codes.push(encode('◆').unwrap());
                codes.extend(kana("むらのひとは"));
                codes.extend([0xFF21, items[i % items.len()]]);
                codes.extend(kana("をてにいれた"));
            }
            codes.push(END);
        }
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let found = extract(&data, &HashSet::new(), &Default::default()).unwrap();
        let texts: Vec<&str> = found
            .entries
            .iter()
            .map(|e| e.source_text.as_str())
            .collect();
        assert!(
            texts.contains(&"◆むらのひとは[FF21:005D]をてにいれた"),
            "{:?}",
            &texts[..8]
        );
        assert!(texts.iter().all(|t| !t.contains("[FF21]")), "{texts:?}");
    }

    #[test]
    fn a_command_keeps_as_many_numbers_as_it_takes() {
        // Sem numero depois: o comando nao leva parametro.
        let text = read_with(0xFF01, &[]).unwrap();
        assert!(text.contains("ひとは[FF01]きょうも"), "{text}");
        // Um e dois numeros: ficam na peca do comando, fora da frase.
        let text = read_with(0xFF23, &[2]).unwrap();
        assert!(text.contains("ひとは[FF23:0002]きょうも"), "{text}");
        let text = read_with(0xFF26, &[3, 1]).unwrap();
        assert!(text.contains("ひとは[FF26:0003:0001]きょうも"), "{text}");
        // Numero ate onde se olha: nao e parametro, e outro alfabeto. A
        // leitura para no comando e nada do que vem depois dele vira frase.
        let text = read_with(0xFF0B, &[0x3E, 5, 9, 7, 2, 8]).unwrap();
        assert!(!text.contains('['), "{text}");
        // Ultimo parametro sempre letra: ela e texto, e o comando leva so a
        // cor ("Ａボタン" = cor 3 + Ａ, cor 0 + ボ, タン).
        let text = read_with(0xFF05, &[3, encode('く').unwrap()]).unwrap();
        assert!(text.contains("ひとは[FF05:0003]くきょうも"), "{text}");
    }

    #[test]
    fn a_kana_where_a_number_usually_goes_is_text() {
        // FF23 leva numero em 119 mensagens; na outra vem direto o の do
        // texto, como "[FF21]のこえ" no texto de batalha do jogo medido.
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        let mut codes = Vec::new();
        for i in 0..120u16 {
            codes.push(encode('◆').unwrap());
            codes.extend(kana("むらのひとは"));
            codes.push(0xFF23);
            if i != 60 {
                codes.push(2 + i % 3);
            }
            codes.extend(kana("のきょうもげんきにはたらいている"));
            codes.extend([0xFF03, 0xFF00, END]);
        }
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let found = extract(&data, &HashSet::new(), &Default::default()).unwrap();
        let text = &found.entries[60].source_text;
        assert!(text.contains("ひとは[FF23]のきょうも"), "{text}");
        let text = &found.entries[59].source_text;
        assert!(text.contains("ひとは[FF23:0004]のきょうも"), "{text}");
    }

    #[test]
    fn a_kana_in_the_last_of_many_numbers_is_the_letter_the_command_draws() {
        // O nome esticado: numero do nome, tamanho e o caractere do fim — ！
        // em 119 mensagens, ッ na outra ("リュカーーーッ！").
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        let mut codes = Vec::new();
        for i in 0..120u16 {
            codes.push(encode('◆').unwrap());
            codes.extend(kana("むらのひとは"));
            let last = if i == 60 { 'ッ' } else { '！' };
            codes.extend([0xFF26, 3, 1 + i % 2, encode(last).unwrap()]);
            codes.extend(kana("きょうもげんきにはたらいている"));
            codes.extend([0xFF03, 0xFF00, END]);
        }
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let found = extract(&data, &HashSet::new(), &Default::default()).unwrap();
        let entry = &found.entries[60];
        assert!(
            entry
                .source_text
                .contains("ひとは[FF26:0003:0001]ッきょうも"),
            "{}",
            entry.source_text
        );
        assert_eq!(entry.metadata["lettered"][0], "[FF26:0003:0001]");
        // Ele desenha o nome antes da letra: ocupa a linha (`letter_only` e
        // so o que desenha a letra e nada mais).
        assert!(entry.metadata.get("letter_only").is_none());
        let text = &found.entries[59].source_text;
        assert!(text.contains("[FF26:0003:0002:0009]きょうも"), "{text}");
    }

    #[test]
    fn a_name_the_game_writes_before_the_sentence_is_counted() {
        // O nome de quem age abre a mensagem, fora da frase ("[FF11]は ..."):
        // ele ocupa a primeira linha, e a entry diz quantos ha.
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        let mut codes = Vec::new();
        for _ in 0..120 {
            codes.push(0xFF11);
            codes.extend(kana("はきょうもげんきにはたらいている"));
            codes.extend([0xFF03, 0xFF00, END]);
        }
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let found = extract(&data, &HashSet::new(), &Default::default()).unwrap();
        let entry = &found.entries[60];
        assert!(entry.source_text.starts_with('は'), "{}", entry.source_text);
        assert_eq!(entry.metadata["names_before"], 1);
    }

    /// A copia de trabalho relida (auditoria): o codigo de kanji que virou
    /// par de letras e lido como as letras, nao como o kanji do JIS. Um
    /// codigo de par que nao esta no texto nao muda nada.
    #[test]
    fn a_drawn_pair_reads_as_its_letters() {
        let kana = |text: &str| text.chars().map(|c| encode(c).unwrap()).collect::<Vec<_>>();
        let (father, tea) = (encode('父').unwrap(), encode('茶').unwrap());
        let mut codes = Vec::new();
        for _ in 0..120 {
            codes.push(encode('◆').unwrap());
            codes.extend(kana("むらの"));
            codes.push(father);
            codes.extend(kana("は"));
            codes.push(0xFF01);
            codes.extend(kana("げんきにはたらいている。"));
            codes.extend([0xFF03, 0xFF00, END]);
        }
        let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let read = |drawn: &[(u16, &str)]| {
            let hints = crate::adapter::ExtractHints {
                drawn_pairs: drawn.iter().map(|&(k, p)| (k, p.to_string())).collect(),
                ..Default::default()
            };
            extract(&data, &HashSet::new(), &hints).unwrap().entries[60]
                .source_text
                .clone()
        };
        assert!(read(&[]).contains('父'));
        let paired = read(&[(father, "de")]);
        assert!(paired.starts_with("◆むらのdeは"), "{paired}");
        assert!(!paired.chars().any(is_kanji));
        assert_eq!(read(&[(tea, "de")]), read(&[]));
    }

    /// O codigo fora da curva (um kanji raro muito acima do resto) nao
    /// conta: no jogo medido, o 6.912 contra o resto ate o 3.798.
    #[test]
    fn the_reach_of_the_text_leaves_out_a_code_far_above_the_rest() {
        let codes = |list: &[u16]| list.iter().copied().collect::<BTreeSet<u16>>();
        assert_eq!(
            reach(&codes(&[94, 300, 1200, 2000, 2900, 3798, 6912])),
            3798
        );
        assert_eq!(reach(&codes(&[94, 300, 900])), 900);
        assert_eq!(reach(&codes(&[94, 2000])), 94);
        assert_eq!(reach(&BTreeSet::new()), 0);
    }

    #[test]
    fn noise_and_small_numbers_are_not_text() {
        assert!(extract(
            &make_random(1 << 20, 5),
            &HashSet::new(),
            &Default::default()
        )
        .is_none());
        // Rampa de valores pequenos, como um mapa de tiles: cai nos kana.
        let ramp: Vec<u8> = (0..40_000u32)
            .flat_map(|i| ((280 + i % 90) as u16).to_le_bytes())
            .collect();
        assert!(extract(&ramp, &HashSet::new(), &Default::default()).is_none());
        assert!(extract(&[], &HashSet::new(), &Default::default()).is_none());
        assert!(extract(&[0x1B], &HashSet::new(), &Default::default()).is_none());
    }
}

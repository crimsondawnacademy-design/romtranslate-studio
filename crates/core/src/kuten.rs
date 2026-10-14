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

use encoding_rs::EUC_JP;

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
}

/// O `.tbl` desta codificacao: uma linha por caractere (chave = os 2 bytes do
/// codigo, o menos significativo primeiro), o espaco, os codigos de controle
/// como `[FFxx]`, cada comando com os parametros dele (`commands`) como
/// `[FFxx:pppp]` e, como `[xxxx]`, os codigos em que a tabela padrao nao tem nada — e onde
/// o jogo poe os desenhos dele (kanji proprio, icone). `space` = o codigo que
/// o jogo usa como espaco entre palavras; sem ele o espaco e o codigo 0 (o de
/// largura inteira do JIS).
pub fn table_text(space: Option<u16>, commands: &BTreeSet<Vec<u16>>) -> String {
    let mut out = String::from(
        "# Tabela gerada pelo RomTranslate Studio: codigos de 16 bits na ordem do JIS X 0208\n\
         # (codigo = posicao do caractere na tabela, gravado em little-endian).\n\
         # [FFxx] e comando do jogo; [FFxx:pppp], comando com os parametros dele.\n\
         # [xxxx] e um desenho proprio do jogo (kanji, icone) num codigo que a tabela\n\
         # padrao deixa vazio: troque pelo caractere certo se souber qual e.\n\
         # Pode ser completada a mao: o app nao sobrescreve este arquivo.\n",
    );
    let space = space.unwrap_or(0);
    let _ = writeln!(out, "{}= ", key(space));
    for code in 1..GLYPHS {
        if code == space {
            continue;
        }
        match decode(code) {
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
            let params = after
                .iter()
                .take_while(|&&(n, text)| n >= MIN_USES && text * 10 < n)
                .count();
            (code, params)
        })
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
        if Some(code) == self.space {
            (Piece::Char(' '), 1)
        } else if (CONTROL..END).contains(&code) {
            let rest = &codes[i + 1..];
            let params = match self.params.get(&code) {
                // Numero ate onde se mediu: o que vem depois nao e desta tabela.
                Some(&n) if n > MAX_PARAMS => return (Piece::Other, 1),
                Some(&n) => n.min(rest.len()),
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
        } else if let Some(c) = decode(code) {
            (Piece::Char(c), 1)
        } else if code != 0 && code < GLYPHS {
            (Piece::Glyph, 1)
        } else {
            (Piece::Other, 1)
        }
    }

    fn text_of(&self, codes: &[u16], i: usize) -> String {
        match self.piece(codes, i) {
            (Piece::Char(c), _) => c.to_string(),
            (Piece::Control, len) => token_of(&codes[i..i + len]),
            _ => token_of(&codes[i..=i]),
        }
    }
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
            Piece::Control => {}
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
            codes[at] < CONTROL && decode(codes[at]).is_some_and(|c| !opens_text(c) && c != '　')
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
/// vazio.
pub fn extract(data: &[u8], starts: &HashSet<usize>) -> Option<Kuten16> {
    let starts: HashSet<usize> = starts.iter().map(|at| at / 2).collect();
    let starts = &starts;
    let codes = units(data);
    let mut dialect = Dialect {
        space: find_space(&codes),
        ..Dialect::default()
    };
    let first = scan(&codes, &dialect, starts);
    dialect.bullets = find_bullets(&codes, &first);
    dialect.params = find_params(&codes, &dialect, &first);
    let found = scan(&codes, &dialect, starts);

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
    let mut entries = Vec::new();
    let mut spans = Vec::new();
    let mut commands = BTreeSet::new();
    let mut group = 0;
    for i in 0..found.len() {
        let last = found
            .get(i + 1)
            .is_none_or(|next| (next.begin - found[i].end) * 2 > NEIGHBOR_GAP);
        if !last {
            continue;
        }
        let grouped = i + 1 - group >= MIN_GROUP;
        for span in &found[group..=i] {
            let text = text_of(span);
            if !grouped && !stands_alone(&text) {
                continue;
            }
            let mut at = span.begin;
            while at < span.end {
                let (piece, len) = dialect.piece(&codes, at);
                if matches!(piece, Piece::Control) && len > 1 {
                    commands.insert(codes[at..at + len].to_vec());
                }
                at += len;
            }
            let (offset, len) = (span.begin * 2, (span.end - span.begin) * 2);
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
                    "chars": span.chars,
                    "terminated": codes.get(span.end) == Some(&0),
                }),
            });
        }
        group = i + 1;
    }
    let sentences = entries
        .iter()
        .filter(|e| is_sentence(&e.source_text))
        .count();

    (sentences >= MIN_SENTENCES).then(|| Kuten16 {
        entries,
        spans,
        table: table_text(dialect.space, &commands),
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
        let found = extract(&data, &HashSet::new())?;
        Some(found.entries[60].source_text.clone())
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
        let found = extract(&data, &HashSet::new()).unwrap();
        let name = found
            .entries
            .iter()
            .find(|e| e.source_text.contains("こあき"))
            .expect("o nome foi lido");
        assert_eq!(name.source_text, "こあき");
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
    }

    #[test]
    fn noise_and_small_numbers_are_not_text() {
        assert!(extract(&make_random(1 << 20, 5), &HashSet::new()).is_none());
        // Rampa de valores pequenos, como um mapa de tiles: cai nos kana.
        let ramp: Vec<u8> = (0..40_000u32)
            .flat_map(|i| ((280 + i % 90) as u16).to_le_bytes())
            .collect();
        assert!(extract(&ramp, &HashSet::new()).is_none());
        assert!(extract(&[], &HashSet::new()).is_none());
        assert!(extract(&[0x1B], &HashSet::new()).is_none());
    }
}

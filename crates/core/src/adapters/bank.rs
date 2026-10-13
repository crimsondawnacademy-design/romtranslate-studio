//! Banco de mensagens em blocos: a estrutura que deixa a traducao CRESCER.
//!
//! E como o primeiro RPG japones medido guarda o roteiro (13/10/2026, so
//! contagens: um banco so, 563 blocos, 8.120 mensagens, 85,6% do texto do
//! cartucho):
//!
//! - no inicio do banco, um u32 com a quantidade N e N offsets u32 relativos
//!   a esse inicio (0 = vazio), aos pares: a tabela de um bloco e o texto dele;
//! - a tabela e uma fila de offsets u16 relativos ao comeco do bloco de texto
//!   (0xFFFF = sem mensagem), um por mensagem;
//! - cada mensagem fecha com 0xFFFF.
//!
//! Nada aqui supoe um jogo: o banco e reconhecido pela propria estrutura, com
//! centenas de offsets que tem que cair no lugar certo. Com ele, mensagem que
//! cresce nao precisa caber no espaco da original: o bloco e remontado com as
//! mensagens em fila e a tabela refeita. Se o bloco novo cabe onde o antigo
//! estava, fica la; senao vai pro espaco livre do arquivo (preenchimento de
//! 0xFF, e o lugar dos blocos que sairam) e o diretorio e reapontado.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::adapter::ApplyReport;
use crate::error::{CoreError, Result};
use crate::types::TextEntry;

const END: u16 = 0xFFFF;
/// Entradas do diretorio de um banco: par, e nem pouco nem um absurdo.
const DIR_ENTRIES: std::ops::RangeInclusive<usize> = 8..=20_000;
/// O primeiro bloco vem logo depois do diretorio, com no maximo isto de
/// alinhamento.
const DIR_SLACK: usize = 64;
/// Um bloco de texto nao passa disto: os offsets da tabela sao u16, e
/// 0xFFFF quer dizer "sem mensagem".
const MAX_TEXT: usize = 0xFFFE;
/// So preenchimento longo assim (em bytes de 0xFF seguidos) e tratado como
/// espaco livre: trecho curto de 0xFF pode ser dado.
const MIN_PADDING: usize = 64 * 1024;
/// Usos de um codigo (ou de uma fila deles) pra valer como medida do jogo.
const MIN_RUNS: usize = 15;
/// Paragrafos mais altos que a caixa pra valer como prova de que ela rola.
const MIN_TALL: usize = 8;

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Um bloco: a tabela de offsets e o texto das mensagens.
#[derive(Debug, Clone)]
pub(crate) struct Room {
    /// Os pares do diretorio que apontam pra este bloco (quase sempre um).
    pairs: Vec<usize>,
    table: usize,
    /// Quantos offsets a tabela tem.
    slots: usize,
    text: usize,
    text_len: usize,
    /// Onde cada mensagem comeca dentro do bloco de texto: os offsets da
    /// tabela, em ordem e sem repetir, com o 0 na frente. Vazio = bloco com
    /// offset fora dele: nao e remontado.
    starts: Vec<usize>,
}

impl Room {
    /// Bytes que o bloco ocupa, tabela e texto.
    fn size(&self) -> usize {
        self.text + self.text_len - self.table
    }

    /// (inicio, fim) do trecho da mensagem `i` dentro do bloco de texto.
    fn segment(&self, i: usize) -> (usize, usize) {
        let end = self.starts.get(i + 1).copied().unwrap_or(self.text_len);
        (self.starts[i], end)
    }

    /// A mensagem que contem os bytes `[at, at + len)` do arquivo, se eles
    /// cabem inteiros numa so.
    fn segment_of(&self, at: usize, len: usize) -> Option<usize> {
        let rel = at.checked_sub(self.text)?;
        let i = self
            .starts
            .partition_point(|&start| start <= rel)
            .checked_sub(1)?;
        (rel + len <= self.segment(i).1).then_some(i)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Bank {
    /// Onde o banco comeca (o u32 da quantidade).
    pub at: usize,
    rooms: Vec<Room>,
}

impl Bank {
    fn room_at(&self, at: usize) -> Option<&Room> {
        let i = self
            .rooms
            .partition_point(|room| room.text <= at)
            .checked_sub(1)?;
        self.rooms
            .get(i)
            .filter(|room| at < room.text + room.text_len)
    }
}

/// O banco que comeca em `at`, se a estrutura inteira confere.
fn bank_at(data: &[u8], at: usize) -> Option<Bank> {
    let count = u32_at(data, at)?;
    if !DIR_ENTRIES.contains(&count) || count % 2 != 0 {
        return None;
    }
    let dir_len = 4 + 4 * count;
    // O diretorio: zero, ou offsets que nunca diminuem, o primeiro logo
    // depois do proprio diretorio. A checagem barata vem primeiro, sem
    // guardar nada: esta funcao roda em cada posicao alinhada do arquivo.
    let (mut last, mut filled) = (0, 0);
    for i in 0..count {
        let value = u32_at(data, at + 4 + 4 * i)?;
        if value != 0 {
            let floor = if last == 0 { dir_len } else { last };
            let ceiling = if last == 0 {
                dir_len + DIR_SLACK
            } else {
                data.len() - at
            };
            if value < floor || value > ceiling {
                return None;
            }
            last = value;
            filled += 1;
        }
    }
    if filled < 8 {
        return None;
    }
    let dir: Vec<usize> = (0..count)
        .map(|i| u32_at(data, at + 4 + 4 * i))
        .collect::<Option<_>>()?;
    // Onde cada bloco de texto termina: no proximo offset do diretorio.
    let mut bounds: Vec<usize> = dir.iter().copied().filter(|v| *v != 0).collect();
    bounds.dedup();
    let next = |value: usize| {
        let i = bounds.partition_point(|b| *b <= value);
        bounds.get(i).copied()
    };

    let mut by_block: BTreeMap<(usize, usize), Room> = BTreeMap::new();
    let (mut total, mut in_range, mut after_end) = (0usize, 0usize, 0usize);
    for pair in 0..count / 2 {
        let (a, b) = (dir[2 * pair], dir[2 * pair + 1]);
        if a == 0 || b == 0 || a >= b {
            continue;
        }
        if let Some(room) = by_block.get_mut(&(a, b)) {
            room.pairs.push(pair);
            continue;
        }
        let (table, text, slots) = (at + a, at + b, (b - a) / 2);
        let offsets: Vec<u16> = (0..slots)
            .map(|k| u16_at(data, table + 2 * k))
            .collect::<Option<_>>()?;
        // O ultimo bloco nao tem vizinho: vai ate o fim da ultima mensagem.
        let text_len = match next(b) {
            Some(end) => end - b,
            None => {
                let far = offsets.iter().filter(|o| **o != END).max().copied()? as usize;
                let closes = (far..MAX_TEXT)
                    .step_by(2)
                    .find(|rel| u16_at(data, text + rel) == Some(END))?;
                (closes + 2).next_multiple_of(4)
            }
        };
        if text + text_len > data.len() {
            return None;
        }
        let mut starts = vec![0];
        // Tabela e texto colados, e texto que um offset de 16 bits alcanca.
        let mut whole = text_len <= MAX_TEXT + 2 && (b - a) % 2 == 0;
        for &offset in offsets.iter().filter(|o| **o != END) {
            let rel = offset as usize;
            total += 1;
            if rel + 2 > text_len {
                whole = false;
                continue;
            }
            in_range += 1;
            // Mensagem comeca no comeco do bloco ou depois do fim de outra.
            let follows = rel <= 4 || u16_at(data, text + rel - 2) == Some(END);
            after_end += usize::from(follows);
            starts.push(rel);
        }
        starts.sort_unstable();
        starts.dedup();
        by_block.insert(
            (a, b),
            Room {
                pairs: vec![pair],
                table,
                slots,
                text,
                text_len,
                starts: if whole { starts } else { Vec::new() },
            },
        );
    }
    // Quase todo offset dentro do bloco dele, e a maioria logo depois de um
    // fim de mensagem: no jogo medido, 99,9% e 96%.
    let sound = by_block.len() >= 4
        && total >= 16
        && in_range * 100 >= total * 98
        && after_end * 10 >= total * 8;
    sound.then(|| Bank {
        at,
        rooms: by_block.into_values().collect(),
    })
}

/// Os bancos de mensagens de `data`, em ordem de posicao.
pub(crate) fn find(data: &[u8]) -> Vec<Bank> {
    let mut banks = Vec::new();
    let mut at = 0;
    while at + 8 <= data.len() {
        match bank_at(data, at) {
            Some(bank) => {
                at += 4 + 4 * u32_at(data, at).unwrap_or(0);
                banks.push(bank);
            }
            None => at += 4,
        }
    }
    banks
}

/// Onde comeca cada mensagem dos bancos (posicao no arquivo): o que a
/// tabela de cada bloco aponta. E o que impede a leitura de emendar o numero
/// da sala, ou o fim de uma mensagem, na mensagem seguinte.
pub(crate) fn message_starts(banks: &[Bank]) -> std::collections::HashSet<usize> {
    banks
        .iter()
        .flat_map(|bank| &bank.rooms)
        .flat_map(|room| room.starts.iter().map(move |start| room.text + start))
        .collect()
}

/// A entry esta numa mensagem de banco: a traducao dela pode crescer.
pub(crate) fn is_banked(entry: &TextEntry) -> bool {
    entry.metadata.get("bank").is_some_and(|v| v.is_object())
}

/// Uma peca do texto de uma entry: trecho de texto ou codigo em colchetes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Piece<'a> {
    Text(&'a str),
    Code(&'a str),
}

fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let code = rest.strip_prefix('[').and_then(|inside| inside.find(']'));
        let len = match code {
            Some(close) => close + 2,
            // Ate o proximo colchete; um '[' que nao fecha e texto.
            None => rest
                .char_indices()
                .skip(1)
                .find(|&(_, c)| c == '[')
                .map_or(rest.len(), |(i, _)| i),
        };
        out.push(match code {
            Some(_) => Piece::Code(&rest[..len]),
            None => Piece::Text(&rest[..len]),
        });
        at += len;
    }
    out
}

/// O comeco de um codigo, sem os parametros: `[FF23:0002]` -> `[FF23`.
fn head(code: &str) -> &str {
    code.split([':', ']']).next().unwrap_or(code)
}

/// Marca de fala e afins (■◆○★♪※, setas): abre uma linha, nao continua uma
/// frase.
fn is_mark(c: char) -> bool {
    matches!(c, '\u{2190}'..='\u{27BF}' | '\u{203B}')
}

/// O tamanho do trecho mais longo de `text` entre dois codigos em colchetes.
pub(crate) fn longest_line(text: &str) -> usize {
    pieces(text)
        .iter()
        .map(|piece| match piece {
            Piece::Text(text) => text.chars().count(),
            Piece::Code(_) => 0,
        })
        .max()
        .unwrap_or(0)
}

/// Como o jogo arruma o texto na caixa, medido nas mensagens do proprio banco
/// (`learn`) e guardado em cada entry dele. Com isto o app quebra as linhas
/// da traducao sozinho: quem traduz escreve a frase inteira.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    /// Caracteres que cabem numa linha da caixa.
    width: usize,
    /// O codigo que quebra a linha, como aparece no texto: `[FF01]`.
    soft: String,
    /// A linha de baixo abre com um espaco quando a de cima abre com espaco
    /// ou marca de fala: o texto fica alinhado depois da marca.
    indent: bool,
    /// Os codigos do meio da linha que a entry tem, pelo comeco (`[FF23`), e
    /// quanto cada um ocupa na tela (um nome que o jogo escreve ali, ou
    /// nada). Codigo fora daqui fecha a linha.
    wide: HashMap<String, usize>,
}

/// O que `learn` mediu num banco: o `Layout` (com a largura dos codigos do
/// meio da linha que deu pra medir), os codigos que fecham a linha e a maior
/// largura medida, que e a que leva o codigo raro demais pra medir — quebrar a
/// linha cedo nao estraga nada, passar da caixa estraga.
struct Learned {
    layout: Layout,
    ends: HashSet<String>,
    widest: usize,
}

impl Learned {
    /// O `Layout` de uma entry: do banco, so os codigos do meio da linha que
    /// ela tem.
    fn of(&self, source: &str) -> Layout {
        let wide = pieces(source)
            .iter()
            .filter_map(|piece| match *piece {
                Piece::Code(code) if !self.ends.contains(code) => Some(code),
                _ => None,
            })
            .map(|code| {
                let width = self.layout.inline(code).unwrap_or(self.widest);
                (head(code).to_string(), width)
            })
            .collect();
        Layout {
            wide,
            ..self.layout.clone()
        }
    }
}

/// Mede o `Layout` de um banco pelos textos originais dele. `None` = sem
/// prova de que da pra mexer nas linhas: nenhuma quebra de linha achada, ou
/// nenhum sinal de que a caixa rola sozinha.
///
/// - Fecha a linha o codigo sem parametro que separa dois trechos que, juntos,
///   nao caberiam na caixa (em 1 de cada 20 usos ou mais). Codigo do meio da
///   linha nunca separa trechos assim. Os que so aparecem juntos ("espera o
///   botao" + "linha nova") entram em grupo.
/// - A quebra de linha e, dos que fecham a linha, o que mais aparece sozinho.
/// - A caixa rola: tem paragrafo (linhas emendadas so pela quebra) mais alto
///   que o de 9 em cada 10 — texto que o jogo so consegue mostrar rolando.
///   Sem isso, linha a mais na traducao poderia sair da caixa.
/// - Um codigo do meio da linha ocupa o que os autores deixaram pra ele: a
///   largura da caixa menos a linha mais cheia (99 em cada 100) que o tem.
///
/// Medido no jogo real (so contagens): quebra `FF01`, sozinha 10 mil vezes;
/// 19 mil paragrafos, 97% de 1 ou 2 linhas e 540 de 3 a 70; 92% das linhas de
/// baixo abrem com espaco; o comando que escreve o nome de um personagem
/// deixa 6 ou 7 caracteres de folga, o de pausa, nenhum.
fn learn(sources: &[&str], width: usize) -> Option<Learned> {
    let texts: Vec<Vec<Piece>> = sources.iter().map(|source| pieces(source)).collect();
    // Fila de codigos entre dois trechos -> (usos, vezes em que os dois
    // trechos juntos passam da caixa). A chave sao os codigos sem parametro.
    let mut runs: HashMap<Vec<&str>, (usize, usize)> = HashMap::new();
    for text in &texts {
        let mut left: Option<usize> = None;
        let mut key: Vec<&str> = Vec::new();
        for piece in text {
            match *piece {
                Piece::Code(code) if !code.contains(':') => key.push(code),
                Piece::Code(_) => {}
                Piece::Text(right) => {
                    let right = right.chars().count();
                    if let (Some(left), false) = (left, key.is_empty()) {
                        key.sort_unstable();
                        key.dedup();
                        let (uses, over) = runs.entry(std::mem::take(&mut key)).or_default();
                        *uses += 1;
                        *over += usize::from(left + right > width);
                    }
                    key.clear();
                    left = Some(right);
                }
            }
        }
    }
    let breaks = |(uses, over): (usize, usize)| uses >= MIN_RUNS && over * 20 >= uses;
    let alone: HashMap<&str, usize> = runs
        .iter()
        .filter(|(key, seen)| key.len() == 1 && breaks(**seen))
        .map(|(key, seen)| (key[0], seen.0))
        .collect();
    let soft = alone
        .iter()
        .max_by_key(|&(code, uses)| (*uses, *code))
        .map(|(code, _)| *code)?;
    let mut ends: HashSet<&str> = alone.keys().copied().collect();
    for (key, seen) in &runs {
        if key.len() > 1 && breaks(*seen) && !key.iter().any(|code| alone.contains_key(code)) {
            ends.extend(key);
        }
    }

    // Linhas e paragrafos, sabendo o que fecha a linha.
    let mut paragraphs: Vec<usize> = Vec::new();
    let (mut softs, mut indented) = (0usize, 0usize);
    let mut room: HashMap<&str, Vec<usize>> = HashMap::new();
    for text in &texts {
        let mut lines = 1;
        let mut visible = 0;
        let mut inline: Vec<&str> = Vec::new();
        // Linha fechada, a espera do texto seguinte: `true` = por mais que a
        // quebra sozinha.
        let mut closed: Option<bool> = None;
        for piece in text.iter().copied().chain([Piece::Code("")]) {
            match piece {
                Piece::Text(text) => {
                    match closed.take() {
                        Some(false) => {
                            lines += 1;
                            softs += 1;
                            indented += usize::from(text.starts_with(' '));
                        }
                        Some(true) => paragraphs.push(std::mem::replace(&mut lines, 1)),
                        None => {}
                    }
                    visible += text.chars().count();
                }
                Piece::Code(code) if !code.is_empty() && !ends.contains(code) => inline.push(code),
                Piece::Code(code) => {
                    if let ([only], true) = (&inline[..], visible > 0) {
                        room.entry(head(only)).or_default().push(visible);
                    }
                    visible = 0;
                    inline.clear();
                    closed = Some(closed.unwrap_or(false) || code != soft);
                }
            }
        }
        paragraphs.push(lines);
    }
    paragraphs.sort_unstable();
    let usual = paragraphs[(paragraphs.len() * 9).div_ceil(10).saturating_sub(1)];
    let tall = paragraphs.iter().filter(|lines| **lines > usual).count();
    if tall < MIN_TALL || tall * 100 < paragraphs.len() {
        return None;
    }

    let wide: HashMap<String, usize> = room
        .into_iter()
        .filter(|(_, seen)| seen.len() >= MIN_RUNS)
        .map(|(head, mut seen)| {
            seen.sort_unstable();
            let full = seen[(seen.len() * 99).div_ceil(100) - 1];
            (head.to_string(), width.saturating_sub(full))
        })
        .collect();
    Some(Learned {
        widest: wide.values().copied().max().unwrap_or(0),
        ends: ends.iter().map(|code| code.to_string()).collect(),
        layout: Layout {
            width,
            soft: soft.to_string(),
            indent: indented * 2 >= softs,
            wide,
        },
    })
}

impl Layout {
    /// O `Layout` guardado na entry (`tag`), se o banco dela tem um.
    pub(crate) fn of(entry: &TextEntry) -> Option<Self> {
        let bank = entry.metadata.get("bank")?;
        Some(Self {
            width: bank.get("line")?.as_u64()? as usize,
            soft: bank.get("break")?.as_str()?.to_string(),
            indent: bank.get("indent")?.as_bool()?,
            wide: bank
                .get("wide")?
                .as_object()?
                .iter()
                .map(|(head, width)| (head.clone(), width.as_u64().unwrap_or(0) as usize))
                .collect(),
        })
    }

    /// O codigo que quebra a linha: o app tira e poe, a traducao nao precisa
    /// ter os mesmos do original.
    pub(crate) fn soft(&self) -> &str {
        &self.soft
    }

    /// A largura de um codigo do meio da linha; `None` = fecha a linha.
    fn inline(&self, code: &str) -> Option<usize> {
        self.wide.get(head(code)).copied()
    }

    /// Depois de uma quebra de linha a frase continua? Continua se vem letra,
    /// pontuacao ou um codigo do meio da linha; marca de fala, outro codigo
    /// que fecha linha ou o fim do texto, nao.
    fn goes_on(&self, rest: &[Piece]) -> bool {
        for piece in rest {
            match piece {
                Piece::Code(code) => return *code != self.soft && self.inline(code).is_some(),
                Piece::Text(text) => {
                    if let Some(c) = text.chars().find(|c| *c != ' ') {
                        return !is_mark(c);
                    }
                }
            }
        }
        false
    }

    /// `text` sem as quebras de linha que so existem pra caber na caixa: a
    /// quebra no meio de uma frase vira um espaco. E o que vai pro tradutor.
    /// Quebra logo depois de outra (linha em branco) ou de outro codigo que
    /// fecha a linha e de proposito, e fica.
    pub(crate) fn flow(&self, text: &str) -> String {
        let pieces = pieces(text);
        let mut out = String::with_capacity(text.len());
        let mut joined = false;
        for (i, piece) in pieces.iter().enumerate() {
            let mid_line = i > 0
                && match pieces[i - 1] {
                    Piece::Text(_) => true,
                    Piece::Code(code) => self.inline(code).is_some(),
                };
            match *piece {
                Piece::Code(code)
                    if code == self.soft && mid_line && self.goes_on(&pieces[i + 1..]) =>
                {
                    out.truncate(out.trim_end_matches(' ').len());
                    out.push(' ');
                    joined = true;
                }
                Piece::Code(code) => {
                    out.push_str(code);
                    joined = false;
                }
                Piece::Text(text) => {
                    out.push_str(if joined {
                        text.trim_start_matches(' ')
                    } else {
                        text
                    });
                    joined = false;
                }
            }
        }
        out
    }

    /// `text` arrumado na caixa: a linha que passaria da largura e quebrada
    /// entre palavras, com a quebra do proprio jogo. As quebras que a
    /// traducao ja tem ficam onde estao: sao de quem traduziu. Devolve tambem
    /// a linha mais larga que saiu (maior que a caixa = palavra que nao cabe
    /// nela).
    ///
    /// ponytail: a conta comeca do zero no inicio da entry; mensagem que abre
    /// com um comando que escreve (um nome) antes do texto tem a primeira
    /// linha contada a menos. Corrigir = levar a largura desse comando pra ca.
    pub(crate) fn wrap(&self, text: &str) -> (String, usize) {
        let mut line = Line {
            layout: self,
            out: String::with_capacity(text.len() + 16),
            col: 0,
            widest: 0,
            fresh: true,
            lead: false,
            gap: 0,
            word: String::new(),
            word_width: 0,
        };
        for piece in pieces(text) {
            match piece {
                Piece::Text(text) => {
                    for c in text.chars() {
                        if c == ' ' {
                            line.place();
                            line.gap += 1;
                        } else {
                            line.word.push(c);
                            line.word_width += 1;
                        }
                    }
                }
                Piece::Code(code) => match self.inline(code) {
                    Some(width) => {
                        line.word.push_str(code);
                        line.word_width += width;
                    }
                    None => {
                        line.place();
                        line.out.push_str(code);
                        (line.fresh, line.lead, line.gap) = (true, false, 0);
                    }
                },
            }
        }
        line.place();
        (line.out, line.widest)
    }
}

/// A linha que `Layout::wrap` esta montando.
struct Line<'a> {
    layout: &'a Layout,
    out: String,
    /// Largura ja usada na linha, e a maior que uma linha chegou a ter.
    col: usize,
    widest: usize,
    /// A linha de cima do paragrafo ainda esta vazia.
    fresh: bool,
    /// A linha de cima abriu com espaco ou marca de fala: as de baixo levam
    /// o recuo.
    lead: bool,
    /// Espacos vistos antes da palavra que esta chegando.
    gap: usize,
    word: String,
    word_width: usize,
}

impl Line<'_> {
    /// Poe a palavra que chegou na linha, ou na de baixo se ela nao cabe.
    fn place(&mut self) {
        if self.word.is_empty() {
            return;
        }
        if self.fresh {
            // O comeco do paragrafo fica como veio, espacos inclusive.
            self.col = self.gap;
            self.lead = self.gap > 0 || self.word.chars().next().is_some_and(is_mark);
            self.fresh = false;
            self.out.extend(std::iter::repeat_n(' ', self.col));
        } else if self.col + 1 + self.word_width <= self.layout.width {
            self.col += 1;
            self.out.push(' ');
        } else {
            self.col = usize::from(self.layout.indent && self.lead);
            self.out.push_str(&self.layout.soft);
            self.out.extend(std::iter::repeat_n(' ', self.col));
        }
        self.out.push_str(&self.word);
        self.col += self.word_width;
        self.widest = self.widest.max(self.col);
        self.word.clear();
        (self.word_width, self.gap) = (0, 0);
    }
}

/// Marca as entries que estao inteiras dentro de uma mensagem de banco: elas
/// deixam de ter teto de bytes. Devolve quantas marcou.
///
/// Junto vai a largura da caixa de texto, medida no proprio jogo: o tamanho
/// de linha (trecho entre dois comandos) que 999 em cada mil linhas do banco
/// respeitam. No jogo medido sao 31 mil linhas e o corte e seco: 99,96% tem
/// ate 20 caracteres. E, quando da pra medir, o `Layout` do banco: ai o app
/// quebra as linhas da traducao sozinho. Sem ele, cada linha da traducao tem
/// que caber na caixa do jeito que vier.
pub(crate) fn tag(entries: &mut [TextEntry], banks: &[Bank]) -> usize {
    let mut tagged = 0;
    for bank in banks {
        let inside: Vec<(usize, usize)> = entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| {
                let at = entry.offset? as usize;
                let room = bank.room_at(at)?;
                room.segment_of(at, entry.original_bytes.len())?;
                Some((i, room.pairs[0]))
            })
            .collect();
        let sources: Vec<&str> = inside
            .iter()
            .map(|&(i, _)| entries[i].source_text.as_str())
            .collect();
        let mut lines: Vec<usize> = sources
            .iter()
            .flat_map(|source| pieces(source))
            .filter_map(|piece| match piece {
                Piece::Text(text) => Some(text.chars().count()),
                Piece::Code(_) => None,
            })
            .collect();
        lines.sort_unstable();
        // ponytail: com menos de mil linhas o corte de 99,9% e a maior linha.
        let line = lines
            .get((lines.len() * 999).div_ceil(1000).saturating_sub(1))
            .copied();
        let learned = line.and_then(|width| learn(&sources, width));
        for (i, pair) in inside {
            let entry = &mut entries[i];
            entry.metadata["bank"] = serde_json::json!({
                "at": bank.at,
                "pair": pair,
                "line": line,
            });
            if let Some(learned) = &learned {
                let layout = learned.of(&entry.source_text);
                entry.metadata["bank"]["break"] = serde_json::json!(layout.soft);
                entry.metadata["bank"]["indent"] = serde_json::json!(layout.indent);
                entry.metadata["bank"]["wide"] = serde_json::json!(layout.wide);
            }
            entry.max_bytes = None;
            entry.context = Some(match (line, &learned) {
                (Some(line), Some(_)) => format!(
                    "realocavel: a mensagem pode crescer (o bloco e remontado) e o app quebra \
                     as linhas na largura da caixa de texto do jogo ({line} caracteres)"
                ),
                (Some(line), None) => format!(
                    "realocavel: a mensagem pode crescer (o bloco e remontado); cada linha \
                     cabe {line} caracteres na caixa de texto do jogo"
                ),
                (None, _) => {
                    "realocavel: a mensagem pode crescer (o bloco e remontado)".to_string()
                }
            });
            tagged += 1;
        }
    }
    tagged
}

/// A largura da caixa de texto guardada na entry (`tag`), se ha uma.
pub(crate) fn line_width(entry: &TextEntry) -> Option<usize> {
    let line = entry.metadata.get("bank")?.get("line")?.as_u64()?;
    Some(line as usize)
}

pub(crate) struct BankPlan {
    /// (posicao, bytes): blocos remontados, diretorio reapontado e o
    /// preenchimento do lugar dos blocos que sairam.
    pub writes: Vec<(usize, Vec<u8>)>,
    /// (posicao, bytes) de todo bloco de texto citado pelas entries: nenhuma
    /// outra escrita pode cair ai.
    pub rooms: Vec<(usize, usize)>,
    pub report: ApplyReport,
}

impl BankPlan {
    pub fn covers(&self, start: usize, len: usize) -> bool {
        let i = self.rooms.partition_point(|&(at, _)| at < start + len);
        i > 0 && self.rooms[i - 1].0 + self.rooms[i - 1].1 > start
    }
}

/// Um bloco remontado, a espera de lugar.
struct Rebuilt<'a> {
    bank: usize,
    room: &'a Room,
    /// Tabela nova seguida do texto novo.
    bytes: Vec<u8>,
    applied: usize,
}

/// Os trechos de preenchimento (0xFF) de `data` com `MIN_PADDING` ou mais.
fn padding(data: &[u8]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < data.len() {
        if data[at] != 0xFF {
            // Pula de 4 em 4 fora do preenchimento; volta ao byte no achado.
            at += 4;
            continue;
        }
        let mut begin = at;
        while begin > 0 && data[begin - 1] == 0xFF {
            begin -= 1;
        }
        let mut end = at;
        while end < data.len() && data[end] == 0xFF {
            end += 1;
        }
        // Margem nas pontas: o ultimo dado antes do preenchimento pode
        // terminar em 0xFF, e o primeiro depois pode comecar com um.
        let (from, to) = (
            (begin + 16).next_multiple_of(4),
            end.saturating_sub(16) & !3,
        );
        if end - begin >= MIN_PADDING && from < to {
            found.push((from, to - from));
        }
        at = end.next_multiple_of(4);
    }
    found
}

/// Tira de `free` um trecho de `len` bytes (multiplo de 4) depois de `floor`
/// e devolve onde ele comeca. O primeiro que servir.
fn allocate(free: &mut Vec<(usize, usize)>, len: usize, floor: usize) -> Option<usize> {
    let i = free
        .iter()
        .position(|&(at, room)| at >= floor && room >= len)?;
    let (at, room) = free[i];
    if room == len {
        free.remove(i);
    } else {
        free[i] = (at + len, room - len);
    }
    Some(at)
}

/// Devolve um trecho a `free`, colado nos vizinhos.
fn release(free: &mut Vec<(usize, usize)>, at: usize, len: usize) {
    free.push((at, len));
    free.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(free.len());
    for &(at, len) in free.iter() {
        match merged.last_mut() {
            Some(last) if last.0 + last.1 == at => last.1 += len,
            _ => merged.push((at, len)),
        }
    }
    *free = merged;
}

fn drifted(what: &str) -> CoreError {
    CoreError::Project(format!(
        "{what} — arquivo diferente do que foi extraido? Re-extraia antes de reinserir"
    ))
}

/// Remonta os blocos de banco que tem traducao. `data` e a imagem ORIGINAL;
/// entry que nao e de banco e ignorada. As traducoes chegam codificadas na
/// propria entry (`TextEntry::encoded`, posto por `reinsert`).
pub(crate) fn plan(data: &[u8], entries: &[TextEntry]) -> Result<BankPlan> {
    let mut plan = BankPlan {
        writes: Vec::new(),
        rooms: Vec::new(),
        report: ApplyReport::default(),
    };
    // (banco, par) -> entries daquele bloco.
    let mut groups: BTreeMap<(usize, usize), Vec<&TextEntry>> = BTreeMap::new();
    for entry in entries.iter().filter(|e| is_banked(e)) {
        let number = |key: &str| {
            entry.metadata["bank"]
                .get(key)?
                .as_u64()
                .map(|n| n as usize)
        };
        let (Some(at), Some(pair)) = (number("at"), number("pair")) else {
            continue;
        };
        groups.entry((at, pair)).or_default().push(entry);
    }
    if groups.is_empty() {
        return Ok(plan);
    }
    let banks: HashMap<usize, Bank> = find(data).into_iter().map(|b| (b.at, b)).collect();

    let mut rebuilt: Vec<Rebuilt> = Vec::new();
    for ((at, pair), mut group) in groups {
        let room = banks
            .get(&at)
            .and_then(|bank| bank.rooms.iter().find(|room| room.pairs.contains(&pair)))
            .filter(|room| !room.starts.is_empty())
            .ok_or_else(|| {
                drifted(&format!(
                    "o bloco de mensagens {pair} do banco em 0x{at:X} nao esta mais la"
                ))
            })?;
        plan.rooms.push((room.text, room.text_len));
        if group.iter().all(|e| e.translated_text.is_none()) {
            plan.report.kept_original += group.len();
            continue;
        }
        group.sort_by_key(|e| e.offset);

        // O texto novo, mensagem por mensagem, e onde cada uma comeca.
        let mut text = Vec::with_capacity(room.text_len);
        let mut moved: HashMap<usize, usize> = HashMap::new();
        let mut pending = group.iter().peekable();
        let mut applied = 0;
        for i in 0..room.starts.len() {
            let (begin, end) = room.segment(i);
            moved.insert(begin, text.len());
            let mut cursor = begin;
            while let Some(entry) = pending.peek() {
                let from = entry.offset.unwrap_or(0) as usize;
                let len = entry.original_bytes.len();
                let Some(rel) = from.checked_sub(room.text).filter(|rel| rel + len <= end) else {
                    break;
                };
                if rel < cursor || data[from..from + len] != entry.original_bytes[..] {
                    return Err(drifted(&format!(
                        "os bytes em 0x{from:X} nao batem com a entry {}",
                        entry.id
                    )));
                }
                text.extend_from_slice(&data[room.text + cursor..from]);
                match (&entry.translated_text, entry.encoded()) {
                    (Some(_), Some(bytes)) => {
                        text.extend_from_slice(&bytes);
                        applied += 1;
                    }
                    (Some(_), None) => {
                        return Err(CoreError::Project(format!(
                            "entry {}: a traducao chegou sem codificar — a reinsercao de \
                             mensagem de banco passa por reinsert_project, que le a tabela do \
                             projeto",
                            entry.id
                        )))
                    }
                    (None, _) => {
                        text.extend_from_slice(&entry.original_bytes);
                        plan.report.kept_original += 1;
                    }
                }
                cursor = rel + len;
                pending.next();
            }
            text.extend_from_slice(&data[room.text + cursor..room.text + end]);
        }
        if pending.next().is_some() {
            return Err(drifted(&format!(
                "uma entry do bloco de mensagens {pair} nao cabe em mensagem nenhuma dele"
            )));
        }

        // A tabela, com cada offset levado pro comeco novo da mensagem dele.
        let mut bytes = Vec::with_capacity(2 * room.slots + text.len() + 2);
        let mut fits = text.len() <= MAX_TEXT;
        for k in 0..room.slots {
            let old = u16_at(data, room.table + 2 * k).unwrap_or(END);
            let new = match moved.get(&(old as usize)) {
                _ if old == END => END,
                Some(&new) if new < END as usize => new as u16,
                _ => {
                    fits = false;
                    END
                }
            };
            bytes.extend_from_slice(&new.to_le_bytes());
        }
        if !fits {
            // Passou do que um offset de 16 bits alcanca: o bloco fica como
            // no original.
            plan.report.no_room += applied;
            continue;
        }
        bytes.extend_from_slice(&text);
        bytes.resize(bytes.len().next_multiple_of(4), 0xFF);
        rebuilt.push(Rebuilt {
            bank: at,
            room,
            bytes,
            applied,
        });
    }

    // Quem cabe onde estava fica; o resto disputa o espaco livre, os maiores
    // primeiro, e o lugar de quem sai vira espaco livre pros outros.
    let mut free = padding(data);
    let (stay, mut leave): (Vec<Rebuilt>, Vec<Rebuilt>) = rebuilt
        .into_iter()
        .partition(|block| block.bytes.len() <= block.room.size());
    for block in stay {
        let mut bytes = block.bytes;
        bytes.resize(block.room.size(), 0xFF);
        plan.writes.push((block.room.table, bytes));
        plan.report.applied += block.applied;
    }
    leave.sort_by_key(|block| std::cmp::Reverse(block.bytes.len()));
    let mut placed = true;
    while placed && !leave.is_empty() {
        placed = false;
        let mut waiting = Vec::new();
        for block in leave {
            let Some(at) = allocate(&mut free, block.bytes.len(), block.bank) else {
                waiting.push(block);
                continue;
            };
            placed = true;
            let room = block.room;
            let (table, text) = (at - block.bank, at + 2 * room.slots - block.bank);
            for &pair in &room.pairs {
                let mut entry = (table as u32).to_le_bytes().to_vec();
                entry.extend_from_slice(&(text as u32).to_le_bytes());
                plan.writes.push((block.bank + 4 + 8 * pair, entry));
            }
            // O lugar antigo e limpo agora e pode ser de outro bloco depois:
            // a ordem das escritas garante que o bloco novo fica por cima.
            plan.writes.push((room.table, vec![0xFF; room.size()]));
            plan.writes.push((at, block.bytes));
            release(&mut free, room.table, room.size() & !3);
            plan.report.applied += block.applied;
            plan.report.relocated += block.applied;
        }
        leave = waiting;
    }
    plan.report.no_room += leave.iter().map(|block| block.applied).sum::<usize>();
    plan.rooms.sort_unstable();
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_is_handed_out_and_taken_back() {
        let mut free = vec![(0x100, 0x40), (0x400, 0x100)];
        // O primeiro que serve, depois do piso.
        assert_eq!(allocate(&mut free, 0x80, 0), Some(0x400));
        assert_eq!(allocate(&mut free, 0x20, 0x200), Some(0x480));
        assert_eq!(allocate(&mut free, 0x40, 0), Some(0x100));
        assert_eq!(allocate(&mut free, 0x1000, 0), None);
        assert_eq!(free, [(0x4A0, 0x60)]);
        // O que volta cola no vizinho.
        release(&mut free, 0x480, 0x20);
        release(&mut free, 0x100, 0x40);
        assert_eq!(free, [(0x100, 0x40), (0x480, 0x80)]);
    }

    #[test]
    fn only_long_runs_of_ff_are_free_space() {
        let mut data = vec![0u8; 3 * MIN_PADDING];
        data[0x100..0x200].fill(0xFF); // curto: pode ser dado
        data[MIN_PADDING..2 * MIN_PADDING + 8].fill(0xFF);
        let free = padding(&data);
        assert_eq!(free.len(), 1);
        let (at, len) = free[0];
        assert!(at >= MIN_PADDING + 16 && at % 4 == 0);
        assert!(at + len <= 2 * MIN_PADDING + 8 - 16);
        assert!(padding(&[]).is_empty());
    }

    #[test]
    fn the_longest_line_ignores_the_codes() {
        assert_eq!(longest_line("abc[FF01]defgh[FF03][FF00]ij"), 5);
        assert_eq!(longest_line("[FF01]"), 0);
        assert_eq!(longest_line("sem codigo"), 10);
    }

    fn layout() -> Layout {
        Layout {
            width: 12,
            soft: "[FF01]".to_string(),
            indent: true,
            wide: HashMap::from([("[FF23".to_string(), 6), ("[FF04".to_string(), 0)]),
        }
    }

    #[test]
    fn the_sentence_flows_and_the_marks_keep_their_lines() {
        let layout = layout();
        // A quebra no meio da frase vira um espaco so, com ou sem recuo; a
        // que vem antes de uma marca de fala, de outro codigo que fecha a
        // linha ou do fim, fica.
        assert_eq!(
            layout.flow("◆um dois[FF01] tres[FF01]quatro[FF01]◆cinco[FF03][FF00] seis[FF01]"),
            "◆um dois tres quatro[FF01]◆cinco[FF03][FF00] seis[FF01]"
        );
        // Linha em branco (duas quebras) e quebra depois de outro codigo que
        // fecha a linha sao de proposito.
        assert_eq!(layout.flow("um[FF01][FF01] dois"), "um[FF01][FF01] dois");
        assert_eq!(
            layout.flow("um[FF03][FF00][FF01]dois"),
            "um[FF03][FF00][FF01]dois"
        );
        // Codigo do meio da linha continua a frase.
        assert_eq!(layout.flow("de[FF01] [FF23:0002]！"), "de [FF23:0002]！");
        assert_eq!(layout.flow("sem quebra"), "sem quebra");
    }

    #[test]
    fn the_lines_are_broken_between_words_inside_the_box() {
        let layout = layout();
        // Com marca: as linhas de baixo levam o recuo. Depois do codigo que
        // fecha a linha a conta recomeca, e o espaco do comeco fica.
        let (text, widest) = layout.wrap("◆um dois tres quatro cinco[FF03][FF00] seis sete oito");
        assert_eq!(
            text,
            "◆um dois[FF01] tres quatro[FF01] cinco[FF03][FF00] seis sete[FF01] oito"
        );
        assert_eq!(widest, 12);
        // Sem marca nem espaco no comeco, sem recuo.
        assert_eq!(
            layout.wrap("um dois tres quatro").0,
            "um dois tres[FF01]quatro"
        );
        // O codigo que escreve um nome ocupa a largura dele; a pausa, nada.
        assert_eq!(
            layout.wrap("◆oi [FF23:0002], tudo[FF04:001E] bem?").0,
            "◆oi [FF23:0002],[FF01] tudo[FF04:001E] bem?"
        );
        // A quebra que a traducao ja tem fica, com o que vem depois dela; a
        // palavra maior que a caixa fica inteira e aparece na conta.
        assert_eq!(
            layout.wrap("◆um[FF01] dois tres quatro[FF01]cinco").0,
            "◆um[FF01] dois tres[FF01] quatro[FF01]cinco"
        );
        let (text, widest) = layout.wrap("◆paralelepipedos sim");
        assert_eq!((text.as_str(), widest), ("◆paralelepipedos[FF01] sim", 16));
        assert_eq!(layout.wrap("").0, "");
    }

    #[test]
    fn garbage_is_not_a_bank() {
        assert!(find(&[]).is_empty());
        assert!(find(&[0xFF; 4096]).is_empty());
        assert!(find(&crate::synth::make_random(64 * 1024, 11)).is_empty());
        // Um diretorio que promete mais do que o arquivo tem.
        let mut lying = vec![0u8; 256];
        lying[..4].copy_from_slice(&16u32.to_le_bytes());
        lying[4..8].copy_from_slice(&68u32.to_le_bytes());
        lying[8..12].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
        assert!(find(&lying).is_empty());
    }
}

//! Arquivo de mensagens BMG ("MESGbmg1"), o da Nintendo — medido no Animal
//! Crossing: Wild World (DS, ADME) em 25/10/2026, so contagens: 1.791
//! arquivos, 19.650 mensagens, 1,89 milhao de letras.
//!
//! - Cabecalho de 32 bytes: "MESGbmg1", tamanho do arquivo, quantas secoes e
//!   a codificacao (1 = uma letra por byte, Windows-1252; as outras ficam de
//!   fora). Secoes alinhadas em 32 bytes, completadas com zeros: INF1
//!   (quantidade u16, tamanho da entrada u16, u32, e entradas que comecam pela
//!   posicao u32 da mensagem em DAT1) e DAT1 (mensagens terminadas em 0x00; a
//!   posicao 0 e a mensagem vazia).
//! - Codigo do jogo: 0x1A, o tamanho dele inteiro em um byte, e os dados — o
//!   primeiro e o tipo (no jogo medido: 01 pausa, 07 expressao, 04 nome que o
//!   jogo escreve, 02 escolha). Vai pra quem traduz em hexa entre colchetes:
//!   `[0100000500]`.
//! - No cartucho o arquivo vem cru, em LZ77 ("LZ77" + bloco 0x10) ou em
//!   blocos: "LZ77", um byte (0xF7), o tamanho em 24 bits, o fim de cada
//!   bloco (u16, contado do fim da lista) e blocos de 4 KiB, cada um LZ77 ou
//!   cru (00 + 3 bytes + os dados). Volta do jeito que veio.
//!
//! O jogo nao quebra a linha: o texto ja vem quebrado, e a largura e em
//! pixels (a de cada letra vem da fonte do jogo, `font_widths`). A caixa de
//! fala (pasta `message`) mostra 3 linhas por vez, e linha em branco completa
//! a pagina antes da frase seguinte (das 4.987 linhas em branco do jogo
//! medido, 40 fora dessa regra). Pra quem traduz a fala vai corrida, com
//! `[p]` onde o original pula pra pagina nova; o app quebra as linhas na
//! largura da caixa e completa as paginas (`layout`). Carta e mural (`mail`,
//! `mailz`, `bbs`) tambem vao corridos, com `[p]` = linha em branco e o teto
//! de linhas do arquivo. O resto (menus, listas) guarda as linhas: `[n]`.
//! Depois de uma escolha (codigo 02) vem uma opcao por linha: tambem `[n]`.
//!
//! A largura de cada grupo (todas as falas; cada pasta de carta e mural; cada
//! arquivo de lista) e a linha mais larga dele no original. O nome que o jogo
//! escreve ocupa o menor espaco que o original deixou pra ele no grupo.
//!
//! Nome de item e movel mora fora do BMG, em tabela de registro fixo
//! (`name_table`): vai no campo dele, no lugar.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use crate::adapter::ApplyReport;
use crate::error::{CoreError, Result};
use crate::lz77;
use crate::types::{TextEncoding, TextEntry};

const MAGIC: &[u8] = b"MESGbmg1";
const ESC: u8 = 0x1A;
const CHUNK: usize = 4096;
const CHOICE: u8 = 0x02;
/// Codigo que o jogo troca por um nome (em hexa, o comeco do codigo).
const WRITES: &str = "04";
const SPEECH_LINES: usize = 3;
/// Quebra de linha guardada, e pagina nova (ou linha em branco), como vao pra
/// quem traduz.
pub(crate) const NL: &str = "[n]";
pub(crate) const PAGE: &str = "[p]";
/// Letra sem largura na fonte (ou jogo sem a fonte achada).
const DEFAULT_WIDTH: u8 = 6;

/// Como o arquivo vem guardado no cartucho.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Pack {
    Raw,
    Lz,
    Chunks(u8),
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// O BMG de dentro do arquivo do cartucho, e como ele estava guardado.
fn open(file: &[u8]) -> Option<(Vec<u8>, Pack)> {
    if file.starts_with(MAGIC) {
        return Some((file.to_vec(), Pack::Raw));
    }
    let rest = file.strip_prefix(b"LZ77")?;
    let (data, pack) = match *rest.first()? {
        0x10 => (lz77::unpack(rest)?.data, Pack::Lz),
        flag => {
            let size = u32_at(&[rest.get(1..4)?, &[0]].concat(), 0)?;
            let base = 4 + 2 * size.div_ceil(CHUNK);
            let mut out = Vec::with_capacity(size);
            let mut start = 0;
            for k in 0..size.div_ceil(CHUNK) {
                let end = u16_at(rest, 4 + 2 * k)?;
                let chunk = rest.get(base + start..base + end)?;
                match *chunk.first()? {
                    0x00 => out.extend_from_slice(chunk.get(4..)?),
                    _ => out.extend_from_slice(&lz77::unpack(chunk)?.data),
                }
                start = end;
            }
            (out, Pack::Chunks(flag))
        }
    };
    (data.starts_with(MAGIC) && u32_at(&data, 8)? == data.len()).then_some((data, pack))
}

/// O BMG guardado de novo do jeito que veio. `None` = nao cabe no formato.
fn close(bmg: &[u8], pack: Pack) -> Option<Vec<u8>> {
    match pack {
        Pack::Raw => Some(bmg.to_vec()),
        Pack::Lz => Some([b"LZ77".as_slice(), &lz77::pack(bmg)?].concat()),
        Pack::Chunks(flag) => {
            let chunks: Vec<Vec<u8>> = bmg.chunks(CHUNK).map(lz77::pack).collect::<Option<_>>()?;
            let mut out = b"LZ77".to_vec();
            out.push(flag);
            out.extend_from_slice(&(bmg.len() as u32).to_le_bytes()[..3]);
            let mut end = 0;
            for chunk in &chunks {
                end += chunk.len();
                out.extend_from_slice(&u16::try_from(end).ok()?.to_le_bytes());
            }
            chunks.iter().for_each(|chunk| out.extend_from_slice(chunk));
            Some(out)
        }
    }
}

/// Posicoes do BMG descomprimido: (entradas do INF1, tamanho de cada uma,
/// quantas, inicio dos dados do DAT1, fim da secao DAT1).
struct Layout {
    entries: usize,
    entry_len: usize,
    count: usize,
    data: usize,
    data_end: usize,
}

/// As mensagens (bytes sem o 0x00 final), na ordem do INF1. `None` = nao e
/// BMG de uma letra por byte, ou os campos nao fecham.
fn parse(bmg: &[u8]) -> Option<(Layout, Vec<Range<usize>>)> {
    if !bmg.starts_with(MAGIC) || *bmg.get(16)? != 1 {
        return None;
    }
    let (mut at, mut inf, mut dat) = (32, None, None);
    for _ in 0..u32_at(bmg, 12)? {
        let len = u32_at(bmg, at + 4)?;
        match bmg.get(at..at + 4)? {
            b"INF1" => inf = Some(at),
            b"DAT1" => dat = Some((at, at + len)),
            _ => {}
        }
        at = at.checked_add(len.max(8))?;
    }
    let (inf, (dat, data_end)) = (inf?, dat?);
    // A remontagem copia tudo ate o DAT1: o INF1 tem que vir antes.
    if inf > dat {
        return None;
    }
    let layout = Layout {
        entries: inf + 16,
        entry_len: u16_at(bmg, inf + 10)?,
        count: u16_at(bmg, inf + 8)?,
        data: dat + 8,
        data_end: data_end.min(bmg.len()),
    };
    if layout.entry_len < 4 {
        return None;
    }
    let messages = (0..layout.count)
        .map(|k| {
            let start = layout.data + u32_at(bmg, layout.entries + k * layout.entry_len)?;
            let mut end = start;
            while *bmg.get(end)? != 0 {
                end += match bmg[end] {
                    ESC => (*bmg.get(end + 1)? as usize).max(2),
                    _ => 1,
                };
            }
            (end <= layout.data_end).then_some(start..end)
        })
        .collect::<Option<Vec<_>>>()?;
    Some((layout, messages))
}

/// O BMG com `texts` (posicao no INF1 -> bytes) no lugar das mensagens; o DAT1
/// e remontado na ordem do INF1 e as secoes depois dele andam junto.
fn rebuild(
    bmg: &[u8],
    layout: &Layout,
    messages: &[Range<usize>],
    texts: &BTreeMap<usize, Vec<u8>>,
) -> Vec<u8> {
    let mut data = vec![0u8];
    let mut out = bmg[..layout.data].to_vec();
    for (k, m) in messages.iter().enumerate() {
        let text = texts.get(&k).map_or(&bmg[m.clone()], Vec::as_slice);
        let at = if text.is_empty() { 0 } else { data.len() };
        if !text.is_empty() {
            data.extend_from_slice(text);
            data.push(0);
        }
        let entry = layout.entries + k * layout.entry_len;
        out[entry..entry + 4].copy_from_slice(&(at as u32).to_le_bytes());
    }
    data.resize((8 + data.len()).next_multiple_of(32) - 8, 0);
    let dat = layout.data - 8;
    out.extend_from_slice(&data);
    out[dat + 4..dat + 8].copy_from_slice(&((8 + data.len()) as u32).to_le_bytes());
    out.extend_from_slice(&bmg[layout.data_end..]);
    let total = out.len() as u32;
    out[8..12].copy_from_slice(&total.to_le_bytes());
    out
}

// --- Letras: Windows-1252 -------------------------------------------------

const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
];

fn decode(b: u8) -> char {
    match b {
        0x80..=0x9F => CP1252_HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

fn encode(c: char) -> Option<u8> {
    match c as u32 {
        0..=0x7F | 0xA0..=0xFF => Some(c as u8),
        _ => CP1252_HIGH
            .iter()
            .position(|&h| h == c)
            .map(|i| 0x80 + i as u8),
    }
}

/// Largura de cada letra (codigo Windows-1252) em pixels, do arquivo de
/// atributos da fonte do jogo medido (`font/fontA_attr.bin`): pares u16
/// (codigo, largura), crus ou em "LZ77" + bloco 0x10.
pub(crate) fn font_widths(file: &[u8]) -> Option<HashMap<u8, u8>> {
    let table = match file.strip_prefix(b"LZ77") {
        Some(rest) => lz77::unpack(rest)?.data,
        None => file.to_vec(),
    };
    let pairs: Vec<(usize, usize)> = (0..table.len() / 4)
        .map(|k| Some((u16_at(&table, 4 * k)?, u16_at(&table, 4 * k + 2)?)))
        .collect::<Option<_>>()?;
    let sane = pairs.len() >= 64
        && pairs.windows(2).all(|w| w[0].0 < w[1].0)
        && pairs
            .iter()
            .all(|&(code, width)| code < 0x100 && width < 32);
    sane.then(|| pairs.iter().map(|&(c, w)| (c as u8, w as u8)).collect())
}

// --- Texto pra quem traduz ------------------------------------------------

enum Bit<'a> {
    Text(&'a [u8]),
    /// Os dados do codigo, sem o 0x1A e o tamanho.
    Code(&'a [u8]),
    Line,
}

fn bits(raw: &[u8]) -> Option<Vec<Bit<'_>>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        match raw[at] {
            ESC => {
                let len = *raw.get(at + 1)? as usize;
                out.push(Bit::Code(raw.get(at + 2..at + len.max(2))?));
                at += len.max(2);
            }
            b'\n' => {
                out.push(Bit::Line);
                at += 1;
            }
            _ => {
                let end = raw[at..]
                    .iter()
                    .position(|&b| b == ESC || b == b'\n')
                    .map_or(raw.len(), |p| at + p);
                out.push(Bit::Text(&raw[at..end]));
                at = end;
            }
        }
    }
    Some(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || !text.len().is_multiple_of(2) || text.len() > 2 * 253 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

fn render(bit: &Bit) -> String {
    match bit {
        Bit::Text(t) => t.iter().map(|&b| decode(b)).collect(),
        Bit::Code(c) => format!("[{}]", hex(c)),
        Bit::Line => NL.to_string(),
    }
}

/// Como o texto do arquivo e arrumado na tela.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// Caixa de fala: linhas corridas, paginas de 3.
    Speech,
    /// Carta, mural: linhas corridas, `[p]` = linha em branco.
    Flow,
    /// Menu, lista: as linhas como estao.
    Lines,
}

fn mode(path: &str) -> Mode {
    let dirs: Vec<&str> = path.split('/').rev().skip(1).collect();
    if dirs.contains(&"message") {
        Mode::Speech
    } else if dirs.iter().any(|d| matches!(*d, "mail" | "mailz" | "bbs")) {
        Mode::Flow
    } else {
        Mode::Lines
    }
}

/// (antes, frase, depois): a frase pra quem traduz e os bytes crus das duas
/// pontas (codigos e quebras), que nao vao pra ela.
fn frame(raw: &[u8], mode: Mode) -> Option<(Vec<u8>, String, Vec<u8>)> {
    let bits = bits(raw)?;
    let edge = |b: &Bit| !matches!(b, Bit::Text(t) if t.iter().any(|&c| c != b' '));
    let lead = bits.iter().take_while(|b| edge(b)).count();
    let tail = bits[lead..].iter().rev().take_while(|b| edge(b)).count();
    let middle = &bits[lead..bits.len() - tail];
    let size = |bits: &[Bit]| -> usize {
        bits.iter()
            .map(|b| match b {
                Bit::Text(t) => t.len(),
                Bit::Code(c) => c.len() + 2,
                Bit::Line => 1,
            })
            .sum()
    };
    let before = raw[..size(&bits[..lead])].to_vec();
    let after = raw[raw.len() - size(&bits[bits.len() - tail..])..].to_vec();
    let text = match mode {
        Mode::Lines => middle.iter().map(render).collect(),
        Mode::Speech | Mode::Flow => flowed(middle),
    };
    Some((before, text, after))
}

/// A fala corrida: linhas viram espaco, linha sem letra vira `[p]` (os
/// codigos dela vao pro comeco da linha seguinte), e depois de uma escolha
/// cada opcao fica na linha dela (`[n]`).
fn flowed(bits: &[Bit]) -> String {
    let lines: Vec<&[Bit]> = bits.split(|b| matches!(b, Bit::Line)).collect();
    let (mut out, mut carry, mut page, mut choice) = (String::new(), String::new(), false, false);
    for line in lines {
        let text: String = line.iter().map(render).collect();
        if choice {
            out.push_str(NL);
            out.push_str(&text);
            continue;
        }
        let visible = line
            .iter()
            .any(|b| matches!(b, Bit::Text(t) if t.iter().any(|&c| c != b' ')));
        if !visible {
            page |= !out.is_empty();
            carry.push_str(text.trim());
            continue;
        }
        if !out.is_empty() {
            out.push_str(if page { PAGE } else { " " });
        }
        out.push_str(&carry);
        out.push_str(text.trim());
        carry.clear();
        page = false;
        choice = line
            .iter()
            .any(|b| matches!(b, Bit::Code(c) if c.first() == Some(&CHOICE)));
    }
    out + &carry
}

/// Os pedacos de um texto pra quem traduz: (e codigo do jogo?, trecho).
/// `[n]` e `[p]` sao codigos; colchete que nao e codigo fica como texto.
fn tokens(text: &str) -> Vec<(bool, &str)> {
    let mut out: Vec<(bool, &str)> = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let code = rest
            .starts_with('[')
            .then(|| rest.find(']'))
            .flatten()
            .filter(|&close| {
                let inside = &rest[1..close];
                inside == "n" || inside == "p" || unhex(inside).is_some()
            });
        let len = match code {
            Some(close) => close + 1,
            None => rest
                .find('[')
                .filter(|&p| p > 0)
                .unwrap_or(rest.chars().next().map_or(1, char::len_utf8)),
        };
        out.push((code.is_some(), &rest[..len]));
        at += len;
    }
    out
}

/// Bytes crus do texto pra quem traduz (sem `[p]`: quem paginou foi
/// `layout`). `Err(letra)` = letra que o jogo nao escreve.
fn unframe(text: &str) -> std::result::Result<Vec<u8>, char> {
    let mut out = Vec::with_capacity(text.len());
    for (code, piece) in tokens(text) {
        match (code, piece) {
            (true, "[n]") => out.push(b'\n'),
            (true, "[p]") => out.extend_from_slice(b"\n\n"),
            (true, _) => {
                let data = unhex(&piece[1..piece.len() - 1]).unwrap_or_default();
                out.extend_from_slice(&[ESC, data.len() as u8 + 2]);
                out.extend_from_slice(&data);
            }
            (false, _) => {
                for c in piece.chars() {
                    out.push(encode(c).filter(|&b| b != ESC && b != 0).ok_or(c)?);
                }
            }
        }
    }
    Ok(out)
}

// --- Largura --------------------------------------------------------------

/// O que a entry guarda pra quebrar as linhas: largura e teto de linhas do
/// grupo, a largura de cada letra (as que fogem da mais comum) e o espaco de
/// cada codigo que o jogo troca por um nome.
struct Box {
    mode: Mode,
    width: usize,
    lines: usize,
    /// Lista: letras da linha mais longa do arquivo original.
    chars: usize,
    common: usize,
    letters: HashMap<char, usize>,
    codes: HashMap<String, usize>,
}

impl Box {
    fn of(entry: &TextEntry) -> Option<Box> {
        let meta = entry.metadata.get("bmg")?;
        let mode = match meta.get("modo")?.as_str()? {
            "fala" => Mode::Speech,
            "corrido" => Mode::Flow,
            _ => Mode::Lines,
        };
        let num = |key: &str| meta.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let mut letters = HashMap::new();
        for (width, chars) in meta
            .get("px")
            .and_then(|v| v.as_object())
            .into_iter()
            .flatten()
        {
            let width = width.parse().unwrap_or(0);
            chars.as_str().unwrap_or_default().chars().for_each(|c| {
                letters.insert(c, width);
            });
        }
        let codes = meta
            .get("nomes")
            .and_then(|v| v.as_object())
            .into_iter()
            .flatten()
            .map(|(k, v)| (format!("[{k}]"), v.as_u64().unwrap_or(0) as usize))
            .collect();
        Some(Box {
            mode,
            width: num("largura"),
            lines: num("linhas"),
            chars: num("letras"),
            common: num("padrao").max(1),
            letters,
            codes,
        })
    }

    fn px(&self, text: &str) -> usize {
        tokens(text)
            .iter()
            .map(|&(code, piece)| match code {
                true => self.codes.get(piece).copied().unwrap_or(0),
                false => piece
                    .chars()
                    .map(|c| *self.letters.get(&c).unwrap_or(&self.common))
                    .sum(),
            })
            .sum()
    }

    /// As linhas do texto na tela: corrido, quebrado no ultimo espaco antes da
    /// largura, com as paginas completadas; ou como esta, em `[n]`.
    fn layout(&self, text: &str) -> Vec<String> {
        if self.mode == Mode::Lines {
            return text.split(NL).map(str::to_string).collect();
        }
        // Escolha: o que vem depois e uma opcao por linha.
        let split = tokens(text)
            .iter()
            .scan(0, |at, &(code, piece)| {
                *at += piece.len();
                Some((*at, code && piece.starts_with("[02")))
            })
            .find(|&(_, choice)| choice)
            .map_or(text.len(), |(at, _)| at);
        let (head, tail) = text.split_at(split);
        let mut lines: Vec<String> = Vec::new();
        for (k, part) in head.split(PAGE).enumerate() {
            if k > 0 {
                match self.mode {
                    Mode::Speech => {
                        while !lines.len().is_multiple_of(SPEECH_LINES) {
                            lines.push(String::new());
                        }
                    }
                    _ => lines.push(String::new()),
                }
            }
            let mut line = String::new();
            for word in part.split([' ', '\u{a0}']).filter(|w| !w.is_empty()) {
                for (i, piece) in word.split(NL).enumerate() {
                    let wider = !line.is_empty()
                        && self.px(&line) + self.px(" ") + self.px(piece) > self.width;
                    if (i > 0 || wider) && !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                    } else if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(piece);
                }
            }
            if !line.is_empty() {
                lines.push(line);
            }
        }
        let mut tail = tail.split(NL);
        if let (Some(first), Some(last)) = (tail.next(), lines.last_mut()) {
            last.push_str(first);
        }
        lines.extend(tail.map(str::to_string));
        lines
    }
}

/// O texto como vai pro jogo: as linhas quebradas pelo app, em `[n]`.
pub(crate) fn wrapped(entry: &TextEntry, text: &str) -> Option<String> {
    let b = Box::of(entry)?;
    Some(b.layout(text).join(NL))
}

/// A entry e uma mensagem de BMG ou um nome de tabela de registro fixo.
pub(crate) fn is_bmg(entry: &TextEntry) -> bool {
    entry.metadata.get("bmg").is_some_and(|v| v.is_object())
}

/// O `[p]` das falas corridas, que a validacao nao conta como codigo: o app
/// pagina sozinho.
pub(crate) fn newline(entry: &TextEntry) -> Option<&'static str> {
    (Box::of(entry)?.mode != Mode::Lines).then_some(PAGE)
}

/// Teto em letras pra quem traduz: o campo do nome.
pub(crate) fn max_chars(entry: &TextEntry) -> Option<usize> {
    let field = entry.metadata.get("bmg")?.get("campo")?.as_u64()? as usize;
    Some(field - 1)
}

/// Teto em letras pra quem traduz: o campo do nome, ou as linhas da carta
/// em letras da largura mais comum (a conta de verdade e em pixels).
pub(crate) fn hint_chars(entry: &TextEntry) -> Option<usize> {
    max_chars(entry).or_else(|| {
        let b = Box::of(entry)?;
        (b.mode == Mode::Flow && b.lines > 0).then(|| b.lines * b.width / b.common)
    })
}

/// Linha da lista em letras da largura mais comum, pra quem traduz.
pub(crate) fn max_line(entry: &TextEntry) -> Option<usize> {
    let b = Box::of(entry)?;
    (b.mode == Mode::Lines).then_some(b.chars)
}

/// Letras de uma linha, sem os codigos.
fn letters(line: &str) -> usize {
    tokens(line)
        .iter()
        .filter(|(code, _)| !code)
        .map(|(_, piece)| piece.chars().count())
        .sum()
}

/// A letra que o jogo nao escreve (fora do Windows-1252), se ha uma.
pub(crate) fn unencodable(text: &str) -> Option<char> {
    text.chars().find(|&c| encode(c).is_none_or(|b| b < 0x20))
}

/// O que nao cabe no jogo: (e erro?, motivo).
pub(crate) fn problems(entry: &TextEntry, text: &str) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    if let Some(bad) = unencodable(text) {
        out.push((true, format!("a letra {bad:?} nao existe na fonte do jogo")));
    }
    if let Some(field) = max_chars(entry) {
        let n = text.chars().count();
        if n > field {
            out.push((
                true,
                format!("o nome tem {n} letras; o campo do jogo guarda {field}"),
            ));
        }
        return out;
    }
    let Some(b) = Box::of(entry) else { return out };
    let lines = b.layout(text);
    // Lista (bordao, apelido...): o jogo pode copiar a linha pra um campo de
    // tamanho fixo do save; o teto e a mais longa do arquivo original.
    if let Some(long) = lines
        .iter()
        .find(|l| b.mode == Mode::Lines && letters(l) > b.chars)
    {
        out.push((
            true,
            format!(
                "a linha \"{long}\" tem {} letras; a mais longa deste arquivo no original tem {} \
                 (o jogo pode guardar o texto num campo de tamanho fixo) — encurte",
                letters(long),
                b.chars
            ),
        ));
    }
    if let Some(wide) = lines.iter().find(|l| b.px(l) > b.width) {
        out.push((
            b.mode == Mode::Lines,
            format!(
                "a linha \"{wide}\" ocupa {} pixels; a caixa do jogo mostra {} — {}",
                b.px(wide),
                b.width,
                if b.mode == Mode::Lines {
                    "encurte a linha"
                } else {
                    "uma palavra sozinha passa da borda"
                }
            ),
        ));
    }
    if b.lines > 0 && lines.len() > b.lines {
        out.push((
            true,
            format!(
                "a traducao ocupa {} linhas; esta caixa do jogo mostra {} — encurte",
                lines.len(),
                b.lines
            ),
        ));
    }
    out
}

// --- Extracao -------------------------------------------------------------

/// Grupo de medida da largura: as falas juntas, cada pasta de carta e
/// mural, e cada arquivo de lista.
fn group(path: &str, mode: Mode) -> String {
    let dir = || path.rsplit_once('/').map_or("", |(dir, _)| dir).to_string();
    match mode {
        Mode::Speech => "fala".into(),
        Mode::Flow => dir(),
        Mode::Lines => path.to_string(),
    }
}

/// Mensagens de `files` (caminho, posicao no cartucho, bytes guardados) que
/// sao BMG, e nomes das tabelas de registro fixo dos outros arquivos.
/// `font` = largura de cada letra (`font_widths`).
pub(crate) fn extract(
    files: &[(String, usize, &[u8])],
    font: Option<&HashMap<u8, u8>>,
) -> Vec<TextEntry> {
    let width = |b: u8| {
        font.and_then(|f| f.get(&b))
            .copied()
            .unwrap_or(DEFAULT_WIDTH) as usize
    };
    let mut opened = Vec::new();
    let mut tables = Vec::new();
    for (path, at, file) in files {
        match open(file).and_then(|(bmg, _)| Some((parse(&bmg)?, bmg))) {
            Some(((_, messages), bmg)) => opened.push((path.as_str(), *at, bmg, messages)),
            None => tables.extend(names(path, *at, file)),
        }
    }
    // Largura de cada grupo, e o espaco que cada codigo teve no mais apertado.
    let mut widest: HashMap<String, usize> = HashMap::new();
    let mut most_lines: HashMap<&str, usize> = HashMap::new();
    let mut most_chars: HashMap<&str, usize> = HashMap::new();
    let mut lines_of = Vec::new();
    for (path, _, bmg, messages) in &opened {
        let g = group(path, mode(path));
        for m in messages {
            let mut lines: Vec<(usize, Vec<String>)> = vec![(0, Vec::new())];
            for bit in bits(&bmg[m.clone()]).unwrap_or_default() {
                match bit {
                    Bit::Text(t) => {
                        lines.last_mut().unwrap().0 += t.iter().map(|&b| width(b)).sum::<usize>()
                    }
                    Bit::Code(c) => lines.last_mut().unwrap().1.push(hex(c)),
                    Bit::Line => lines.push((0, Vec::new())),
                }
            }
            let max = most_lines.entry(path).or_default();
            *max = (*max).max(lines.len());
            let mut chars = vec![0usize];
            for bit in bits(&bmg[m.clone()]).unwrap_or_default() {
                match bit {
                    Bit::Text(t) => *chars.last_mut().unwrap() += t.len(),
                    Bit::Line => chars.push(0),
                    Bit::Code(_) => {}
                }
            }
            let max = most_chars.entry(path).or_default();
            *max = (*max).max(chars.into_iter().max().unwrap_or(0));
            for (px, _) in &lines {
                let w = widest.entry(g.clone()).or_default();
                *w = (*w).max(*px);
            }
            lines_of.push((g.clone(), lines));
        }
    }
    // So o tipo 04 escreve na tela (nome, numero, item). Pausa (01), expressao
    // (07), cor (ff) e o 0b que vem antes do nome nao ocupam nada: medido no
    // jogo de 25/10, com espaco pra todo codigo a pausa ficava com a caixa
    // inteira (ela so aparece em linha curta) e a reticencia nao cabia.
    let mut room: HashMap<(String, String), usize> = HashMap::new();
    for (g, lines) in lines_of {
        for (px, codes) in lines {
            for code in codes.into_iter().filter(|c| c.starts_with(WRITES)) {
                let r = room.entry((g.clone(), code)).or_insert(usize::MAX);
                *r = (*r).min(widest[&g].saturating_sub(px));
            }
        }
    }
    // Letras que fogem da largura mais comum, agrupadas por largura.
    let mut count: HashMap<usize, usize> = HashMap::new();
    (0x20..=0xFF).for_each(|b| *count.entry(width(b)).or_default() += 1);
    let common = count
        .iter()
        .max_by_key(|&(w, n)| (n, std::cmp::Reverse(*w)))
        .map_or(0, |(&w, _)| w);
    let mut px: BTreeMap<String, String> = BTreeMap::new();
    for b in 0x20..=0xFFu8 {
        if width(b) != common {
            px.entry(width(b).to_string()).or_default().push(decode(b));
        }
    }

    let mut out = Vec::new();
    for (path, at, bmg, messages) in &opened {
        let (mode, g) = (mode(path), group(path, mode(path)));
        for (k, m) in messages.iter().enumerate() {
            let raw = &bmg[m.clone()];
            let Some((before, text, after)) = frame(raw, mode) else {
                continue;
            };
            if text.is_empty() {
                continue;
            }
            let codes: serde_json::Map<String, serde_json::Value> = tokens(&text)
                .iter()
                .filter(|(code, _)| *code)
                .filter_map(|(_, piece)| {
                    let key = &piece[1..piece.len() - 1];
                    let r = *room.get(&(g.clone(), key.to_string()))?;
                    (r > 0).then(|| (key.to_string(), r.into()))
                })
                .collect();
            let meta = serde_json::json!({
                "indice": k,
                "antes": hex(&before),
                "depois": hex(&after),
                "modo": match mode { Mode::Speech => "fala", Mode::Flow => "corrido", Mode::Lines => "linhas" },
                "largura": widest[&g],
                "linhas": if mode == Mode::Flow { most_lines[path] } else { 0 },
                "letras": if mode == Mode::Lines { most_chars[path] } else { 0 },
                "padrao": common,
                "px": px,
                "nomes": codes,
            });
            out.push(TextEntry {
                id: format!("bmg-{path}-{k}"),
                resource_path: Some(path.to_string()),
                offset: Some(*at as u64),
                original_bytes: raw.to_vec(),
                source_text: text,
                translated_text: None,
                context: None,
                max_bytes: None,
                encoding: TextEncoding::Utf8,
                status: Default::default(),
                metadata: serde_json::json!({ "bmg": meta }),
            });
        }
    }
    out.extend(tables);
    out
}

// --- Tabela de nomes de registro fixo -------------------------------------

/// Letra de nome: ASCII imprimivel ou acentuada. O 0xFF ("ÿ") fica de fora:
/// e o byte de cabecalho de registro mais comum.
fn letter(b: u8) -> bool {
    (0x20..0x7F).contains(&b) || (0xA0..0xFF).contains(&b)
}

/// Modelo, textura e animacao 3D do DS: o nome de cada parte e procurado
/// pelo jogo, nao lido por ninguem.
const MODELS: [&[u8]; 7] = [
    b"BMD0", b"BTX0", b"BCA0", b"BTA0", b"BTP0", b"BMA0", b"BVA0",
];

/// (passo do registro, inicio do campo no registro) de um arquivo que e
/// tabela de nomes: 20+ nomes com cara de palavra (90%), quase todos (90%)
/// na mesma posicao de registros do mesmo
/// tamanho, cada um no fim do registro, com zeros ate o fim dele. O passo e
/// o maior que alinha os nomes (o dobro do certo alinha so metade).
// ponytail: medido em 3 tabelas do jogo de 25/10 (registros de 20 e 28
// bytes, nome no fim); tabela com o nome no meio do registro fica de fora.
fn name_table(file: &[u8]) -> Option<(usize, usize)> {
    if MODELS.iter().any(|magic| file.starts_with(magic)) {
        return None;
    }
    let mut runs = Vec::new();
    let mut at = 0;
    while at < file.len() {
        let len = file[at..].iter().take_while(|&&b| letter(b)).count();
        if len >= 4 && (at == 0 || !letter(file[at - 1])) && file.get(at + len) == Some(&0) {
            runs.push(at..at + len);
        }
        at += len.max(1);
    }
    let word = |r: &Range<usize>| {
        let t = &file[r.clone()];
        t[0].is_ascii_alphabetic()
            && t.iter().any(u8::is_ascii_lowercase)
            && t.iter()
                .all(|&b| b.is_ascii_alphanumeric() || b" '-.,&!?".contains(&b) || b >= 0xA0)
    };
    let words = runs.iter().filter(|r| word(r)).count();
    if runs.len() < 20 || words * 10 < runs.len() * 9 {
        return None;
    }
    let (stride, start) = (8..=64).rev().find_map(|s| {
        let mut count: HashMap<usize, usize> = HashMap::new();
        runs.iter()
            .for_each(|r| *count.entry(r.start % s).or_default() += 1);
        let (&c, &n) = count.iter().max_by_key(|&(_, n)| *n)?;
        (n * 10 >= runs.len() * 9).then_some((s, c))
    })?;
    let tail_zero = runs
        .iter()
        .filter(|r| r.start % stride == start)
        .filter(|r| {
            let end = (r.start / stride + 1) * stride;
            file.get(r.end..end)
                .is_some_and(|rest| rest.iter().all(|&b| b == 0))
        })
        .count();
    (tail_zero * 10 >= runs.len() * 9).then_some((stride, start))
}

/// Os nomes de um arquivo que e tabela de registro fixo: um por registro com
/// o campo preenchido (nome e zeros).
fn names(path: &str, at: usize, file: &[u8]) -> Vec<TextEntry> {
    let Some((stride, start)) = name_table(file) else {
        return Vec::new();
    };
    let field = stride - start;
    (0..file.len() / stride)
        .filter_map(|k| {
            let slot = &file[k * stride + start..(k + 1) * stride];
            let len = slot.iter().position(|&b| b == 0)?;
            (len > 0
                && slot[..len].iter().all(|&b| letter(b))
                && slot[len..].iter().all(|&b| b == 0))
            .then(|| TextEntry {
                id: format!("nome-{path}-{k}"),
                resource_path: Some(path.to_string()),
                offset: Some((at + k * stride + start) as u64),
                original_bytes: slot.to_vec(),
                source_text: slot[..len].iter().map(|&b| decode(b)).collect(),
                translated_text: None,
                context: None,
                max_bytes: None,
                encoding: TextEncoding::Utf8,
                status: Default::default(),
                metadata: serde_json::json!({ "bmg": { "campo": field } }),
            })
        })
        .collect()
}

// --- Regravacao -----------------------------------------------------------

/// Arquivo remontado: (caminho, bytes novos, ja guardados como vieram).
pub(crate) type Rebuilt = (String, Vec<u8>);

/// Os arquivos com as traducoes: BMG remontado e guardado como veio, tabela
/// de nomes com o nome no campo. `file` da o (inicio, tamanho) do arquivo no
/// cartucho ORIGINAL.
pub(crate) fn plan(
    data: &[u8],
    entries: &[TextEntry],
    file: impl Fn(&str) -> Option<(usize, usize)>,
) -> Result<(Vec<Rebuilt>, ApplyReport)> {
    let mut groups: BTreeMap<&str, Vec<&TextEntry>> = BTreeMap::new();
    for entry in entries.iter().filter(|e| is_bmg(e)) {
        groups
            .entry(entry.resource_path.as_deref().unwrap_or_default())
            .or_default()
            .push(entry);
    }
    let mut report = ApplyReport::default();
    let mut out = Vec::new();
    for (path, group) in groups {
        let translated = group.iter().filter(|e| e.translated_text.is_some()).count();
        report.kept_original += group.len() - translated;
        if translated == 0 {
            continue;
        }
        let changed = || {
            CoreError::Project(format!(
                "{path}: o arquivo nao e mais o que foi extraido — re-extraia antes de reinserir"
            ))
        };
        let bad = |entry: &TextEntry, c: char| {
            CoreError::Project(format!(
                "entry {}: a letra {c:?} nao existe na fonte do jogo",
                entry.id
            ))
        };
        let (start, len) = file(path).ok_or_else(changed)?;
        let bytes = &data[start..start + len];
        let rebuilt = match open(bytes) {
            Some((bmg, pack)) => {
                let (layout, messages) = parse(&bmg).ok_or_else(changed)?;
                let mut texts = BTreeMap::new();
                for entry in &group {
                    let meta = &entry.metadata["bmg"];
                    let k = meta["indice"].as_u64().unwrap_or(u64::MAX) as usize;
                    let m = messages.get(k).ok_or_else(changed)?;
                    if bmg[m.clone()] != entry.original_bytes[..] {
                        return Err(changed());
                    }
                    if let Some(text) = &entry.translated_text {
                        let side = |key: &str| {
                            unhex(meta[key].as_str().unwrap_or_default()).unwrap_or_default()
                        };
                        let body = unframe(&wrapped(entry, text).unwrap_or_else(|| text.clone()))
                            .map_err(|c| bad(entry, c))?;
                        texts.insert(k, [side("antes"), body, side("depois")].concat());
                    }
                }
                close(&rebuild(&bmg, &layout, &messages, &texts), pack).ok_or_else(|| {
                    CoreError::Project(format!(
                        "{path}: o arquivo traduzido passa do que o formato guarda"
                    ))
                })?
            }
            None => {
                let mut copy = bytes.to_vec();
                for entry in group.iter().filter(|e| e.translated_text.is_some()) {
                    let field = max_chars(entry).ok_or_else(changed)? + 1;
                    let at = (entry.offset.unwrap_or(0) as usize)
                        .checked_sub(start)
                        .ok_or_else(changed)?;
                    let slot = copy.get_mut(at..at + field).ok_or_else(changed)?;
                    if *slot != entry.original_bytes[..] {
                        return Err(changed());
                    }
                    let name = unframe(entry.translated_text.as_deref().unwrap_or_default())
                        .map_err(|c| bad(entry, c))?;
                    if name.len() >= field {
                        return Err(CoreError::Project(format!(
                            "entry {}: o nome passa das {} letras do campo",
                            entry.id,
                            field - 1
                        )));
                    }
                    slot.fill(0);
                    slot[..name.len()].copy_from_slice(&name);
                }
                copy
            }
        };
        report.applied += translated;
        out.push((path.to_string(), rebuilt));
    }
    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BMG de uma letra por byte com `messages` (bytes crus).
    fn bmg(messages: &[&[u8]]) -> Vec<u8> {
        let mut inf = Vec::new();
        let mut data = vec![0u8];
        for m in messages {
            let at = if m.is_empty() { 0 } else { data.len() };
            inf.extend_from_slice(&(at as u32).to_le_bytes());
            inf.extend_from_slice(&[0xFF, 0, 0, 0]);
            if !m.is_empty() {
                data.extend_from_slice(m);
                data.push(0);
            }
        }
        let mut inf1 = b"INF1\0\0\0\0".to_vec();
        inf1.extend_from_slice(&(messages.len() as u16).to_le_bytes());
        inf1.extend_from_slice(&8u16.to_le_bytes());
        inf1.extend_from_slice(&[0; 4]);
        inf1.extend_from_slice(&inf);
        inf1.resize(inf1.len().next_multiple_of(32), 0);
        let n = inf1.len() as u32;
        inf1[4..8].copy_from_slice(&n.to_le_bytes());
        let mut dat1 = b"DAT1\0\0\0\0".to_vec();
        dat1.extend_from_slice(&data);
        dat1.resize(dat1.len().next_multiple_of(32), 0);
        let n = dat1.len() as u32;
        dat1[4..8].copy_from_slice(&n.to_le_bytes());
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&((32 + inf1.len() + dat1.len()) as u32).to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.push(1);
        out.resize(32, 0);
        [out, inf1, dat1].concat()
    }

    fn entry(meta: serde_json::Value) -> TextEntry {
        TextEntry {
            id: "e".into(),
            resource_path: None,
            offset: None,
            original_bytes: Vec::new(),
            source_text: String::new(),
            translated_text: None,
            context: None,
            max_bytes: None,
            encoding: TextEncoding::Utf8,
            status: Default::default(),
            metadata: serde_json::json!({ "bmg": meta }),
        }
    }

    #[test]
    fn containers_come_back_the_way_they_were() {
        let file = bmg(&[b"Hello there!", b"", "Ol\u{e1}".as_bytes()]);
        let long: Vec<u8> = file.iter().cycle().take(9000).copied().collect();
        for (bytes, pack) in [
            (file.clone(), Pack::Raw),
            (file.clone(), Pack::Lz),
            (long.clone(), Pack::Chunks(0xF7)),
        ] {
            let packed = close(&bytes, pack).unwrap();
            // O arquivo longo nao e BMG valido (tamanho do cabecalho), so os blocos.
            if bytes.len() == file.len() {
                assert_eq!(open(&packed), Some((bytes.clone(), pack)));
            }
        }
        // Blocos: 3 de 4 KiB, cada um LZ77; um bloco cru (00) tambem le.
        let packed = close(&long, Pack::Chunks(0xF7)).unwrap();
        assert_eq!(
            &packed[..8],
            &[b'L', b'Z', b'7', b'7', 0xF7, 0x28, 0x23, 0x00]
        );
        let mut real = file.clone();
        real.resize(CHUNK + 40, 0);
        let total = real.len() as u32;
        real[8..12].copy_from_slice(&total.to_le_bytes());
        let first = lz77::pack(&real[..CHUNK]).unwrap();
        let mut raw = vec![0, 0, 0x10, 0];
        raw.extend_from_slice(&real[CHUNK..]);
        let mut chunks = b"LZ77\xF7".to_vec();
        chunks.extend_from_slice(&total.to_le_bytes()[..3]);
        chunks.extend_from_slice(&(first.len() as u16).to_le_bytes());
        chunks.extend_from_slice(&((first.len() + raw.len()) as u16).to_le_bytes());
        chunks.extend_from_slice(&first);
        chunks.extend_from_slice(&raw);
        assert_eq!(open(&chunks).map(|(b, _)| b), Some(real));
        // Lixo e corte nunca panicam.
        for cut in 0..packed.len().min(64) {
            let _ = open(&packed[..cut]);
        }
        assert!(open(b"LZ77").is_none() && open(&[]).is_none() && parse(&file[..40]).is_none());
    }

    #[test]
    fn speech_goes_flowing_with_pages_and_comes_back_in_lines() {
        // Pagina 1 cheia (3 linhas), pagina 2 com linha em branco, escolha.
        let raw = b"\x1a\x05\x07\x28\x00Flag on the\nmoon\x1a\x07\x01\x00\x00\x05\x00. Numm numm\nnumm. How did\n\x1a\x05\x07\x00\x00It get there?\n\nOK?\x1a\x08\x02\x01\x00\x0d\x0e\x0f\nYes\nNo\x1a\x05\x09\x09\x00";
        let (before, text, after) = frame(raw, Mode::Speech).unwrap();
        assert_eq!(before, b"\x1a\x05\x07\x28\x00");
        assert_eq!(after, b"\x1a\x05\x09\x09\x00");
        assert_eq!(
            text,
            "Flag on the moon[0100000500]. Numm numm numm. How did [070000]It get there?[p]OK?[0201000d0e0f][n]Yes[n]No"
        );
        let e = entry(
            serde_json::json!({ "modo": "fala", "largura": 60, "padrao": 5, "px": { "3": " .i" } }),
        );
        let lines = Box::of(&e).unwrap().layout(&text);
        // A pagina 2 tem 2 linhas e uma em branco: "OK?" abre a terceira.
        assert_eq!(
            lines,
            [
                "Flag on the",
                "moon[0100000500]. Numm",
                "numm numm.",
                "How did [070000]It",
                "get there?",
                "",
                "OK?[0201000d0e0f]",
                "Yes",
                "No"
            ]
        );
        let text = "Um dois[p]Tres";
        assert_eq!(
            Box::of(&e).unwrap().layout(text),
            ["Um dois", "", "", "Tres"]
        );
        let body = unframe(&wrapped(&e, text).unwrap()).unwrap();
        assert_eq!(body, b"Um dois\n\n\nTres");
    }

    #[test]
    fn a_name_the_game_writes_takes_its_room_and_lists_keep_their_lines() {
        let e = entry(serde_json::json!({
            "modo": "corrido", "largura": 50, "linhas": 2, "padrao": 5, "px": {}, "nomes": { "041f00": 40 }
        }));
        assert_eq!(
            Box::of(&e).unwrap().layout("Oi, [041f00] tudo"),
            ["Oi,", "[041f00]", "tudo"]
        );
        let found = problems(&e, "Oi, [041f00] tudo");
        assert!(found
            .iter()
            .any(|(error, why)| *error && why.contains("3 linhas")));
        let list = entry(
            serde_json::json!({ "modo": "linhas", "largura": 20, "letras": 5, "padrao": 5, "px": {} }),
        );
        assert_eq!(wrapped(&list, "Sim[n]Deixa").unwrap(), "Sim[n]Deixa");
        let errors = problems(&list, "Sim[n]Deixa pra la");
        assert!(errors.iter().any(|(error, _)| *error));
        assert_eq!(max_line(&list), Some(5));
        // Bordao do save: nao passa das letras da linha mais longa do original.
        assert!(problems(&list, "Sim[n]Deixar")
            .iter()
            .any(|(error, why)| *error && why.contains("6 letras")));
        assert!(problems(&list, "Sim, \u{263a}")
            .iter()
            .any(|(_, why)| why.contains("fonte")));
    }

    #[test]
    fn codes_and_letters_come_back_byte_for_byte() {
        let raw = [
            b"Caf\xe9 \x93ok\x94".as_slice(),
            &[ESC, 6, 0xFF, 0, 0, 6],
            b"x[y]",
        ]
        .concat();
        let (before, text, after) = frame(&raw, Mode::Lines).unwrap();
        assert_eq!(text, "Caf\u{e9} \u{201c}ok\u{201d}[ff000006]x[y]");
        assert!(before.is_empty() && after.is_empty());
        assert_eq!([before, unframe(&text).unwrap(), after].concat(), raw);
        assert_eq!(unframe("a\u{263a}"), Err('\u{263a}'));
    }

    #[test]
    fn a_grown_message_moves_the_others_and_sizes_follow() {
        let file = bmg(&[b"Hi.", b"", b"Bye."]);
        let (layout, messages) = parse(&file).unwrap();
        let texts = BTreeMap::from([(
            0,
            "Ol\u{e1}, tudo bem por a\u{ed}?"
                .chars()
                .map(|c| encode(c).unwrap())
                .collect(),
        )]);
        let out = rebuild(&file, &layout, &messages, &texts);
        let (_, again) = parse(&out).unwrap();
        assert_eq!(&out[again[0].clone()], texts[&0].as_slice());
        assert!(again[1].is_empty());
        assert_eq!(&out[again[2].clone()], b"Bye.");
        assert_eq!(u32_at(&out, 8), Some(out.len()));
        assert_eq!(out.len() % 32, 0);
    }

    #[test]
    fn a_fixed_record_table_gives_one_name_per_record() {
        let names_list = ["butterfly paper", "exotic bed", "koi", "blue bench"];
        let mut file = Vec::new();
        for k in 0..40 {
            let mut record = vec![0xEC, 0x09, 0x0A, 0x1E, 0x00, 0x64, 0, 0, 0xFF, 0x02, 0x01];
            let mut field = names_list[k % 4].as_bytes().to_vec();
            field.resize(17, 0);
            record.extend_from_slice(&field);
            file.extend_from_slice(&record);
        }
        assert_eq!(name_table(&file), Some((28, 11)));
        let entries = names("ftr/dma.bin", 0x1000, &file);
        assert_eq!(entries.len(), 40);
        assert_eq!(entries[2].source_text, "koi");
        assert_eq!(entries[2].offset, Some(0x1000 + 2 * 28 + 11));
        assert_eq!(max_chars(&entries[0]), Some(16));
        // Modelo 3D (nomes sem cara de palavra) nao e tabela.
        let model: Vec<u8> = (0..40)
            .flat_map(|k| format!("polygon_{k:02}\0\0\0\0\0\0\0").into_bytes())
            .collect();
        assert_eq!(name_table(&model), None);
        let mut tx = entries[2].clone();
        tx.translated_text = Some("carpa-koi".into());
        let rom = [vec![0; 0x1000], file.clone()].concat();
        let (out, report) = plan(&rom, &[tx.clone()], |_| Some((0x1000, file.len()))).unwrap();
        tx.translated_text = Some("carpa koi do lago real".into());
        assert!(plan(&rom, &[tx], |_| Some((0x1000, file.len()))).is_err());
        assert_eq!(report.applied, 1);
        assert_eq!(&out[0].1[2 * 28 + 11..2 * 28 + 20], b"carpa-koi");
        assert_eq!(out[0].1[2 * 28 + 20], 0);
    }

    #[test]
    fn font_attributes_give_the_width_of_each_letter() {
        let mut attr = Vec::new();
        for code in 0x20u16..0x100 {
            let w: u16 = if code == b'i' as u16 { 3 } else { 5 };
            attr.extend_from_slice(&code.to_le_bytes());
            attr.extend_from_slice(&w.to_le_bytes());
        }
        let packed = [b"LZ77".as_slice(), &lz77::pack(&attr).unwrap()].concat();
        let widths = font_widths(&packed).unwrap();
        assert_eq!((widths[&b'i'], widths[&b'm']), (3, 5));
        assert!(font_widths(&attr[..40]).is_none());
    }
}

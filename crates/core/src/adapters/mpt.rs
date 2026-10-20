//! Arquivo de mensagens MPT0, o da serie Dragon Quest de DS (medido no V em
//! 19/10/2026, so contagens: 157 arquivos de texto por idioma, 29.248
//! mensagens em ingles).
//!
//! - Cabecalho de 8 u32: "MPT0", tamanho do arquivo, dois campos do jogo (o
//!   numero do arquivo, que fica como esta), quantidade N, posicao do indice,
//!   tamanho do indice (multiplo de 4) e tamanho dos dados.
//! - Indice: N entradas de 6 bytes — id u16, tamanho u16 em bytes e posicao
//!   u16 em unidades de 4 bytes, contada do fim do indice.
//! - Mensagem: UTF-8, com o resto ate o multiplo de 4 preenchido com 0xFE.
//!   O mesmo cabecalho guarda os icones do jogo, que nao sao UTF-8: ficam de
//!   fora.
//!
//! Os codigos do jogo vao em colchetes pra quem traduz: `@aNome@b` (quem
//! fala), `@c0@` (fim da fala), `@0N@2the @3a @4stick@5...@8N@` (nome com os
//! artigos e o plural), `%a000090` (o nome do heroi),
//! `%H0860%Xa visitor%Yvisitors%Z` (singular ou plural), `\r\n` (`[rn]`) e
//! `\n` (`[n]`). Os codigos das pontas saem da frase e voltam na regravacao.
//! A mensagem pode crescer a vontade: o arquivo e remontado e, se nao couber
//! no lugar dele, vai pro fim do cartucho (`nds::replace_files`).
//!
//! Cada idioma do cartucho tem a sua pasta (`EN/`, `FR/`...): so a do idioma
//! de origem e traduzida. As outras dao o teto de cada trecho dos nomes de
//! menu — o maior que o jogo mostra ali em qualquer idioma. A fala (fim
//! `@c`) o jogo quebra sozinho na caixa: sem teto.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use crate::adapter::ApplyReport;
use crate::error::{CoreError, Result};
use crate::types::{TextEncoding, TextEntry};

const MAGIC: &[u8] = b"MPT0";
const HEADER: usize = 32;
const PAD: u8 = 0xFE;
/// A posicao no indice e u16 em unidades de 4 bytes.
const MAX_DATA: usize = 0xFFFF * 4;

pub(crate) struct Message {
    pub body: Range<usize>,
}

fn u32_at(file: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(file.get(at..at + 4)?.try_into().ok()?) as usize)
}

fn u16_at(file: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(file.get(at..at + 2)?.try_into().ok()?) as usize)
}

/// As mensagens de um MPT0 de texto, na ordem do indice. `None` = nao e
/// MPT0, os campos nao fecham, ou alguma mensagem nao e UTF-8.
pub(crate) fn parse(file: &[u8]) -> Option<Vec<Message>> {
    if file.get(..4)? != MAGIC {
        return None;
    }
    let total = u32_at(file, 4)?;
    let count = u32_at(file, 16)?;
    let index = u32_at(file, 20)?;
    let index_len = u32_at(file, 24)?;
    let base = index.checked_add(index_len)?;
    if total > file.len()
        || index < HEADER
        || base.checked_add(u32_at(file, 28)?)? != total
        || count.checked_mul(6)? > index_len
    {
        return None;
    }
    (0..count)
        .map(|i| {
            let at = index + 6 * i;
            let start = base + 4 * u16_at(file, at + 4)?;
            let body = start..start + u16_at(file, at + 2)?;
            std::str::from_utf8(file.get(body.clone())?).ok()?;
            Some(Message { body })
        })
        .collect()
}

/// Tamanho do codigo do jogo no comeco de `s`, se ha um.
fn code_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let digits = |from: usize| {
        b.get(from..).map_or(0, |rest| {
            rest.iter().take_while(|c| c.is_ascii_digit()).count()
        })
    };
    match b {
        [b'\r', b'\n', ..] => Some(2),
        [b'\r' | b'\n', ..] => Some(1),
        [b'@', b'c', ..] if digits(2) > 0 && b.get(2 + digits(2)) == Some(&b'@') => {
            Some(3 + digits(2))
        }
        [b'@', b'0' | b'8' | b'9', flag, ..] if flag.is_ascii_uppercase() => Some(3),
        [b'@', c, ..] if c.is_ascii_alphanumeric() => Some(2),
        [b'@', ..] => Some(1),
        [b'%', c, ..] if c.is_ascii_digit() => Some(2),
        [b'%', c, ..] if c.is_ascii_alphabetic() => Some(2 + digits(2)),
        _ => None,
    }
}

/// O codigo como vai pra quem traduz: em colchetes, e a quebra de linha com
/// nome.
fn bracket(code: &str) -> String {
    match code {
        "\r\n" => "[rn]".to_string(),
        "\n" => "[n]".to_string(),
        "\r" => "[r]".to_string(),
        _ => format!("[{code}]"),
    }
}

/// Os pedacos de uma mensagem crua: (e codigo?, trecho).
fn bits(raw: &str) -> Vec<(bool, &str)> {
    let mut out: Vec<(bool, &str)> = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        let rest = &raw[at..];
        let (code, len) = match code_len(rest) {
            Some(len) => (true, len),
            None => (false, rest.chars().next().map_or(1, char::len_utf8)),
        };
        match out.last_mut() {
            Some((false, text)) if !code => *text = &raw[at - text.len()..at + len],
            _ => out.push((code, &rest[..len])),
        }
        at += len;
    }
    out
}

/// (antes, frase, depois): a frase com os codigos em colchetes, e os codigos
/// crus das duas pontas, que nao vao pra quem traduz.
pub(crate) fn frame(raw: &str) -> (String, String, String) {
    let bits = bits(raw);
    let lead = bits.iter().take_while(|(code, _)| *code).count();
    let tail = bits[lead..]
        .iter()
        .rev()
        .take_while(|(code, _)| *code)
        .count();
    let join = |bits: &[(bool, &str)]| bits.iter().map(|(_, s)| *s).collect::<String>();
    let text: String = bits[lead..bits.len() - tail]
        .iter()
        .map(|&(code, s)| if code { bracket(s) } else { s.to_string() })
        .collect();
    // O espaco da ponta e do jogo (" equips [%a020100]!" vem depois de um
    // nome): fica com os codigos, que a IA nao ve.
    let start = text.len() - text.trim_start_matches(' ').len();
    let end = text.trim_end_matches(' ').len().max(start);
    (
        join(&bits[..lead]) + &text[..start],
        text[start..end].to_string(),
        text[end..].to_string() + &join(&bits[bits.len() - tail..]),
    )
}

/// O caminho de volta: colchete com codigo do jogo dentro vira o codigo; o
/// resto fica como esta.
pub(crate) fn unframe(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let inside = after.find(']').map(|close| &after[..close]);
        let code = match inside {
            Some("rn") => Some("\r\n"),
            Some("n") => Some("\n"),
            Some("r") => Some("\r"),
            Some(code) if !code.starts_with(['\r', '\n']) && code_len(code) == Some(code.len()) => {
                Some(code)
            }
            _ => None,
        };
        match (code, inside) {
            (Some(code), Some(inside)) => {
                out.push_str(code);
                rest = &after[inside.len() + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A pasta de idioma no caminho (`data/MESS5/EN/b0.mpt` -> `EN`): um nome de
/// duas letras maiusculas.
fn language(path: &str) -> Option<&str> {
    path.split('/')
        .find(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_uppercase()))
}

/// O caminho com a pasta de idioma trocada por `*`: a chave que junta os
/// idiomas de um mesmo arquivo.
fn any_language(path: &str) -> String {
    path.split('/')
        .map(|part| match Some(part) == language(path) {
            true => "*",
            false => part,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// As frases dos arquivos MPT0 de `files` (caminho, posicao no cartucho,
/// bytes): so as da pasta do idioma `wanted` (`EN`), ou de todas se nenhuma
/// pasta e dele. Mensagem so de codigos fica de fora.
pub(crate) fn extract(files: &[(String, usize, &[u8])], wanted: &str) -> Vec<TextEntry> {
    let parsed: Vec<(&str, usize, &[u8], Vec<Message>)> = files
        .iter()
        .filter_map(|(path, at, file)| Some((path.as_str(), *at, *file, parse(file)?)))
        .collect();
    let ours = |path: &str| language(path).is_none_or(|lang| lang == wanted);
    let only_ours = parsed
        .iter()
        .any(|(path, ..)| language(path) == Some(wanted));

    // Teto de cada mensagem de menu: o trecho mais longo entre dois codigos,
    // no idioma em que ele e maior (japones fora: letra de largura dupla).
    let mut widest: HashMap<(String, usize), usize> = HashMap::new();
    for (path, _, file, messages) in &parsed {
        for (i, m) in messages.iter().enumerate() {
            let raw = std::str::from_utf8(&file[m.body.clone()]).unwrap_or_default();
            if crate::pipeline::has_japanese(raw) {
                continue;
            }
            let line = crate::adapters::bank::longest_line(&frame(raw).1);
            let max = widest.entry((any_language(path), i)).or_default();
            *max = (*max).max(line);
        }
    }

    let mut out = Vec::new();
    for (path, at, file, messages) in &parsed {
        if only_ours && !ours(path) {
            continue;
        }
        let siblings = language(path).is_some();
        for (i, m) in messages.iter().enumerate() {
            let raw = std::str::from_utf8(&file[m.body.clone()]).unwrap_or_default();
            let (before, text, after) = frame(raw);
            if text.is_empty() {
                continue;
            }
            let mut meta = serde_json::json!({ "indice": i, "antes": before, "depois": after });
            let line = widest.get(&(any_language(path), i));
            if let (true, false, Some(&line)) = (siblings, raw.contains("@c"), line) {
                meta["linha"] = line.into();
            }
            out.push(TextEntry {
                id: format!("mpt-{path}-{i}"),
                resource_path: Some(path.to_string()),
                offset: Some((at + m.body.start) as u64),
                original_bytes: file[m.body.clone()].to_vec(),
                source_text: text,
                translated_text: None,
                context: None,
                max_bytes: None,
                encoding: TextEncoding::Utf8,
                status: Default::default(),
                metadata: serde_json::json!({ "mpt": meta }),
            });
        }
    }
    out
}

/// A entry e uma mensagem de arquivo MPT0.
pub(crate) fn is_mpt(entry: &TextEntry) -> bool {
    entry.metadata.get("mpt").is_some_and(|v| v.is_object())
}

/// O arquivo remontado com `texts` (posicao no indice -> texto cru) no lugar
/// das mensagens; as outras ficam como estao.
fn rebuild(file: &[u8], messages: &[Message], texts: &BTreeMap<usize, String>) -> Option<Vec<u8>> {
    let index = u32_at(file, 20)?;
    let base = index + u32_at(file, 24)?;
    let mut out = file[..base].to_vec();
    // Mensagens com a mesma posicao continuam dividindo o texto.
    let mut placed: HashMap<usize, usize> = HashMap::new();
    for (i, m) in messages.iter().enumerate() {
        let text = texts
            .get(&i)
            .map_or(&file[m.body.clone()], |text| text.as_bytes());
        let original = !texts.contains_key(&i);
        let at = match placed.get(&m.body.start) {
            Some(&at) if original => at,
            _ => {
                let at = out.len() - base;
                out.extend_from_slice(text);
                out.resize(out.len().next_multiple_of(4), PAD);
                if original {
                    placed.insert(m.body.start, at);
                }
                at
            }
        };
        if at > MAX_DATA || text.len() > 0xFFFF {
            return None;
        }
        let entry = index + 6 * i;
        out[entry + 2..entry + 4].copy_from_slice(&(text.len() as u16).to_le_bytes());
        out[entry + 4..entry + 6].copy_from_slice(&((at / 4) as u16).to_le_bytes());
    }
    let (total, data_len) = (out.len() as u32, (out.len() - base) as u32);
    out[4..8].copy_from_slice(&total.to_le_bytes());
    out[28..32].copy_from_slice(&data_len.to_le_bytes());
    Some(out)
}

/// Arquivo remontado: (caminho, bytes novos).
pub(crate) type Rebuilt = (String, Vec<u8>);

/// Os arquivos MPT0 remontados com as traducoes.
/// `file` da o (inicio, tamanho) do arquivo no cartucho ORIGINAL.
pub(crate) fn plan(
    data: &[u8],
    entries: &[TextEntry],
    file: impl Fn(&str) -> Option<(usize, usize)>,
) -> Result<(Vec<Rebuilt>, ApplyReport)> {
    let mut groups: BTreeMap<&str, Vec<&TextEntry>> = BTreeMap::new();
    for entry in entries.iter().filter(|e| is_mpt(e)) {
        let path = entry.resource_path.as_deref().unwrap_or_default();
        groups.entry(path).or_default().push(entry);
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
                "{path}: o arquivo de mensagens nao e mais o que foi extraido — re-extraia antes \
                 de reinserir"
            ))
        };
        let (start, len) = file(path).ok_or_else(changed)?;
        let bytes = &data[start..start + len];
        let messages = parse(bytes).ok_or_else(changed)?;
        let mut texts = BTreeMap::new();
        for entry in group {
            let meta = &entry.metadata["mpt"];
            let i = meta["indice"].as_u64().unwrap_or(u64::MAX) as usize;
            let m = messages.get(i).ok_or_else(changed)?;
            if bytes[m.body.clone()] != entry.original_bytes[..] {
                return Err(changed());
            }
            if let Some(text) = &entry.translated_text {
                let side = |key: &str| meta[key].as_str().unwrap_or_default().to_string();
                texts.insert(i, side("antes") + &unframe(text) + &side("depois"));
            }
        }
        let rebuilt = rebuild(bytes, &messages, &texts).ok_or_else(|| {
            CoreError::Project(format!(
                "{path}: as mensagens traduzidas passam do que o indice do arquivo alcanca \
                 ({} KiB de texto) — encurte as traducoes deste arquivo",
                MAX_DATA / 1024
            ))
        })?;
        report.applied += translated;
        out.push((path.to_string(), rebuilt));
    }
    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::synth::mpt_file as file;

    #[test]
    fn codes_go_in_brackets_and_come_back_byte_for_byte() {
        let raw = "@aPankraz@b%0%a000090... I see %H0860%Xa visitor%Yvisitors%Z!\r\nHmph.\n@c0@";
        let (before, text, after) = frame(raw);
        assert_eq!(before, "@a");
        assert_eq!(after, "\n@c0@");
        assert_eq!(
            text,
            "Pankraz[@b][%0][%a000090]... I see [%H0860][%X]a visitor[%Y]visitors[%Z]![rn]Hmph."
        );
        assert_eq!(before + &unframe(&text) + &after, raw);

        let (before, text, after) = frame("@0N@2the @3a @4cypress ;stick@5the @8N@");
        assert_eq!((before.as_str(), after.as_str()), ("@0N@2", " @8N@"));
        assert_eq!(text, "the [@3]a [@4]cypress ;stick[@5]the");
        let (before, text, _) = frame("@a@b equips %a020100!\n@c0@");
        assert_eq!(
            (before.as_str(), text.as_str()),
            ("@a@b ", "equips [%a020100]!")
        );
        assert_eq!(frame("@a@b  @c0@").1, "");
        // Colchete que nao e codigo do jogo fica como texto; 100% nao e codigo.
        assert_eq!(unframe("[sic] [@5]100% [n]"), "[sic] @5100% \n");
        assert_eq!(frame("@").1, "");
    }

    #[test]
    fn parse_rejects_what_is_not_a_text_file() {
        let good = file(&["@aBob@bHi.\n@c0@", "@"]);
        assert_eq!(parse(&good).unwrap().len(), 2);
        assert!(parse(&good[..good.len() - 1]).is_none());
        let mut icon = good.clone();
        let at = parse(&good).unwrap()[0].body.start;
        icon[at] = 0xFF; // nao e UTF-8: icone
        assert!(parse(&icon).is_none());
        assert!(parse(b"MPT0").is_none());
        assert!(parse(&[]).is_none());
    }

    #[test]
    fn a_grown_message_moves_the_ones_after_it() {
        let original = file(&["@aBob@bHi.\n@c0@", "@4stick@", "@aAl@bYo.\n@c0@"]);
        let messages = parse(&original).unwrap();
        let texts = BTreeMap::from([(0, "@aBob@bOlá, tudo bem por aí?\n@c0@".to_string())]);
        let rebuilt = rebuild(&original, &messages, &texts).unwrap();
        let again = parse(&rebuilt).unwrap();
        let text = |i: usize| std::str::from_utf8(&rebuilt[again[i].body.clone()]).unwrap();
        assert_eq!(text(0), "@aBob@bOlá, tudo bem por aí?\n@c0@");
        assert_eq!((text(1), text(2)), ("@4stick@", "@aAl@bYo.\n@c0@"));
        assert_eq!(u32_at(&rebuilt, 4), Some(rebuilt.len()));
        assert_eq!(u32_at(&rebuilt, 8), Some(7002));
        assert!(again.iter().all(|m| m.body.start % 4 == 0));
    }

    #[test]
    fn only_the_source_language_is_extracted_and_menus_get_the_widest_line() {
        let en = file(&["@aBob@bHi.\n@c0@", "@4stick@"]);
        let fr = file(&["@aBob@bSalut.\n@c0@", "@4baton de ;cypres@"]);
        let ja = file(&["@aBob@bこんにちは。\n@c0@", "@4ひのきのぼうひのきのぼう@"]);
        let files = vec![
            ("data/M/EN/b0.mpt".to_string(), 0x1000, en.as_slice()),
            ("data/M/FR/b0.mpt".to_string(), 0x2000, fr.as_slice()),
            ("data/M/JA/b0.mpt".to_string(), 0x3000, ja.as_slice()),
        ];
        let entries = extract(&files, "EN");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source_text, "Bob[@b]Hi.");
        assert!(entries[0].metadata["mpt"].get("linha").is_none());
        assert_eq!(entries[1].source_text, "stick");
        assert_eq!(entries[1].metadata["mpt"]["linha"], 16);
        assert_eq!(entries[1].id, "mpt-data/M/EN/b0.mpt-1");
        // Sem a pasta do idioma, vem tudo.
        assert_eq!(extract(&files, "PT").len(), 6);
    }
}

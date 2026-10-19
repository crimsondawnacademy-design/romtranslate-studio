//! Perfil do jogo (`<projeto>/perfil.toml`): o texto que quem estudou o
//! arquivo — uma pessoa ou uma IA — descreveu, pra o app ler e regravar sem
//! adivinhar. Cobre o formato classico que nenhuma regra automatica pega
//! sozinha: frases num banco, cada uma com cabecalho e terminador, letras
//! numa tabela propria (`tables/<id>.tbl`), achadas por ponteiros de 32 bits
//! que guardam o endereco de memoria da frase.
//!
//! **Extracao.** Frase = cabecalho alinhado dentro da faixa, lida pela tabela
//! ate o terminador. Vale a que tem ponteiro (palavra alinhada, numa das
//! faixas de busca, com o endereco exato do cabecalho) e cara de texto
//! (`scan::looks_like_text` sem os codigos). Byte que a tabela nao tem vira
//! codigo `[XX]`, com os bytes de parametro declarados (`[E3:10]`), e a linha
//! dele entra na tabela, numa secao gerada.
//!
//! **Gravacao.** As frases achadas sao remontadas dentro do espaco que elas
//! mesmas ocupavam: o arquivo nao cresce (modulo que o jogo carrega na
//! memoria tem outro dado logo depois — armadilha 7.1 do handoff), frase
//! repetida e gravada uma vez so e todo ponteiro e reapontado. Cada frase tem
//! um teto de bytes (`max_bytes`) dado na extracao, pra a soma caber: o espaco
//! dela com o alinhamento e, nas curtas (nomes), a sobra que as repetidas
//! liberam. O que mesmo assim nao couber fica como no original e e contado em
//! `ApplyReport::no_room`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::adapter::{ApplyReport, Extraction};
use crate::error::{CoreError, Result};
use crate::scan::{looks_like_identifier, looks_like_text};
use crate::tbl::{TblTable, TABLES_DIR};
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

/// Nome do arquivo do perfil, na pasta do projeto.
pub const FILE: &str = "perfil.toml";
/// Maior frase lida (cabecalho e terminador fora): passou disso sem achar o
/// terminador, nao e frase.
const MAX_MESSAGE: usize = 4096;
/// Frase curta (nome de item, de inimigo, de menu): ganha a sobra que as
/// frases repetidas liberam, de 4 em 4 bytes.
const SHORT_CHARS: usize = 16;
/// Marca da secao da tabela que a extracao escreve sozinha.
const GENERATED: &str = "# --- codigos achados pelo perfil (gerado; nao edite abaixo) ---";

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub texto: Vec<Source>,
}

/// Um banco de frases do jogo.
#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    /// Vai no `resource_path` das frases ("overlay 0", "arm9"...).
    pub nome: String,
    /// Id da tabela: `tables/<tabela>.tbl` no projeto.
    pub tabela: String,
    /// Faixa do arquivo onde as frases moram (posicoes no arquivo do jogo).
    pub inicio: u64,
    pub fim: u64,
    /// Bytes que abrem cada frase, em hexadecimal ("0100"). Pode ser vazio.
    #[serde(default)]
    pub cabecalho: String,
    /// Bytes que fecham cada frase ("EA", "00").
    pub terminador: String,
    /// Alinhamento do comeco de cada frase, em bytes.
    #[serde(default = "one")]
    pub alinhamento: usize,
    /// Codigo -> quantos bytes de parametro vem depois dele ({ E3 = 1 }).
    #[serde(default)]
    pub parametros: BTreeMap<String, usize>,
    /// Codigo que quebra a linha ("E6"): com ele a frase ganha a largura da
    /// linha medida no jogo, e a gravacao quebra a linha que passar dela.
    #[serde(default)]
    pub quebra: Option<String>,
    /// Codigo que fecha uma pagina da caixa de texto ("E5", a espera do
    /// botao): cada pagina da traducao cabe nas linhas da mesma pagina do
    /// original.
    #[serde(default)]
    pub pagina: Option<String>,
    pub ponteiros: Pointers,
}

/// Ponteiro de 32 bits, little-endian: valor = `memoria` + (posicao no
/// arquivo - `origem`). Num modulo de DS, `memoria` e o endereco onde ele e
/// carregado e `origem` a posicao dele no cartucho.
#[derive(Debug, Clone, Deserialize)]
pub struct Pointers {
    pub memoria: u32,
    pub origem: u64,
    /// Faixas do arquivo onde procurar os ponteiros, [inicio, fim).
    pub procurar: Vec<[u64; 2]>,
}

fn one() -> usize {
    1
}

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("perfil: {}", msg.into()))
}

fn hex(text: &str, what: &str) -> Result<Vec<u8>> {
    let digits: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !digits.len().is_multiple_of(2) {
        return Err(err(format!(
            "{what} \"{text}\" nao e hexadecimal aos pares"
        )));
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&digits[i..i + 2], 16)
                .map_err(|_| err(format!("{what} \"{text}\" nao e hexadecimal")))
        })
        .collect()
}

/// O perfil do projeto, se ele tem um.
pub fn load(project_dir: &Path) -> Result<Option<Profile>> {
    let path = project_dir.join(FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CoreError::io(&path, e)),
    };
    let profile: Profile =
        toml::from_str(&text).map_err(|e| err(format!("{FILE} invalido: {e}")))?;
    for source in &profile.texto {
        Rules::of(source)?;
    }
    Ok(Some(profile))
}

/// A entry veio de um perfil.
pub fn is_profile(entry: &TextEntry) -> bool {
    entry.metadata.get("perfil").is_some_and(|v| v.is_object())
}

/// O que o perfil diz, ja lido.
struct Rules {
    header: Vec<u8>,
    end: Vec<u8>,
    align: usize,
    params: HashMap<u8, usize>,
    newline: Option<String>,
    page: Option<String>,
}

impl Rules {
    fn of(source: &Source) -> Result<Self> {
        let end = hex(&source.terminador, "terminador")?;
        if end.is_empty() {
            return Err(err(format!("{}: terminador vazio", source.nome)));
        }
        if source.alinhamento == 0 || source.inicio >= source.fim {
            return Err(err(format!(
                "{}: alinhamento zero ou faixa vazia",
                source.nome
            )));
        }
        let mut params = HashMap::new();
        for (code, n) in &source.parametros {
            let byte = hex(code, "codigo")?;
            let [byte] = byte[..] else {
                return Err(err(format!("codigo \"{code}\" tem que ser de 1 byte")));
            };
            params.insert(byte, *n);
        }
        let newline = match &source.quebra {
            Some(code) => {
                let byte = hex(code, "quebra")?;
                let [byte] = byte[..] else {
                    return Err(err(format!("quebra \"{code}\" tem que ser de 1 byte")));
                };
                Some(tag(&[byte]))
            }
            None => None,
        };
        let page = match &source.pagina {
            Some(code) => {
                let byte = hex(code, "pagina")?;
                let [byte] = byte[..] else {
                    return Err(err(format!("pagina \"{code}\" tem que ser de 1 byte")));
                };
                Some(tag(&[byte]))
            }
            None => None,
        };
        Ok(Rules {
            page,
            header: hex(&source.cabecalho, "cabecalho")?,
            end,
            align: source.alinhamento,
            params,
            newline,
        })
    }

    /// Bytes que a frase ocupa no arquivo, com o alinhamento.
    fn padded(&self, body: usize) -> usize {
        (self.header.len() + body + self.end.len()).next_multiple_of(self.align)
    }
}

/// Codigo do jogo como texto: `[E6]`, `[E3:10]`, `[E1:04B3]`.
fn tag(bytes: &[u8]) -> String {
    let code = format!("{:02X}", bytes[0]);
    if bytes.len() == 1 {
        return format!("[{code}]");
    }
    let params: String = bytes[1..].iter().map(|b| format!("{b:02X}")).collect();
    format!("[{code}:{params}]")
}

/// Uma frase lida: onde o corpo comeca e acaba, o texto e os codigos que a
/// tabela nao tinha.
struct Read {
    body: std::ops::Range<usize>,
    text: String,
    codes: Vec<Vec<u8>>,
}

fn read(data: &[u8], at: usize, limit: usize, table: &TblTable, rules: &Rules) -> Option<Read> {
    if !data.get(at..)?.starts_with(&rules.header) {
        return None;
    }
    let start = at + rules.header.len();
    let mut i = start;
    let mut text = String::new();
    let mut codes = Vec::new();
    loop {
        if i >= limit || i - start > MAX_MESSAGE {
            return None;
        }
        if data[i..].starts_with(&rules.end) {
            break;
        }
        if let Some((n, piece)) = table.lookup(&data[i..limit]) {
            text.push_str(piece);
            i += n;
            continue;
        }
        let n = 1 + rules.params.get(&data[i]).copied().unwrap_or(0);
        let code = data.get(i..i + n).filter(|_| i + n <= limit)?;
        text.push_str(&tag(code));
        codes.push(code.to_vec());
        i += n;
    }
    (i > start).then_some(Read {
        body: start..i,
        text,
        codes,
    })
}

/// O texto sem os codigos (cada um vira `sep`): o que aparece na tela.
fn visible(text: &str, sep: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        match rest[open..].find(']') {
            Some(close) => {
                out.push_str(sep);
                rest = &rest[open + close + 1..];
            }
            None => {
                rest = &rest[open..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Tamanho de cada linha da frase, cortada no codigo de quebra.
fn lines<'a>(text: &'a str, newline: &str) -> impl Iterator<Item = usize> + 'a {
    let newline = newline.to_string();
    text.split(newline.as_str())
        // A mesma conta da validacao: o trecho entre dois codigos, espaco
        // do fim incluso.
        .map(crate::adapters::bank::longest_line)
        .collect::<Vec<_>>()
        .into_iter()
}

/// Extracao de todas as fontes do perfil.
pub fn extract(project_dir: &Path, data: &[u8], profile: &Profile) -> Result<Extraction> {
    let mut out = Extraction::default();
    for (index, source) in profile.texto.iter().enumerate() {
        let rules = Rules::of(source)?;
        let table_path = project_dir
            .join(TABLES_DIR)
            .join(format!("{}.tbl", source.tabela));
        // So as linhas da pessoa: a secao gerada e refeita no fim, e uma linha
        // velha dela (`E1=[E1]`) passaria na frente da regra de parametro.
        let written = fs::read_to_string(&table_path).map_err(|e| CoreError::io(&table_path, e))?;
        let table = TblTable::parse(written.split(GENERATED).next().unwrap_or_default())?;
        let start = (source.inicio as usize).min(data.len());
        let end = (source.fim as usize).min(data.len());

        // 1. Frases: cabecalho alinhado, lido ate o terminador.
        //    Todo comeco alinhado e candidato, inclusive dentro de outro: um
        //    dado que abre com o cabecalho pode ir longe ate achar um
        //    terminador, e as frases de dentro dele nao podem sumir junto.
        let mut found: BTreeMap<usize, Read> = BTreeMap::new();
        let mut at = start.next_multiple_of(rules.align);
        while at < end {
            if let Some(message) = read(data, at, end, &table, &rules) {
                found.insert(at, message);
            }
            at += rules.align;
        }

        // 2. Ponteiros: palavra alinhada com o endereco exato do cabecalho.
        let mut pointers: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for [from, to] in &source.ponteiros.procurar {
            let to = (*to as usize).min(data.len());
            let mut k = (*from as usize).next_multiple_of(4);
            while k + 4 <= to {
                let value = u32::from_le_bytes([data[k], data[k + 1], data[k + 2], data[k + 3]]);
                let target = (value as i64) - (source.ponteiros.memoria as i64)
                    + (source.ponteiros.origem as i64);
                if let Some(target) = usize::try_from(target)
                    .ok()
                    .filter(|t| found.contains_key(t))
                {
                    pointers.entry(target).or_default().push(k);
                }
                k += 4;
            }
        }

        // 3. Frases de verdade: com ponteiro e cara de texto.
        //    Duas que se sobrepoem: fica a primeira.
        let mut codes: HashSet<Vec<u8>> = HashSet::new();
        let mut kept: Vec<(usize, Read, Vec<usize>)> = Vec::new();
        for (at, message) in found {
            let Some(refs) = pointers.remove(&at) else {
                continue;
            };
            let plain = visible(&message.text, " ");
            let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
            let overlaps = kept
                .last()
                .is_some_and(|(_, last, _)| at < last.body.end + rules.end.len());
            if overlaps || !looks_like_text(&plain) {
                out.discarded += 1;
                continue;
            }
            codes.extend(message.codes.iter().cloned());
            kept.push((at, message, refs));
        }

        // 4. Teto de cada frase: o espaco dela com o alinhamento e, nas
        //    curtas, a sobra das repetidas (gravadas uma vez so), de 4 em 4.
        let mut budgets: Vec<usize> = kept
            .iter()
            .map(|(_, m, _)| rules.padded(m.body.len()) - rules.header.len() - rules.end.len())
            .collect();
        let mut seen: HashSet<&[u8]> = HashSet::new();
        let mut spare = 0;
        for (_, m, _) in &kept {
            if !seen.insert(&data[m.body.clone()]) {
                spare += rules.padded(m.body.len());
            }
        }
        // A sobra vai por frase unica: as copias de uma repetida tem a mesma
        // traducao (a memoria de traducao garante) e sao gravadas uma vez.
        let newline = rules.newline.clone();
        let multiline = |m: &Read| newline.as_deref().is_some_and(|nl| m.text.contains(nl));
        let mut groups: BTreeMap<(usize, &str), Vec<usize>> = BTreeMap::new();
        for (i, (_, m, _)) in kept.iter().enumerate() {
            if !multiline(m) && m.text.chars().count() <= SHORT_CHARS {
                groups
                    .entry((m.text.chars().count(), m.text.as_str()))
                    .or_default()
                    .push(i);
            }
        }
        'share: while spare >= 4 {
            let before = spare;
            for copies in groups.values() {
                if spare < 4 {
                    break 'share;
                }
                for &i in copies {
                    budgets[i] += 4;
                }
                spare -= 4;
            }
            if spare == before {
                break;
            }
        }

        // 5. Largura da linha: a maior linha da propria frase ou a que 95%
        //    das linhas do banco respeitam, a que for maior.
        let width = newline.as_deref().and_then(|nl| {
            let mut all: Vec<usize> = kept
                .iter()
                .filter(|(_, m, _)| m.text.contains(nl))
                .flat_map(|(_, m, _)| lines(&m.text, nl))
                .filter(|&n| n > 0)
                .collect();
            all.sort_unstable();
            all.get(all.len() * 95 / 100).copied()
        });

        for ((at, message, refs), budget) in kept.into_iter().zip(budgets) {
            let line = match (&newline, width) {
                (Some(nl), Some(width)) if message.text.contains(nl.as_str()) => {
                    Some(lines(&message.text, nl).max().unwrap_or(0).max(width))
                }
                _ => None,
            };
            let status = if looks_like_identifier(&message.text) {
                TranslationStatus::Internal
            } else {
                TranslationStatus::Untranslated
            };
            let mut perfil = serde_json::json!({
                "fonte": index,
                "inicio": at,
                "ponteiros": refs,
            });
            if let (Some(line), Some(nl)) = (line, &newline) {
                perfil["linha"] = serde_json::json!(line);
                perfil["quebra"] = serde_json::json!(nl);
                if let Some(page) = &rules.page {
                    perfil["pagina"] = serde_json::json!(page);
                }
            }
            out.entries.push(TextEntry {
                id: format!("perfil-{at:08x}"),
                resource_path: Some(source.nome.clone()),
                offset: Some(message.body.start as u64),
                original_bytes: data[message.body.clone()].to_vec(),
                source_text: message.text,
                translated_text: None,
                context: Some(format!(
                    "frase do perfil do jogo ({}): pode mudar de lugar no banco, ate {budget} bytes",
                    source.nome
                )),
                max_bytes: Some(budget),
                encoding: TextEncoding::Table(source.tabela.clone()),
                status,
                metadata: serde_json::json!({ "perfil": perfil }),
            });
        }
        write_codes(&table_path, &codes)?;
    }
    Ok(out)
}

/// Poe na tabela a linha de cada codigo achado (`E310=[E3:10]`), numa secao
/// que a extracao refaz: a validacao e a gravacao leem a frase pela tabela.
fn write_codes(path: &Path, codes: &HashSet<Vec<u8>>) -> Result<()> {
    let old = fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
    let mine = match old.find(GENERATED) {
        Some(at) => &old[..at],
        None => old.as_str(),
    };
    let mut lines: Vec<String> = codes
        .iter()
        .map(|code| {
            let key: String = code.iter().map(|b| format!("{b:02X}")).collect();
            format!("{key}={}", tag(code))
        })
        .collect();
    lines.sort();
    let mut out = mine.trim_end().to_string();
    out.push('\n');
    if !lines.is_empty() {
        out.push_str(GENERATED);
        out.push('\n');
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
    }
    if out != old {
        fs::write(path, out).map_err(|e| CoreError::io(path, e))?;
    }
    Ok(())
}

/// Quebra a linha que passa de `width` no ultimo espaco antes dela.
fn wrap(text: &str, newline: &str, width: usize) -> String {
    text.split(newline)
        .map(|line| {
            let mut out = String::new();
            let mut current = 0;
            for (i, word) in line.split(' ').enumerate() {
                let size = visible(word, "").trim_end().chars().count();
                if i > 0 {
                    if current + 1 + size > width && current > 0 {
                        out.push_str(newline);
                        current = 0;
                    } else {
                        out.push(' ');
                        current += 1;
                    }
                }
                out.push_str(word);
                current += size;
            }
            out
        })
        .collect::<Vec<_>>()
        .join(newline)
}

/// A traducao da frase do perfil com as linhas que passam da caixa quebradas
/// no codigo do jogo — o que a gravacao escreve. `None` = frase sem quebra
/// de linha declarada.
pub fn wrapped(entry: &TextEntry, text: &str) -> Option<String> {
    let perfil = entry.metadata.get("perfil")?;
    let newline = perfil.get("quebra")?.as_str()?;
    let width = perfil.get("linha")?.as_u64()? as usize;
    Some(wrap(text, newline, width))
}

/// O codigo de quebra de linha da frase do perfil (`[E6]`), se ela tem um.
pub fn newline(entry: &TextEntry) -> Option<&str> {
    entry.metadata.get("perfil")?.get("quebra")?.as_str()
}

/// Linhas com texto de cada pagina (o trecho ate o codigo de pagina).
fn page_lines(text: &str, newline: &str, page: Option<&str>) -> Vec<usize> {
    let pages: Vec<&str> = match page {
        Some(page) => text.split(page).collect(),
        None => vec![text],
    };
    pages
        .iter()
        .map(|page| {
            page.split(newline)
                .filter(|line| !visible(line, "").trim().is_empty())
                .count()
        })
        .collect()
}

/// A traducao, depois de quebrada pelo app, tem numa pagina mais linhas que
/// a mesma pagina do original: `Some((pagina, linhas, cabem))`, a primeira.
/// A caixa de texto do jogo tem a altura dela; o que passa some ou vaza.
pub fn page_overflow(entry: &TextEntry, translation: &str) -> Option<(usize, usize, usize)> {
    let perfil = entry.metadata.get("perfil")?;
    let nl = perfil.get("quebra")?.as_str()?;
    let page = perfil.get("pagina").and_then(|p| p.as_str());
    let text = wrapped(entry, translation)?;
    let original = page_lines(&entry.source_text, nl, page);
    page_lines(&text, nl, page)
        .into_iter()
        .zip(original)
        .enumerate()
        .find(|(_, (lines, room))| lines > room)
        .map(|(i, (lines, room))| (i + 1, lines, room))
}

/// Uma frase pronta pra gravar.
struct Placed<'a> {
    entry: &'a TextEntry,
    at: usize,
    pointers: Vec<usize>,
    old: Vec<u8>,
    new: Vec<u8>,
}

/// Trechos [inicio, fim) do arquivo.
type Spans = Vec<(usize, usize)>;

/// Onde cada frase vai, na ordem de `placed` (que esta pela posicao). A que
/// cabe no lugar dela fica onde esta — so a copia de uma repetida sai,
/// apontando pra primeira; a que cresceu vai pra sobra (o resto do lugar das
/// que encolheram e o lugar das que sairam), a maior primeiro, no menor
/// buraco em que cabe. Mudar de lugar so o que precisa protege o texto que o
/// jogo acha sem passar pelo ponteiro. `None` = alguma nao coube; devolve
/// tambem o espaco que ficou livre, pra ser zerado.
fn pack(placed: &[Placed], align: usize) -> Option<(Vec<usize>, Spans)> {
    let pad = |n: usize| n.next_multiple_of(align);
    let mut target = vec![0; placed.len()];
    let mut first: HashMap<&[u8], usize> = HashMap::new();
    let mut free: Spans = Vec::new();
    let mut movers = Vec::new();
    for (i, p) in placed.iter().enumerate() {
        let slot = pad(p.old.len());
        if let Some(&at) = first.get(&p.new[..]) {
            target[i] = at;
            free.push((p.at, p.at + slot));
        } else if pad(p.new.len()) <= slot {
            target[i] = p.at;
            first.insert(&p.new, p.at);
            free.push((p.at + pad(p.new.len()), p.at + slot));
        } else {
            free.push((p.at, p.at + slot));
            movers.push(i);
        }
    }
    free.retain(|(a, b)| a < b);
    free.sort_unstable();
    let mut merged: Spans = Vec::new();
    for (a, b) in free {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let zeroed = merged.clone();
    movers.sort_by_key(|&i| std::cmp::Reverse(placed[i].new.len()));
    for i in movers {
        if let Some(&at) = first.get(&placed[i].new[..]) {
            target[i] = at;
            continue;
        }
        let size = pad(placed[i].new.len());
        let (best, _) = merged
            .iter()
            .enumerate()
            .filter(|(_, (a, b))| b - a >= size)
            .min_by_key(|(_, (a, b))| b - a)?;
        target[i] = merged[best].0;
        merged[best].0 += size;
        first.insert(&placed[i].new, target[i]);
    }
    Some((target, zeroed))
}

fn perfil_field(entry: &TextEntry, key: &str) -> Option<u64> {
    entry.metadata.get("perfil")?.get(key)?.as_u64()
}

/// Regrava as frases das fontes do perfil em `out` (a imagem original, que
/// vira a traduzida). `entries` sao as do perfil, traduzidas ou nao.
pub fn apply(
    project_dir: &Path,
    profile: &Profile,
    out: &mut [u8],
    entries: &[TextEntry],
) -> Result<ApplyReport> {
    let mut report = ApplyReport::default();
    for (index, source) in profile.texto.iter().enumerate() {
        let rules = Rules::of(source)?;
        let table = TblTable::load(
            &project_dir
                .join(TABLES_DIR)
                .join(format!("{}.tbl", source.tabela)),
        )?;
        let mine: Vec<&TextEntry> = entries
            .iter()
            .filter(|e| perfil_field(e, "fonte") == Some(index as u64))
            .collect();
        if mine.iter().all(|e| e.translated_text.is_none()) {
            report.kept_original += mine.len();
            continue;
        }
        let address = |at: usize| -> Result<u32> {
            let value =
                at as i64 - source.ponteiros.origem as i64 + source.ponteiros.memoria as i64;
            u32::try_from(value).map_err(|_| err(format!("endereco de 0x{at:X} fora de 32 bits")))
        };

        // Confere que a imagem e a da extracao e monta os bytes de cada frase.
        let mut placed = Vec::new();
        for entry in mine {
            let at = perfil_field(entry, "inicio")
                .ok_or_else(|| err(format!("entry {} sem o inicio da frase", entry.id)))?
                as usize;
            let mut old = rules.header.clone();
            old.extend(&entry.original_bytes);
            old.extend(&rules.end);
            if out.get(at..at + old.len()) != Some(&old[..]) {
                return Err(err(format!(
                    "a frase {} nao e mais a da extracao — arquivo diferente? Re-extraia antes de \
                     reinserir",
                    entry.id
                )));
            }
            let pointers: Vec<usize> = entry
                .metadata
                .get("perfil")
                .and_then(|p| p.get("ponteiros"))
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_u64().map(|v| v as usize))
                .collect();
            let expected = address(at)?.to_le_bytes();
            if pointers.is_empty()
                || pointers
                    .iter()
                    .any(|&p| out.get(p..p + 4) != Some(&expected[..]))
            {
                return Err(err(format!(
                    "um ponteiro da frase {} nao aponta mais pra ela — Re-extraia antes de reinserir",
                    entry.id
                )));
            }
            let new = match &entry.translated_text {
                Some(text) => {
                    let text = wrapped(entry, text).unwrap_or_else(|| text.clone());
                    let encoded = table
                        .encode(&text)
                        .map_err(|e| err(format!("entry {}: {e}", entry.id)))?;
                    let fits = entry.max_bytes.is_none_or(|max| encoded.len() <= max);
                    if fits {
                        let mut new = rules.header.clone();
                        new.extend(encoded);
                        new.extend(&rules.end);
                        Some(new)
                    } else {
                        report.no_room += 1;
                        None
                    }
                }
                None => None,
            };
            if new.is_some() {
                report.applied += 1;
            } else if entry.translated_text.is_none() {
                report.kept_original += 1;
            }
            placed.push(Placed {
                entry,
                at,
                pointers,
                new: new.unwrap_or_else(|| old.clone()),
                old,
            });
        }

        // Espaco: o que as frases ocupavam, com o alinhamento, emendado.
        placed.sort_by_key(|p| p.at);
        let mut room: Vec<(usize, usize)> = Vec::new();
        for p in &placed {
            let span = (p.at, p.at + p.old.len().next_multiple_of(rules.align));
            match room.last_mut() {
                Some(last) if span.0 <= last.1 => last.1 = last.1.max(span.1),
                _ => room.push(span),
            }
        }
        let inside = |at: usize| room.iter().any(|&(a, b)| (a..b).contains(&at));
        if let Some(p) = placed
            .iter()
            .flat_map(|p| p.pointers.iter())
            .find(|&&k| (0..4).any(|d| inside(k + d)))
        {
            return Err(err(format!(
                "o ponteiro em 0x{p:X} fica dentro do banco de frases — faixa do perfil errada?"
            )));
        }

        // Remonta. Se nao couber (o banco pode ter buracos de outro dado no
        // meio), a traducao que mais cresceu volta ao original e tenta de
        // novo; so com originais tudo fica onde estava.
        let (target, zeroed) = loop {
            match pack(&placed, rules.align) {
                Some(plan) => break plan,
                None => {
                    let worst = placed
                        .iter_mut()
                        .filter(|p| p.new != p.old)
                        .max_by_key(|p| p.new.len() as i64 - p.old.len() as i64);
                    let Some(worst) = worst else {
                        return Err(err(format!(
                            "as frases originais de \"{}\" nao cabem de volta no banco (bug)",
                            source.nome
                        )));
                    };
                    worst.new = worst.old.clone();
                    report.applied -= 1;
                    report.no_room += 1;
                }
            }
        };
        for &(a, b) in &zeroed {
            out[a..b].fill(0);
        }
        for (p, &at) in placed.iter().zip(&target) {
            out[at..at + p.new.len().next_multiple_of(rules.align)].fill(0);
            out[at..at + p.new.len()].copy_from_slice(&p.new);
            if at != p.at {
                let value = address(at)?.to_le_bytes();
                for &k in &p.pointers {
                    out[k..k + 4].copy_from_slice(&value);
                }
                if p.entry.translated_text.is_some() {
                    report.relocated += 1;
                }
            }
        }

        // Auto-checagem: cada ponteiro leva aos bytes novos da frase.
        for p in &placed {
            for &k in &p.pointers {
                let value = u32::from_le_bytes([out[k], out[k + 1], out[k + 2], out[k + 3]]);
                let at = (value as i64 - source.ponteiros.memoria as i64
                    + source.ponteiros.origem as i64) as usize;
                if out.get(at..at + p.new.len()) != Some(&p.new[..]) {
                    return Err(err(format!(
                        "auto-checagem falhou na frase {} (bug) — nada foi gravado",
                        p.entry.id
                    )));
                }
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_breaks_only_the_line_that_is_too_long() {
        let nl = "[E6]";
        assert_eq!(
            wrap("Bom dia, Soma.[E6]Tudo bem?", nl, 20),
            "Bom dia, Soma.[E6]Tudo bem?"
        );
        assert_eq!(
            wrap("Voce chegou cedo demais hoje[E3:10]", nl, 12),
            "Voce chegou[E6]cedo demais[E6]hoje[E3:10]"
        );
    }

    #[test]
    fn codes_with_parameters_become_one_tag() {
        assert_eq!(tag(&[0xE6]), "[E6]");
        assert_eq!(tag(&[0xE3, 0x10]), "[E3:10]");
        assert_eq!(tag(&[0xE1, 0x04, 0xB3]), "[E1:04B3]");
        assert_eq!(visible("Oi[E3:10] Soma[E6]", " "), "Oi  Soma ");
        assert_eq!(visible("[E3:10]Soma[E6]", ""), "Soma");
    }
}

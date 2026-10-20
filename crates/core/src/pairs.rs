//! Pares de letras na fonte do jogo (DTE de fonte): o codigo que o texto
//! traduzido deixa livre ganha 2 letras ("de", "a ", "ão"), e cada codigo de
//! 2 bytes passa a mostrar ate 2 letras. E a tecnica das traducoes de
//! Tokimeki Memorial e Shadowrun pra caber o portugues no espaco do japones.
//!
//! O projeto guarda a lista dos pares que quer, o mais usado primeiro, em
//! `tables/jis16.pares.json` (`make`, pelo texto ja traduzido). A extracao
//! poe cada par que cabe nas fontes do jogo num codigo livre
//! (`images::letters::choose_pairs`), a tabela grava e le com eles, e a
//! reinsercao os desenha (`images::letters::draw`).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adapters::bank::{self, Layout, Piece};
use crate::db::ProjectDb;
use crate::error::{CoreError, Result};
use crate::tbl::{is_pair, TblTable, PAIR_RATE};
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

/// A lista de pares do projeto, em `tables/`.
pub const FILE: &str = "jis16.pares.json";
/// Par que aparece menos que isto no texto traduzido nao vale um codigo.
const MIN_USES: usize = 20;
/// No maximo isto de pares: cada um troca um kanji da fonte.
const MAX_PAIRS: usize = 640;

#[derive(Debug, Default, Serialize, Deserialize)]
struct List {
    /// O mais usado primeiro.
    pares: Vec<String>,
}

fn file(project_dir: &Path) -> PathBuf {
    project_dir.join(crate::tbl::TABLES_DIR).join(FILE)
}

/// Os pares que o projeto quer, o mais usado primeiro. Sem a lista, nenhum.
pub fn wanted(project_dir: &Path) -> Result<Vec<String>> {
    let path = file(project_dir);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let list: List = serde_json::from_str(&text)
                .map_err(|e| CoreError::Project(format!("{}: {e}", path.display())))?;
            Ok(list.pares.into_iter().filter(|p| is_pair(p)).collect())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CoreError::io(&path, e)),
    }
}

/// Codigo -> par de letras que `table` grava (chave de 2 bytes, o menos
/// significativo primeiro): os que a copia de trabalho tem desenhados.
pub fn drawn(table: &TblTable) -> HashMap<u16, String> {
    table
        .entries()
        .filter(|(_, text)| is_pair(text))
        .filter_map(|(key, text)| {
            Some((u16::from_le_bytes(key.try_into().ok()?), text.to_string()))
        })
        .collect()
}

/// `drawn` da tabela do japones de 16 bits do projeto, se ela existe.
pub fn drawn_in(project_dir: &Path) -> Result<HashMap<u16, String>> {
    let path = project_dir
        .join(crate::tbl::TABLES_DIR)
        .join(format!("{}.tbl", crate::kuten::TABLE_ID));
    if !path.is_file() {
        return Ok(HashMap::new());
    }
    Ok(drawn(&TblTable::load(&path)?))
}

/// Os trechos de texto de `text`, sem os codigos em colchetes: par nao
/// atravessa codigo.
fn runs(text: &str) -> impl Iterator<Item = &str> {
    bank::pieces(text)
        .into_iter()
        .filter_map(|piece| match piece {
            Piece::Text(text) => Some(text),
            Piece::Code(_) => None,
        })
}

/// Cada par de caracteres seguidos de `texts` e quantas vezes aparece, o
/// mais usado primeiro (no empate, pela ordem do texto do par). Maiuscula
/// conta a parte ("De" e "de"), e o espaco entra de um lado ("a ", " o"),
/// nunca dos dois.
pub fn ranked<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for text in texts {
        for run in runs(text) {
            let chars: Vec<char> = run.chars().collect();
            for pair in chars.windows(2) {
                if pair
                    .iter()
                    .any(|c| c.is_control() || matches!(c, '[' | ']'))
                    || pair == [' ', ' ']
                {
                    continue;
                }
                *counts.entry(pair.iter().collect()).or_default() += 1;
            }
        }
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|(a, x), (b, y)| y.cmp(x).then_with(|| a.cmp(b)));
    ranked
}

/// (letras, codigos) do texto de `texts` gravado com `table`, sem os codigos
/// em colchetes.
fn rate(table: &TblTable, texts: &[&str]) -> (usize, usize) {
    texts
        .iter()
        .flat_map(|text| runs(text))
        .filter_map(|run| Some((run.chars().count(), table.cells(run)?)))
        .fold((0, 0), |(l, c), (letters, cells)| (l + letters, c + cells))
}

/// `table_text` com a linha `PAIR_RATE` na frente, medida em `texts` (o
/// texto ja traduzido do projeto), se a tabela tem pares de letras: e por ela
/// que o pedido a IA conta quantas letras cabem num espaco.
pub(crate) fn with_rate(table_text: &str, texts: &[&str]) -> String {
    let Ok(table) = TblTable::parse(table_text) else {
        return table_text.to_string();
    };
    let (letters, codes) = rate(&table, texts);
    if !table.has_pairs() || codes == 0 {
        return table_text.to_string();
    }
    format!(
        "{PAIR_RATE}{letters} letras em {codes} codigos (texto traduzido do projeto)\n{table_text}"
    )
}

/// As traducoes de `entries` gravadas com a tabela `id`, das frases que vao
/// pro jogo.
pub(crate) fn translations<'a>(entries: &'a [TextEntry], id: &str) -> Vec<&'a str> {
    let encoding = TextEncoding::Table(id.to_string());
    entries
        .iter()
        .filter(|e| e.encoding == encoding && e.status != TranslationStatus::Internal)
        .filter_map(|e| e.translated_text.as_deref())
        .collect()
}

/// O texto traduzido do projeto gravado com uma tabela: o que ocupa e o que
/// cabe.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measure {
    /// Bytes de todas as traducoes gravadas.
    pub bytes: usize,
    /// Mensagens de banco quebradas pelo app (`bank::Layout`): linhas de
    /// todas, e quantas ficam com mais linhas que o japones (mais paginas da
    /// caixa pra ler).
    pub lines: usize,
    pub longer_than_original: usize,
    /// Frases de espaco fixo (nome de item, inimigo...): quantas ocupam o
    /// espaco inteiro (a traducao que foi apertada pra caber), e o teto em
    /// letras que o pedido a IA dava a todas, somado.
    pub full_slots: usize,
    pub slot_letters: usize,
    /// Traducoes que a tabela nao grava ou que nao cabem no espaco fixo.
    pub not_fitting: usize,
}

/// Mede `entries` (com traducao) gravadas com `table`.
fn measure(table: &TblTable, entries: &[&TextEntry]) -> Measure {
    let mut m = Measure::default();
    for entry in entries {
        let Some(text) = entry.translated_text.as_deref() else {
            continue;
        };
        let layout = Layout::of(entry);
        let written = match &layout {
            Some(layout) => layout.wrap(text, Some(table)).0,
            None => text.to_string(),
        };
        let Ok(bytes) = table.encode(&written) else {
            m.not_fitting += 1;
            continue;
        };
        m.bytes += bytes.len();
        if let Some(layout) = &layout {
            let lines = |text: &str| text.matches(layout.soft()).count() + 1;
            m.lines += lines(&written);
            m.longer_than_original += usize::from(lines(&written) > lines(&entry.source_text));
        } else if !entry.can_grow() {
            let slot = entry.original_bytes.len();
            let kept = crate::validate::extract_tokens(&entry.source_text);
            m.slot_letters += table.chars_that_fit_keeping(slot, &kept);
            m.full_slots += usize::from(bytes.len() == slot);
            m.not_fitting += usize::from(bytes.len() > slot);
        }
    }
    m
}

/// O que `make` fez.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// Pares na lista (usados `MIN_USES` vezes ou mais), e quantos a
    /// extracao conseguiu por na tabela (cabem nas fontes, codigo livre).
    pub wanted: usize,
    pub in_table: usize,
    /// Os 20 primeiros da tabela, com os usos no texto traduzido.
    pub top: Vec<(String, usize)>,
    /// Letras por codigo medidas (`PAIR_RATE`), x100.
    pub rate_x100: usize,
    pub without: Measure,
    pub with: Measure,
}

/// Escolhe os pares pelo texto ja traduzido do projeto (o mais usado
/// primeiro, ate `MAX_PAIRS`), grava a lista, extrai de novo (a tabela ganha
/// os que cabem) e mede o texto traduzido gravado sem e com eles.
pub fn make(project_dir: &Path) -> Result<Outcome> {
    let id = crate::kuten::TABLE_ID;
    let entries = ProjectDb::open(project_dir)?.load_entries()?;
    let ranked = ranked(translations(&entries, id));
    let list = List {
        pares: ranked
            .iter()
            .take_while(|(_, uses)| *uses >= MIN_USES)
            .take(MAX_PAIRS)
            .map(|(pair, _)| pair.clone())
            .collect(),
    };
    let path = file(project_dir);
    let json = serde_json::to_string_pretty(&list)?;
    fs::write(&path, json + "\n").map_err(|e| CoreError::io(&path, e))?;
    crate::adapters::extract_project(project_dir)?;

    let table_path = project_dir
        .join(crate::tbl::TABLES_DIR)
        .join(format!("{id}.tbl"));
    let text = fs::read_to_string(&table_path).map_err(|e| CoreError::io(&table_path, e))?;
    let with = TblTable::parse(&text)?;
    let without = TblTable::parse(
        &text
            .lines()
            .filter(|line| !line.starts_with(PAIR_RATE))
            .filter(|line| {
                !line
                    .split_once('=')
                    .is_some_and(|(_, value)| is_pair(value))
            })
            .fold(String::new(), |mut out, line| {
                let _ = writeln!(out, "{line}");
                out
            }),
    )?;
    let entries = ProjectDb::open(project_dir)?.load_entries()?;
    let encoding = TextEncoding::Table(id.to_string());
    let translated: Vec<&TextEntry> = entries
        .iter()
        .filter(|e| e.encoding == encoding && e.status != TranslationStatus::Internal)
        .filter(|e| e.translated_text.is_some())
        .collect();
    let uses: HashMap<&str, usize> = ranked.iter().map(|(p, n)| (p.as_str(), *n)).collect();
    let mut in_table: Vec<(String, usize)> = drawn(&with)
        .into_values()
        .map(|pair| {
            let n = uses.get(pair.as_str()).copied().unwrap_or(0);
            (pair, n)
        })
        .collect();
    in_table.sort_by(|(a, x), (b, y)| y.cmp(x).then_with(|| a.cmp(b)));
    let (letters, codes) = rate(&with, &translations(&entries, id));
    Ok(Outcome {
        wanted: list.pares.len(),
        in_table: in_table.len(),
        top: in_table.into_iter().take(20).collect(),
        rate_x100: (letters * 100).checked_div(codes).unwrap_or(0),
        without: measure(&without, &translated),
        with: measure(&with, &translated),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_are_counted_in_the_text_and_never_across_a_code() {
        let ranked = ranked(["ao ao[FF01]ao", "Ao  ão"]);
        let uses = |pair: &str| ranked.iter().find(|(p, _)| p == pair).map(|(_, n)| *n);
        assert_eq!(uses("ao"), Some(3));
        assert_eq!(uses("o "), Some(2));
        assert_eq!(uses(" a"), Some(1));
        assert_eq!(uses("Ao"), Some(1), "maiuscula conta a parte");
        assert_eq!(uses("ão"), Some(1));
        assert_eq!(uses("  "), None, "espaco dos dois lados nao");
        assert_eq!(uses("o["), None, "par nao atravessa codigo");
        assert_eq!(ranked[0], ("ao".to_string(), 3), "o mais usado primeiro");
    }

    #[test]
    fn the_rate_line_goes_only_into_a_table_with_pairs() {
        let plain = "0100= \n1001=a\n1101=o\n";
        assert_eq!(with_rate(plain, &["ao ao"]), plain);
        let paired = format!("{plain}1002=ao\n");
        let text = with_rate(&paired, &["ao ao[FF01]"]);
        // "ao ao": 5 letras em 3 codigos ("ao", " ", "ao").
        assert!(
            text.starts_with(&format!("{PAIR_RATE}5 letras em 3 codigos")),
            "{text}"
        );
        assert_eq!(TblTable::parse(&text).unwrap().letters_in(3), 5);
        // Sem texto traduzido, sem conta.
        assert_eq!(with_rate(&paired, &[]), paired);
    }
}

//! Busca relativa: acha texto em tabela de caracteres desconhecida a partir
//! de uma palavra que o usuario sabe que aparece no jogo, e monta o `.tbl`.
//!
//! Jogo de cartucho raramente usa ASCII — cada um tem a propria tabela, e o
//! scanner ASCII voltar vazio NAO prova que nao ha texto. Mas quase toda
//! tabela mantem as letras em ordem alfabetica: "START" aparece como bytes
//! cuja distancia entre si e a de S-T-A-R-T, seja qual for o byte de 'A'.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::project::load_project;
use crate::scan::MAX_SCAN_FILE_SIZE;

pub const MIN_WORD_LEN: usize = 4;
/// Letras diferentes exigidas num mesmo caso: "AAAA" casaria com qualquer
/// sequencia de 4 bytes iguais.
const MIN_DISTINCT: usize = 3;
const MAX_CANDIDATES: usize = 8;
const MAX_OFFSETS: usize = 20;
const LETTERS: u8 = 26;
const PREVIEW_BEFORE: usize = 12;
const PREVIEW_LEN: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelativeCandidate {
    /// Byte de 'A'. `None` = a palavra buscada nao tinha maiuscula.
    pub upper_base: Option<u8>,
    /// Byte de 'a'.
    pub lower_base: Option<u8>,
    /// Quantas vezes a palavra aparece com estas bases.
    pub hits: usize,
    /// As primeiras posicoes.
    pub offsets: Vec<u64>,
    /// Trecho em volta da primeira ocorrencia lido com a tabela candidata
    /// ('·' = byte fora dela) — e o que o usuario confere no olho.
    pub preview: String,
    /// As bases sao as do ASCII: o scanner ASCII ja le este jogo.
    pub ascii: bool,
}

/// (maiuscula?, posicao no alfabeto) de cada letra da palavra.
fn parse_word(word: &str) -> Result<Vec<(bool, u8)>> {
    let letters: Vec<(bool, u8)> = word
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' => Ok((true, b - b'A')),
            b'a'..=b'z' => Ok((false, b - b'a')),
            _ => Err(CoreError::Tbl(format!(
                "a busca relativa so aceita letras de A a Z, sem espaco, numero ou acento \
                 (\"{word}\")"
            ))),
        })
        .collect::<Result<_>>()?;
    let distinct = |upper: bool| {
        let mut seen = [false; LETTERS as usize];
        for &(u, idx) in &letters {
            seen[idx as usize] |= u == upper;
        }
        seen.iter().filter(|&&s| s).count()
    };
    if letters.len() < MIN_WORD_LEN || distinct(true).max(distinct(false)) < MIN_DISTINCT {
        return Err(CoreError::Tbl(format!(
            "use uma palavra com pelo menos {MIN_WORD_LEN} letras e {MIN_DISTINCT} letras \
             diferentes — palavra curta ou repetitiva casa com lixo"
        )));
    }
    Ok(letters)
}

fn decode(byte: u8, upper: Option<u8>, lower: Option<u8>, space: Option<u8>) -> Option<char> {
    let letter = |base: Option<u8>, first: u8| {
        let idx = byte.checked_sub(base?)?;
        (idx < LETTERS).then(|| (first + idx) as char)
    };
    letter(upper, b'A')
        .or_else(|| letter(lower, b'a'))
        .or_else(|| (Some(byte) == space).then_some(' '))
}

/// Byte que separa palavras: o mais comum entre duas duplas de letras. So
/// vale quando domina — fora de texto, byte na faixa das letras aparece a toa.
// ponytail: nunca infere 0x00 nem 0xFF (terminador e padding de ROM: o
// scanner leria o cartucho vazio como um mar de espacos). Jogo que usa um
// dos dois como espaco pede a linha "00= " escrita a mao no .tbl.
fn infer_space(data: &[u8], upper: Option<u8>, lower: Option<u8>) -> Option<u8> {
    let is_letter = |b: u8| decode(b, upper, lower, None).is_some();
    let mut counts = [0u32; 256];
    for w in data.windows(5) {
        if is_letter(w[0])
            && is_letter(w[1])
            && !is_letter(w[2])
            && is_letter(w[3])
            && is_letter(w[4])
        {
            counts[w[2] as usize] += 1;
        }
    }
    counts[0x00] = 0;
    counts[0xFF] = 0;
    let (best, &top) = counts.iter().enumerate().max_by_key(|&(_, &n)| n)?;
    let second = counts
        .iter()
        .enumerate()
        .filter(|&(b, _)| b != best)
        .map(|(_, &n)| n)
        .max()
        .unwrap_or(0);
    (top >= 8 && top >= second * 2).then_some(best as u8)
}

fn check_bases(upper: Option<u8>, lower: Option<u8>) -> Result<()> {
    let fits = |base: Option<u8>| base.is_none_or(|b| b <= u8::MAX - (LETTERS - 1));
    let overlap = matches!((upper, lower), (Some(u), Some(l)) if u.abs_diff(l) < LETTERS);
    if (upper.is_none() && lower.is_none()) || !fits(upper) || !fits(lower) || overlap {
        return Err(CoreError::Tbl(
            "bases invalidas pra tabela: o alfabeto nao cabe num byte ou maiusculas e \
             minusculas ocupam os mesmos bytes"
                .to_string(),
        ));
    }
    Ok(())
}

/// Procura `word` em `data` por distancia entre letras. Devolve as bases
/// candidatas, da que mais aparece pra que menos aparece.
// ponytail: so 1 byte por caractere. Jogo com caractere de 2 bytes (parte dos
// japoneses de SNES/GBA) pede a variante de 16 bits.
pub fn relative_search(data: &[u8], word: &str) -> Result<Vec<RelativeCandidate>> {
    let letters = parse_word(word)?;
    let mut groups: HashMap<(Option<u8>, Option<u8>), Vec<u64>> = HashMap::new();

    for (offset, window) in data.windows(letters.len()).enumerate() {
        let mut upper = None;
        let mut lower = None;
        let matched = window
            .iter()
            .zip(&letters)
            .all(|(&byte, &(is_upper, idx))| {
                let Some(base) = byte.checked_sub(idx) else {
                    return false;
                };
                let slot = if is_upper { &mut upper } else { &mut lower };
                *slot.get_or_insert(base) == base
            });
        if matched && check_bases(upper, lower).is_ok() {
            groups
                .entry((upper, lower))
                .or_default()
                .push(offset as u64);
        }
    }

    // Ordem total (hits, depois posicao): mesmo arquivo => mesma lista.
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by_key(|(_, offsets)| (std::cmp::Reverse(offsets.len()), offsets[0]));
    groups.truncate(MAX_CANDIDATES);

    // A relacao entre as letras e a mesma em qualquer caso: "START" tambem
    // casa com "start" em ASCII, e a base sai como a do outro caso.
    let is_ascii = |base: Option<u8>| base.is_none_or(|b| b == b'A' || b == b'a');
    Ok(groups
        .into_iter()
        .map(|((upper_base, lower_base), offsets)| {
            let space = infer_space(data, upper_base, lower_base);
            let start = (offsets[0] as usize).saturating_sub(PREVIEW_BEFORE);
            let end = (start + PREVIEW_LEN).min(data.len());
            RelativeCandidate {
                upper_base,
                lower_base,
                hits: offsets.len(),
                preview: data[start..end]
                    .iter()
                    .map(|&b| decode(b, upper_base, lower_base, space).unwrap_or('·'))
                    .collect(),
                ascii: is_ascii(upper_base) && is_ascii(lower_base),
                offsets: offsets.into_iter().take(MAX_OFFSETS).collect(),
            }
        })
        .collect())
}

/// Texto de um `.tbl` com as letras das bases dadas e, quando da pra inferir,
/// o espaco. Numeros e pontuacao ficam por conta do usuario.
pub fn build_tbl(data: &[u8], upper_base: Option<u8>, lower_base: Option<u8>) -> Result<String> {
    check_bases(upper_base, lower_base)?;
    let mut out = String::from(
        "# Tabela gerada pela busca relativa do RomTranslate Studio.\n\
         # Traz so as letras (e o espaco, quando deu pra inferir): complete numeros e\n\
         # pontuacao a mao, uma linha HEX=texto por caractere.\n",
    );
    for (base, first) in [(upper_base, b'A'), (lower_base, b'a')] {
        let Some(base) = base else { continue };
        for idx in 0..LETTERS {
            let _ = writeln!(out, "{:02X}={}", base + idx, (first + idx) as char);
        }
    }
    if let Some(space) = infer_space(data, upper_base, lower_base) {
        let _ = writeln!(out, "{space:02X}= ");
    }
    Ok(out)
}

fn read_source(project_dir: &Path) -> Result<Vec<u8>> {
    let source = load_project(project_dir)?.source_path;
    let size = fs::metadata(&source)
        .map_err(|e| CoreError::io(&source, e))?
        .len();
    if size > MAX_SCAN_FILE_SIZE {
        return Err(CoreError::FileTooLarge {
            size,
            limit: MAX_SCAN_FILE_SIZE,
        });
    }
    fs::read(&source).map_err(|e| CoreError::io(&source, e))
}

/// Busca relativa no arquivo de origem do projeto.
pub fn search_project(project_dir: &Path, word: &str) -> Result<Vec<RelativeCandidate>> {
    relative_search(&read_source(project_dir)?, word)
}

/// Grava o `.tbl` das bases em `<projeto>/tables/` e devolve o caminho. Um
/// arquivo que ja existe e mantido: o usuario pode te-lo completado a mao.
pub fn save_tbl(
    project_dir: &Path,
    upper_base: Option<u8>,
    lower_base: Option<u8>,
) -> Result<PathBuf> {
    let text = build_tbl(&read_source(project_dir)?, upper_base, lower_base)?;
    let hex = |base: Option<u8>| base.map_or("xx".to_string(), |b| format!("{b:02X}"));
    let dir = project_dir.join("tables");
    let path = dir.join(format!(
        "letras-{}-{}.tbl",
        hex(upper_base),
        hex(lower_base)
    ));
    if !path.exists() {
        fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
        fs::write(&path, text).map_err(|e| CoreError::io(&path, e))?;
    }
    Ok(path)
}

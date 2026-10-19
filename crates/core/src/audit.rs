//! Auditoria: quanto do jogo ainda esta no idioma de origem.
//!
//! A prova de "tudo traduzido" e o jogo, nao a lista do projeto: a copia de
//! trabalho e lida de novo com a mesma extracao do app, e cada trecho que
//! ainda tem a escrita do idioma de origem entra na conta, com o motivo. So o
//! japones tem escrita propria pra contar (kana e kanji); em outro idioma,
//! conta o trecho que esta no jogo igual ao original (sem traducao, com erro
//! ou nao gravado). Com perfil do jogo, a releitura e a do perfil.
//!
//! O que a releitura nao alcanca fica em contas a parte: frase desenhada (a
//! traducao vira pixel) e imagem que ninguem conferiu.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::adapters;
use crate::db::ProjectDb;
use crate::error::{CoreError, Result};
use crate::project::load_project;
use crate::types::{TextEncoding, TextEntry, TranslationStatus};
use crate::validate::{validate_project_db, Severity};

/// Imagens conferidas sem texto do idioma de origem: um id por linha, `#`
/// comeca comentario. Fica na pasta das imagens do projeto.
pub const WITHOUT_TEXT_FILE: &str = "sem-texto.txt";

/// Por que um trecho ainda esta no idioma de origem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// A frase nao tem traducao.
    Untranslated,
    /// A traducao tem erro de validacao: a reinsercao deixa o original.
    Error,
    /// Traduzida, sem erro, e mesmo assim igual no jogo.
    NotWritten,
    /// Texto que o projeto nao tem: a extracao do original nao o leu.
    NotInProject,
}

/// Um trecho que segue no idioma de origem.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    /// A frase do projeto (ou o id da releitura, se o projeto nao a tem).
    pub id: String,
    /// Onde o trecho esta na copia traduzida.
    pub offset: Option<u64>,
    /// Caracteres do idioma de origem no trecho.
    pub chars: usize,
    pub text: String,
    pub why: Why,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureAudit {
    /// Imagens com quadros que parecem letra (`images::find`).
    pub found: usize,
    /// Lidas pela IA: tem frase desenhada no projeto.
    pub read: usize,
    /// Conferidas sem texto do idioma de origem (`WITHOUT_TEXT_FILE`).
    pub without_text: usize,
    /// Ninguem conferiu ainda.
    pub unchecked: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReport {
    pub working_path: PathBuf,
    /// Caracteres do idioma de origem nas frases de texto do projeto.
    pub source_chars: usize,
    /// Os que seguem no jogo: os da releitura e os das frases desenhadas que
    /// nao vao pro desenho.
    pub remaining_chars: usize,
    pub remaining: Vec<Leftover>,
    /// Frases marcadas como nao-texto (fora da conta) com a escrita do idioma
    /// de origem: as que vale olhar, porque fala marcada por engano fica
    /// escondida aqui.
    pub not_text: usize,
    pub pictures: PictureAudit,
}

/// Le a copia traduzida de novo e conta o que segue no idioma de origem.
pub fn audit_project(project_dir: &Path) -> Result<AuditReport> {
    let project = load_project(project_dir)?;
    let file_name = project
        .source_path
        .file_name()
        .ok_or_else(|| CoreError::Project("origem sem nome de arquivo".to_string()))?;
    let working_path = project_dir.join("working").join(file_name);
    if !working_path.is_file() {
        return Err(CoreError::Project(
            "a copia traduzida nao existe: reinsira antes de auditar".to_string(),
        ));
    }
    let adapter = adapters::find(&project.adapter_id)
        .ok_or_else(|| CoreError::Project(format!("adapter {} nao existe", project.adapter_id)))?;
    let japanese = project
        .source_language
        .as_deref()
        .is_some_and(|lang| lang.starts_with("ja"));
    let count = |text: &str| source_chars(text, japanese);

    let mut db = ProjectDb::open(project_dir)?;
    let validation = validate_project_db(&mut db)?;
    let errors: HashSet<String> = validation
        .issues
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.entry_id)
        .collect();
    let entries = db.load_entries()?;
    let by_id: HashMap<&str, &TextEntry> = entries.iter().map(|e| (e.id.as_str(), e)).collect();
    // Mensagem que ficou num bloco remontado muda de lugar: e achada pelo
    // texto.
    let mut by_text: HashMap<&str, &TextEntry> = HashMap::new();
    for e in &entries {
        by_text
            .entry(e.source_text.as_str())
            .and_modify(|seen| {
                if e.translated_text.is_none() && seen.translated_text.is_some() {
                    *seen = e;
                }
            })
            .or_insert(e);
    }

    let text: Vec<&TextEntry> = entries
        .iter()
        .filter(|e| e.status != TranslationStatus::Internal)
        .collect();
    let mut report = AuditReport {
        working_path: working_path.clone(),
        source_chars: text.iter().map(|e| count(&e.source_text)).sum(),
        remaining_chars: 0,
        remaining: Vec::new(),
        // Fora do japones, letra nao separa fala de lixo: conta o que tem cara
        // de texto.
        not_text: entries
            .iter()
            .filter(|e| e.status == TranslationStatus::Internal)
            .filter(|e| {
                if japanese {
                    count(&e.source_text) > 0
                } else {
                    crate::scan::looks_like_text(&e.source_text)
                }
            })
            .count(),
        pictures: PictureAudit::default(),
    };
    let why_of = |e: &TextEntry| {
        if e.translated_text.is_none() {
            Why::Untranslated
        } else if errors.contains(&e.id) {
            Why::Error
        } else {
            Why::NotWritten
        }
    };

    // A mesma extracao do projeto: com perfil, a dele (a varredura do adapter
    // nem chega no banco que o perfil descreve).
    let data = crate::fileio::read_view(&working_path)?;
    let again = match crate::profile::load(project_dir)? {
        Some(profile) => crate::profile::reread(project_dir, &data, &profile)?,
        None => adapter.extract_with(&data, &adapters::hints(project_dir)?)?,
    }
    .entries;
    let stray: HashSet<String> = crate::scan::stray_ids(&again).into_iter().collect();
    for e in again
        .iter()
        .filter(|e| e.status != TranslationStatus::Internal && !stray.contains(&e.id))
    {
        let chars = count(&e.source_text);
        if chars == 0 {
            continue;
        }
        let known = by_id
            .get(e.id.as_str())
            .filter(|old| old.source_text == e.source_text)
            .or_else(|| by_text.get(e.source_text.as_str()));
        let why = match known {
            Some(old) if old.status == TranslationStatus::Internal => continue,
            // Traducao igual ao original (nome proprio): esta gravada.
            Some(old) if old.translated_text.as_deref() == Some(old.source_text.as_str()) => {
                continue
            }
            Some(old) => why_of(old),
            // Fora do projeto so conta onde ha escrita pra reconhecer.
            None if japanese => Why::NotInProject,
            None => continue,
        };
        report.remaining.push(Leftover {
            id: known.map_or(e.id.clone(), |old| old.id.clone()),
            offset: e.offset,
            chars,
            text: e.source_text.clone(),
            why,
        });
    }

    // Frase desenhada vira pixel: segue no original a que nao vai pro desenho.
    for e in text.iter().filter(|e| e.encoding == TextEncoding::Image) {
        let why = why_of(e);
        if why != Why::NotWritten {
            report.remaining.push(Leftover {
                id: e.id.clone(),
                offset: e.offset,
                chars: count(&e.source_text),
                text: e.source_text.clone(),
                why,
            });
        }
    }
    report.remaining_chars = report.remaining.iter().map(|l| l.chars).sum();
    report.pictures = pictures(project_dir, &project.source_path, &entries)?;
    Ok(report)
}

/// Caracteres do idioma de origem: kana e kanji no japones; letras nos
/// outros (ali so conta o trecho igual ao original).
fn source_chars(text: &str, japanese: bool) -> usize {
    text.chars()
        .filter(|&c| {
            if japanese {
                crate::sjis::is_kana(c) || crate::sjis::is_kanji(c)
            } else {
                c.is_alphabetic()
            }
        })
        .count()
}

fn pictures(project_dir: &Path, source: &Path, entries: &[TextEntry]) -> Result<PictureAudit> {
    let data = crate::fileio::read_view(source)?;
    let found = crate::images::find(&data);
    let read: HashSet<String> = entries
        .iter()
        .filter_map(|e| crate::images::place_of(e).map(|(picture, _)| picture.id))
        .collect();
    let without = without_text(project_dir);
    let unchecked = found
        .iter()
        .filter(|p| !read.contains(&p.id) && !without.contains(&p.id))
        .map(|p| p.id.clone())
        .collect();
    Ok(PictureAudit {
        found: found.len(),
        read: found.iter().filter(|p| read.contains(&p.id)).count(),
        without_text: found.iter().filter(|p| without.contains(&p.id)).count(),
        unchecked,
    })
}

/// Os ids de `WITHOUT_TEXT_FILE` (vazio se o arquivo nao existe).
pub fn without_text(project_dir: &Path) -> HashSet<String> {
    let path = project_dir
        .join(crate::images::PICTURES_DIR)
        .join(WITHOUT_TEXT_FILE);
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

/// Acrescenta `ids` a `WITHOUT_TEXT_FILE`, sem repetir.
pub fn mark_without_text(project_dir: &Path, ids: &[String]) -> Result<usize> {
    let dir = project_dir.join(crate::images::PICTURES_DIR);
    fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    let path = dir.join(WITHOUT_TEXT_FILE);
    let mut known = without_text(project_dir);
    let mut text = fs::read_to_string(&path).unwrap_or_default();
    let mut added = 0;
    for id in ids {
        if known.insert(id.clone()) {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(id);
            text.push('\n');
            added += 1;
        }
    }
    fs::write(&path, text).map_err(|e| CoreError::io(&path, e))?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_kana_and_kanji_in_japanese_and_letters_elsewhere() {
        assert_eq!(source_chars("ＰＳＩを 使った！[FF01]", true), 4);
        assert_eq!(source_chars("Usou PSI!", true), 0);
        assert_eq!(source_chars("Use PSI!", false), 6);
    }

    #[test]
    fn marked_pictures_are_read_back_once() {
        let dir = std::env::temp_dir().join(format!(
            "rts-audit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ids = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            mark_without_text(&dir, &ids(&["img-1", "img-2"])).unwrap(),
            2
        );
        assert_eq!(
            mark_without_text(&dir, &ids(&["img-2", "img-3"])).unwrap(),
            1
        );
        let known = without_text(&dir);
        assert_eq!(known.len(), 3);
        assert!(known.contains("img-3"));
        fs::remove_dir_all(&dir).unwrap();
    }
}

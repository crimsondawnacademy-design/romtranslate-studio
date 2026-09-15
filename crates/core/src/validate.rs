//! Validacao de traducoes (spec §14): placeholders/control codes/tags,
//! limite de bytes no encoding destino, strings vazias e anomalias de
//! comprimento. Erros aparecem ANTES de qualquer reinsercao — a UI bloqueia
//! reinsercao de Error (Sprint 5 usa esses statuses).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::db::{GlobalTm, ProjectDb};
use crate::error::Result;
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// Placeholder/control code/tag removido ou inventado ({0}, %s, <BR>, [ITEM], \n).
    PlaceholderMismatch,
    /// Traducao nao cabe: acima de max_bytes (Error) ou do espaco original (Warning).
    ByteOverflow,
    /// Original tem texto, traducao esta vazia.
    EmptyTranslation,
    /// Comprimento mudou de forma anormal (>3x ou <1/3).
    LengthAnomaly,
    /// Traducao nao e codificavel no encoding destino (ex.: "ç" em ASCII).
    Unencodable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub entry_id: String,
    pub severity: Severity,
    pub kind: IssueKind,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
    pub errors: usize,
    pub warnings: usize,
    pub checked: usize,
}

/// Tamanho da traducao codificada no encoding destino.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodedLen {
    Bytes(usize),
    /// O texto tem chars que o encoding nao representa.
    Unencodable,
    /// Sem encoder implementado (tabela custom, Shift-JIS) — check pulado.
    Unsupported,
}

pub fn encoded_len(text: &str, encoding: &TextEncoding) -> EncodedLen {
    match encoding {
        TextEncoding::Ascii => {
            if text.is_ascii() {
                EncodedLen::Bytes(text.len())
            } else {
                EncodedLen::Unencodable
            }
        }
        TextEncoding::Utf8 => EncodedLen::Bytes(text.len()),
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            EncodedLen::Bytes(text.encode_utf16().count() * 2)
        }
        TextEncoding::ShiftJis | TextEncoding::Table(_) => EncodedLen::Unsupported,
    }
}

/// Tokens que a traducao e obrigada a preservar: {..}, <..>, [..], %x e \x.
pub fn extract_tokens(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            open @ ('{' | '<' | '[') => {
                let close = match open {
                    '{' => '}',
                    '<' => '>',
                    _ => ']',
                };
                if let Some(end) = chars[i + 1..].iter().position(|&c| c == close) {
                    let token: String = chars[i..=i + 1 + end].iter().collect();
                    tokens.push(token);
                    i += end + 2;
                } else {
                    i += 1;
                }
            }
            '%' => {
                // %% (escape), %s, %d, %1$s...
                let mut j = i + 1;
                if j < chars.len() && chars[j] == '%' {
                    tokens.push("%%".to_string());
                    i = j + 1;
                    continue;
                }
                while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '$') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_alphabetic() {
                    let token: String = chars[i..=j].iter().collect();
                    tokens.push(token);
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
            '\\' if i + 1 < chars.len() => {
                tokens.push(chars[i..=i + 1].iter().collect());
                i += 2;
            }
            _ => i += 1,
        }
    }
    tokens
}

fn count_tokens(text: &str) -> HashMap<String, i32> {
    let mut map = HashMap::new();
    for token in extract_tokens(text) {
        *map.entry(token).or_insert(0) += 1;
    }
    map
}

/// Valida UMA entry traduzida. Entry sem traducao retorna vazio.
pub fn validate_entry(entry: &TextEntry) -> Vec<ValidationIssue> {
    let Some(translation) = entry.translated_text.as_deref() else {
        return Vec::new();
    };
    let mut issues = Vec::new();
    let mut push = |severity: Severity, kind: IssueKind, message: String| {
        issues.push(ValidationIssue {
            entry_id: entry.id.clone(),
            severity,
            kind,
            message,
        });
    };

    // Vazia indevida.
    if translation.trim().is_empty() && !entry.source_text.trim().is_empty() {
        push(
            Severity::Error,
            IssueKind::EmptyTranslation,
            "traducao vazia para texto nao-vazio".to_string(),
        );
        return issues;
    }

    // Placeholders / control codes / tags (multiset dos dois lados).
    let source_tokens = count_tokens(&entry.source_text);
    let translation_tokens = count_tokens(translation);
    for (token, &n) in &source_tokens {
        let have = translation_tokens.get(token).copied().unwrap_or(0);
        if have < n {
            push(
                Severity::Error,
                IssueKind::PlaceholderMismatch,
                format!("placeholder {token} removido da traducao"),
            );
        }
    }
    for (token, &n) in &translation_tokens {
        let had = source_tokens.get(token).copied().unwrap_or(0);
        if n > had {
            push(
                Severity::Error,
                IssueKind::PlaceholderMismatch,
                format!("placeholder {token} nao existe no original"),
            );
        }
    }

    // Bytes no encoding destino.
    match encoded_len(translation, &entry.encoding) {
        EncodedLen::Unencodable => push(
            Severity::Error,
            IssueKind::Unencodable,
            "traducao tem caracteres fora do encoding destino".to_string(),
        ),
        EncodedLen::Bytes(n) => {
            if let Some(max) = entry.max_bytes {
                if n > max {
                    push(
                        Severity::Error,
                        IssueKind::ByteOverflow,
                        format!("traducao ocupa {n} bytes; limite do campo e {max}"),
                    );
                }
            } else if n > entry.original_bytes.len() {
                push(
                    Severity::Warning,
                    IssueKind::ByteOverflow,
                    format!(
                        "traducao ocupa {n} bytes; o espaco original tem {} (reinsercao fixa exigiria texto menor)",
                        entry.original_bytes.len()
                    ),
                );
            }
        }
        EncodedLen::Unsupported => {}
    }

    // Anomalia de comprimento (em chars, so para textos nao-minusculos).
    let source_chars = entry.source_text.chars().count();
    let translation_chars = translation.chars().count();
    if source_chars >= 4
        && (translation_chars > source_chars * 3
            || (translation_chars > 0 && translation_chars * 3 < source_chars))
    {
        push(
            Severity::Warning,
            IssueKind::LengthAnomaly,
            format!("comprimento mudou de {source_chars} para {translation_chars} chars"),
        );
    }

    issues
}

/// Valida todas as entries traduzidas do banco e ajusta statuses:
/// - com Error -> `Error`;
/// - `Error` antigo que ficou limpo -> volta para `Machine`;
/// - `Reviewed` limpo permanece `Reviewed`.
pub fn validate_project_db(db: &mut ProjectDb) -> Result<ValidationReport> {
    let entries = db.load_entries()?;
    let mut issues = Vec::new();
    let mut checked = 0;
    for entry in &entries {
        if entry.translated_text.is_none() {
            continue;
        }
        checked += 1;
        let entry_issues = validate_entry(entry);
        let has_error = entry_issues.iter().any(|i| i.severity == Severity::Error);
        if has_error {
            db.set_status(&entry.id, TranslationStatus::Error)?;
        } else if entry.status == TranslationStatus::Error {
            db.set_status(&entry.id, TranslationStatus::Machine)?;
        }
        issues.extend(entry_issues);
    }
    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    let warnings = issues.len() - errors;
    Ok(ValidationReport {
        issues,
        errors,
        warnings,
        checked,
    })
}

/// Edicao manual: grava a traducao (status Machine), alimenta a TM e valida.
/// Se a validacao tiver Error, o status vira `Error`. Retorna as issues.
pub fn apply_manual_translation(
    db: &mut ProjectDb,
    global_tm: Option<&mut GlobalTm>,
    id: &str,
    translation: &str,
    source_language: &str,
    target_language: &str,
) -> Result<Vec<ValidationIssue>> {
    db.update_translation(id, translation, TranslationStatus::Machine)?;
    let Some(entry) = db.get_entry(id)? else {
        return Err(crate::error::CoreError::Project(format!(
            "entry {id} nao existe"
        )));
    };
    db.tm_store(
        &entry.source_text,
        source_language,
        target_language,
        translation,
    )?;
    if let Some(global) = global_tm {
        global.tm_store(
            &entry.source_text,
            source_language,
            target_language,
            translation,
        )?;
    }
    let issues = validate_entry(&entry);
    if issues.iter().any(|i| i.severity == Severity::Error) {
        db.set_status(id, TranslationStatus::Error)?;
    }
    Ok(issues)
}

/// Marca/desmarca revisao manual. Marcar exige validacao sem Error.
pub fn set_reviewed(db: &mut ProjectDb, id: &str, reviewed: bool) -> Result<TranslationStatus> {
    let Some(entry) = db.get_entry(id)? else {
        return Err(crate::error::CoreError::Project(format!(
            "entry {id} nao existe"
        )));
    };
    let status = if reviewed {
        let issues = validate_entry(&entry);
        if issues.iter().any(|i| i.severity == Severity::Error) {
            return Err(crate::error::CoreError::Project(
                "ha erros de validacao nesta entry; corrija antes de marcar como revisada"
                    .to_string(),
            ));
        }
        if entry.translated_text.is_none() {
            return Err(crate::error::CoreError::Project(
                "entry sem traducao nao pode ser marcada como revisada".to_string(),
            ));
        }
        TranslationStatus::Reviewed
    } else if entry.translated_text.is_some() {
        TranslationStatus::Machine
    } else {
        TranslationStatus::Untranslated
    };
    db.set_status(id, status)?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TranslationStatus;

    fn entry(source: &str, translation: &str, encoding: TextEncoding) -> TextEntry {
        TextEntry {
            id: "t".into(),
            resource_path: None,
            offset: None,
            original_bytes: source.as_bytes().to_vec(),
            source_text: source.into(),
            translated_text: Some(translation.into()),
            context: None,
            max_bytes: None,
            encoding,
            status: TranslationStatus::Machine,
            metadata: serde_json::Value::Null,
        }
    }

    fn kinds(issues: &[ValidationIssue]) -> Vec<IssueKind> {
        issues.iter().map(|i| i.kind).collect()
    }

    #[test]
    fn extracts_all_token_families() {
        let tokens = extract_tokens("HP {0}: %d <BR>[ITEM] 100%% \\n end");
        assert_eq!(tokens, vec!["{0}", "%d", "<BR>", "[ITEM]", "%%", "\\n"]);
        assert!(extract_tokens("plain text 100% legal").is_empty());
    }

    #[test]
    fn placeholder_removal_and_invention_are_errors() {
        let removed = validate_entry(&entry("HP {0}: 120", "PV: 120", TextEncoding::Utf8));
        assert!(kinds(&removed).contains(&IssueKind::PlaceholderMismatch));
        assert!(removed[0].message.contains("{0}"));

        let invented = validate_entry(&entry("Save game", "Salvar %s", TextEncoding::Utf8));
        assert!(kinds(&invented).contains(&IssueKind::PlaceholderMismatch));

        let ok = validate_entry(&entry("HP {0}: %d", "PV {0}: %d", TextEncoding::Utf8));
        assert!(ok.is_empty(), "{ok:?}");
    }

    #[test]
    fn byte_limits_and_encodings() {
        // max_bytes explicito estourado -> Error.
        let mut e = entry("SAVE", "SALVAR O JOGO AGORA", TextEncoding::Ascii);
        e.max_bytes = Some(6);
        let issues = validate_entry(&e);
        assert!(issues
            .iter()
            .any(|i| i.kind == IssueKind::ByteOverflow && i.severity == Severity::Error));

        // Sem max_bytes: acima do espaco original -> Warning.
        let long = validate_entry(&entry("HI", "OLA MUNDO", TextEncoding::Ascii));
        assert!(long
            .iter()
            .any(|i| i.kind == IssueKind::ByteOverflow && i.severity == Severity::Warning));

        // Nao-ASCII em encoding ASCII -> Unencodable.
        let bad = validate_entry(&entry("POTION", "POÇÃO", TextEncoding::Ascii));
        assert!(kinds(&bad).contains(&IssueKind::Unencodable));

        // UTF-16: 2 bytes por unidade.
        assert_eq!(
            encoded_len("ABC", &TextEncoding::Utf16Le),
            EncodedLen::Bytes(6)
        );
        // Tabela custom: sem encoder -> check pulado.
        assert_eq!(
            encoded_len("ABC", &TextEncoding::Table("x".into())),
            EncodedLen::Unsupported
        );
    }

    #[test]
    fn empty_and_anomalous_translations() {
        let empty = validate_entry(&entry("WELCOME!", "  ", TextEncoding::Utf8));
        assert_eq!(kinds(&empty), vec![IssueKind::EmptyTranslation]);

        let bloated = validate_entry(&entry(
            "MENU",
            "este e um texto absurdamente maior que o original",
            TextEncoding::Utf8,
        ));
        assert!(kinds(&bloated).contains(&IssueKind::LengthAnomaly));

        let untranslated = TextEntry {
            translated_text: None,
            ..entry("X", "y", TextEncoding::Utf8)
        };
        assert!(validate_entry(&untranslated).is_empty());
    }
}

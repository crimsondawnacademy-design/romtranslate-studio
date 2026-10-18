//! Validacao de traducoes (spec §14): placeholders/control codes/tags,
//! limite de bytes no encoding destino, strings vazias e anomalias de
//! comprimento. Erros aparecem ANTES de qualquer reinsercao — a UI bloqueia
//! reinsercao de Error (Sprint 5 usa esses statuses).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::adapters::inplace::fill;
use crate::db::{GlobalTm, ProjectDb};
use crate::error::Result;
use crate::tbl::{ProjectTables, TblTable};
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
    /// Traducao nao cabe: acima de max_bytes (Error) ou do espaco original
    /// (Warning; Error em texto de tabela, que so e gravado no lugar). Tambem
    /// a string de largura fixa lida por tabela cuja sobra nao da pra
    /// preencher (Error).
    ByteOverflow,
    /// Original tem texto, traducao esta vazia.
    EmptyTranslation,
    /// Comprimento mudou de forma anormal (>3x ou <1/3).
    LengthAnomaly,
    /// Traducao nao e codificavel no encoding destino (ex.: "ç" em ASCII).
    Unencodable,
    /// A traducao automatica veio com acento (ou aspas e travessao
    /// tipograficos) que o texto da string nao escreve — ASCII ou a tabela do
    /// jogo — e foi gravada com o equivalente simples (Warning).
    AccentDropped,
    /// O original tem letra e a traducao nenhuma ("?????", "..."): quase
    /// sempre dado lido como texto, que a IA nao soube traduzir. Gravar isso
    /// troca o dado do jogo por pontuacao (medido em 16/10/2026: tabelas,
    /// mapas de quadros e a lista de silabas do teclado de nomes).
    NoLetters,
    /// Original japones e traducao ainda com kana ou kanji: nao foi
    /// traduzida (o modelo devolveu o japones).
    SourceScript,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodedLen {
    Bytes(usize),
    /// O texto nao vira bytes neste encoding; a mensagem diz por que.
    Unencodable(String),
}

/// `table` = a tabela do projeto pra entry de encoding `Table`
/// (`ProjectTables::for_entry`); os outros encodings nao olham pra ela.
pub fn encoded_len(text: &str, encoding: &TextEncoding, table: Option<&TblTable>) -> EncodedLen {
    match encoding {
        TextEncoding::Ascii => match text.chars().enumerate().find(|(_, c)| !c.is_ascii()) {
            None => EncodedLen::Bytes(text.len()),
            Some((at, ch)) => EncodedLen::Unencodable(format!(
                "o caractere {ch:?} (posicao {}) nao existe em ASCII, que e como o texto desta \
                 string esta gravado — troque o caractere",
                at + 1
            )),
        },
        TextEncoding::Utf8 => EncodedLen::Bytes(text.len()),
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            EncodedLen::Bytes(text.encode_utf16().count() * 2)
        }
        TextEncoding::Table(id) => match table.map(|table| table.encode(text)) {
            Some(Ok(bytes)) => EncodedLen::Bytes(bytes.len()),
            Some(Err(e)) => EncodedLen::Unencodable(format!(
                "{e} — troque o caractere ou acrescente a linha dele em tables/{id}.tbl"
            )),
            None => EncodedLen::Unencodable(format!(
                "sem a tabela \"{id}\" nao da pra conferir a traducao"
            )),
        },
        TextEncoding::ShiftJis => match crate::sjis::encode(text) {
            Ok(bytes) => EncodedLen::Bytes(bytes.len()),
            Err(ch) => EncodedLen::Unencodable(format!(
                "o caractere {ch:?} nao existe em Shift-JIS, que e como o texto desta string \
                 esta gravado — troque o caractere (letra acentuada vira a letra simples)"
            )),
        },
        // Desenho: o tamanho e em pixels, conferido por `images::layout`.
        TextEncoding::Image => match crate::images::font::missing(text) {
            None => EncodedLen::Bytes(0),
            Some(ch) => EncodedLen::Unencodable(format!(
                "a fonte do app nao tem o caractere {ch:?} — troque o caractere"
            )),
        },
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

/// Letras de `text` (de qualquer escrita), sem contar as dos codigos.
fn letters(text: &str) -> usize {
    let mut rest = text.to_string();
    for token in extract_tokens(text) {
        rest = rest.replacen(&token, "", 1);
    }
    rest.chars().filter(|c| c.is_alphabetic()).count()
}

fn count_tokens(text: &str) -> HashMap<String, i32> {
    let mut map = HashMap::new();
    for token in extract_tokens(text) {
        *map.entry(token).or_insert(0) += 1;
    }
    map
}

/// Valida UMA entry traduzida. Entry sem traducao retorna vazio. `table` e a
/// tabela do projeto quando o texto e lido por uma (ver `encoded_len`).
pub fn validate_entry(entry: &TextEntry, table: Option<&TblTable>) -> Vec<ValidationIssue> {
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

    // Codigo do jogo conta como conteudo: "◆[FF23:0001]!" e o nome de alguem
    // com exclamacao (o original era "◆[FF23:0001]さん！").
    if letters(translation) == 0
        && extract_tokens(translation).is_empty()
        && letters(&entry.source_text) > 0
    {
        push(
            Severity::Error,
            IssueKind::NoLetters,
            format!(
                "a traducao {translation:?} nao tem nenhuma letra: se o original nao e texto, \
                 marque como nao-texto; senao, traduza de novo"
            ),
        );
        return issues;
    }

    if crate::pipeline::has_japanese(&entry.source_text)
        && crate::pipeline::has_japanese(translation)
    {
        push(
            Severity::Error,
            IssueKind::SourceScript,
            "a traducao ainda tem japones: traduza de novo".to_string(),
        );
        return issues;
    }

    // Texto desenhado: o que conta e caber no desenho com a fonte do app.
    if entry.encoding == TextEncoding::Image {
        let place = crate::images::place_of(entry);
        match place.map(|(_, block)| crate::images::layout(translation, &block)) {
            Some(Ok(_)) => {}
            Some(Err(why)) => {
                let kind = if crate::images::font::missing(translation).is_some() {
                    IssueKind::Unencodable
                } else {
                    IssueKind::ByteOverflow
                };
                push(Severity::Error, kind, why);
            }
            None => push(
                Severity::Error,
                IssueKind::Unencodable,
                "frase desenhada sem a posicao do desenho".to_string(),
            ),
        }
        return issues;
    }

    // Placeholders / control codes / tags (multiset dos dois lados). A quebra
    // de linha que o app tira e poe sozinho nao entra na conta.
    let layout = crate::adapters::bank::Layout::of(entry);
    let mut source_tokens = count_tokens(&entry.source_text);
    let mut translation_tokens = count_tokens(translation);
    if let Some(layout) = &layout {
        source_tokens.remove(layout.soft());
        translation_tokens.remove(layout.soft());
    }
    // Kanji da fonte do jogo que a tabela nao leu (`[045C]`) e texto: some na
    // traducao junto com o resto do japones.
    let unread = |token: &str| {
        entry.encoding == TextEncoding::Table(crate::kuten::TABLE_ID.to_string())
            && crate::kuten::is_glyph_token(token)
    };
    for (token, &n) in &source_tokens {
        let have = translation_tokens.get(token).copied().unwrap_or(0);
        if have < n && !unread(token) {
            push(
                Severity::Error,
                IssueKind::PlaceholderMismatch,
                format!("placeholder {token} removido da traducao"),
            );
        }
    }
    // Comando que desenha a letra que vem logo depois dele (`kuten`): sem
    // letra ali, o jogo le o proximo codigo como a letra.
    let lettered = entry
        .metadata
        .get("lettered")
        .and_then(|list| list.as_array())
        .into_iter()
        .flatten()
        .filter_map(|token| token.as_str());
    for token in lettered {
        let bare = translation.split(token).skip(1);
        if bare
            .into_iter()
            .any(|after| after.is_empty() || after.starts_with('['))
        {
            push(
                Severity::Error,
                IssueKind::PlaceholderMismatch,
                format!(
                    "o codigo {token} desenha a letra que vem logo depois dele: ponha uma \
                     letra (ou um espaco) depois dele"
                ),
            );
        }
    }
    // Em texto de tabela, so e codigo inventado o que a tabela grava como
    // codigo: o "<" que a IA escreve no lugar do "＜" do original e letra.
    let is_code = |token: &str| {
        !matches!(entry.encoding, TextEncoding::Table(_))
            || table.is_none_or(|table| table.has_entry(token))
    };
    for (token, &n) in &translation_tokens {
        let had = source_tokens.get(token).copied().unwrap_or(0);
        if n > had && is_code(token) {
            push(
                Severity::Error,
                IssueKind::PlaceholderMismatch,
                format!("placeholder {token} nao existe no original"),
            );
        }
    }

    // Bytes no encoding destino. A tabela so vale pra entry lida por tabela.
    let table = table.filter(|_| matches!(entry.encoding, TextEncoding::Table(_)));
    match encoded_len(translation, &entry.encoding, table) {
        EncodedLen::Unencodable(why) => push(Severity::Error, IssueKind::Unencodable, why),
        EncodedLen::Bytes(n) => {
            let original = entry.original_bytes.len();
            // Texto em tabela: a sobra do espaco tem que ser preenchivel — a
            // mesma conta que a reinsercao faz.
            if let Some(Err(e)) = table
                .map(|table| table.encode_slot_with(translation, original, fill(entry).as_deref()))
            {
                push(Severity::Error, IssueKind::ByteOverflow, e.to_string());
            }
            let relocatable = entry.can_grow();
            // Realocavel pode ter teto (PS1: a sobra do setor final do arquivo):
            // passar dele e erro; entre o original e o teto, aviso.
            match entry.max_bytes {
                Some(max) if n > max => push(
                    Severity::Error,
                    IssueKind::ByteOverflow,
                    format!("traducao ocupa {n} bytes; limite do campo e {max}"),
                ),
                // Mensagem de banco com a largura da caixa conhecida: crescer
                // e o esperado, e quem avisa e a conta de linha, mais abaixo.
                _ if n > original && crate::adapters::bank::line_width(entry).is_some() => {}
                // Realocar resolve o espaco na imagem, nao na tela: caixa de texto
                // ou buffer de RAM do jogo podem nao comportar o texto maior.
                _ if n > original && relocatable => push(
                    Severity::Warning,
                    IssueKind::ByteOverflow,
                    format!(
                        "traducao ocupa {n} bytes (original {original}): sera realocada com os ponteiros atualizados — confira no emulador se cabe na caixa de texto"
                    ),
                ),
                // Texto de tabela sem ponteiro em tabela so e gravado no lugar: a
                // reinsercao recusa o que passa do espaco, modo avancado
                // inclusive. E erro ja aqui, na frase.
                _ if n > original && table.is_some() => push(
                    Severity::Error,
                    IssueKind::ByteOverflow,
                    format!(
                        "traducao ocupa {n} bytes na tabela; o espaco original tem {original} e \
                         texto de tabela e gravado no lugar — encurte o texto"
                    ),
                ),
                None if n > original => push(
                    Severity::Warning,
                    IssueKind::ByteOverflow,
                    format!(
                        "traducao ocupa {n} bytes; o espaco original tem {original} (reinsercao fixa exigiria texto menor)"
                    ),
                ),
                _ => {}
            }
        }
    }

    // Mensagem que pode crescer ainda tem que caber na caixa de texto: cada
    // linha (trecho entre dois codigos) ate a largura medida no jogo. Onde o
    // app quebra as linhas, so sobra a palavra que sozinha nao cabe.
    if let Some(width) = crate::adapters::bank::line_width(entry) {
        let line = match &layout {
            Some(layout) => layout.wrap(translation).1,
            None => crate::adapters::bank::longest_line(translation),
        };
        if line > width && layout.is_some() {
            push(
                Severity::Warning,
                IssueKind::ByteOverflow,
                format!(
                    "uma palavra da traducao ocupa {line} caracteres; a caixa de texto do jogo \
                     mostra {width} por linha — ela vai passar da borda"
                ),
            );
        } else if line > width {
            // Sem a quebra do app, a linha e gravada como esta: passa da borda
            // da janela com certeza (16/10/2026: descricoes de inimigos com
            // linhas de 34 letras numa janela de 14).
            push(
                Severity::Error,
                IssueKind::ByteOverflow,
                format!(
                    "uma linha da traducao tem {line} caracteres; a caixa de texto do jogo \
                     mostra {width} por linha — quebre a frase de outro jeito ou encurte"
                ),
            );
        }
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

/// O aviso da troca de acento feita na traducao automatica
/// (`pipeline::save_translation`). Vale enquanto a traducao for a que foi
/// gravada la: editada a mao, o aviso some.
fn accent_swap_issue(
    entry: &TextEntry,
    swaps: &HashMap<String, (String, String)>,
) -> Option<ValidationIssue> {
    let (answer, written) = swaps.get(&entry.id)?;
    let why = match &entry.encoding {
        TextEncoding::Table(id) => format!(
            "a tabela do jogo nao tem esses caracteres — confira se o sentido se mantem; pra \
             te-los no jogo, acrescente a linha deles em tables/{id}.tbl"
        ),
        TextEncoding::Ascii => "o texto desta string e ASCII, sem letra acentuada nem aspas e \
                                travessao tipograficos — confira se o sentido se mantem"
            .to_string(),
        // UTF-8 e UTF-16 escrevem tudo: troca nenhuma vale pra eles.
        _ => return None,
    };
    (entry.translated_text.as_ref() == Some(written)).then(|| ValidationIssue {
        entry_id: entry.id.clone(),
        severity: Severity::Warning,
        kind: IssueKind::AccentDropped,
        message: format!(
            "a traducao veio como \"{answer}\" e foi gravada como \"{written}\": {why}"
        ),
    })
}

/// Valida todas as entries traduzidas do banco e ajusta statuses:
/// - com Error -> `Error`;
/// - `Error` antigo que ficou limpo -> volta para `Machine`;
/// - `Reviewed` limpo permanece `Reviewed`.
///
/// Tabela `.tbl` ausente do projeto e `Err`, nao issue por entry: sem ela
/// nada se confere, e marcar tudo como erro apagaria os `Reviewed`.
pub fn validate_project_db(db: &mut ProjectDb) -> Result<ValidationReport> {
    let entries = db.load_entries()?;
    let mut tables = ProjectTables::new(db.project_dir());
    let swaps = db.accent_swaps()?;
    let mut issues = Vec::new();
    let mut checked = 0;
    for entry in &entries {
        if entry.translated_text.is_none() {
            continue;
        }
        checked += 1;
        let mut entry_issues = validate_entry(entry, tables.for_entry(entry)?);
        entry_issues.extend(accent_swap_issue(entry, &swaps));
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
    let mut issues = validate_entry(
        &entry,
        ProjectTables::new(db.project_dir()).for_entry(&entry)?,
    );
    issues.extend(accent_swap_issue(&entry, &db.accent_swaps()?));
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
        let issues = validate_entry(
            &entry,
            ProjectTables::new(db.project_dir()).for_entry(&entry)?,
        );
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

/// Desfaz a marca de nome interno (`scan::looks_like_identifier`): a entry
/// volta a ser texto pendente e entra na proxima traducao.
pub fn unmark_internal(db: &mut ProjectDb, id: &str) -> Result<()> {
    match db.get_entry(id)?.map(|entry| entry.status) {
        Some(TranslationStatus::Internal) => db.set_status(id, TranslationStatus::Untranslated),
        Some(_) => Err(crate::error::CoreError::Project(format!(
            "a entry {id} nao esta marcada como nome interno"
        ))),
        None => Err(crate::error::CoreError::Project(format!(
            "entry {id} nao existe"
        ))),
    }
}

/// Marca como nao-texto (dado lido como texto) as frases `ids` e, com
/// `kind`, todas as que tem esse erro de validacao: saem da traducao e nao
/// sao gravadas (`ProjectDb::mark_stray`), e a marca fica mesmo extraindo de
/// novo. Devolve (marcadas, pedidas).
pub fn mark_not_text(
    db: &mut ProjectDb,
    ids: &[String],
    kind: Option<IssueKind>,
) -> Result<(usize, usize)> {
    let mut ids = ids.to_vec();
    if let Some(kind) = kind {
        ids.extend(
            validate_project_db(db)?
                .issues
                .into_iter()
                .filter(|i| i.kind == kind)
                .map(|i| i.entry_id),
        );
    }
    ids.sort();
    ids.dedup();
    Ok((db.mark_stray(&ids)?, ids.len()))
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
        let removed = validate_entry(&entry("HP {0}: 120", "PV: 120", TextEncoding::Utf8), None);
        assert!(kinds(&removed).contains(&IssueKind::PlaceholderMismatch));
        assert!(removed[0].message.contains("{0}"));

        let invented = validate_entry(&entry("Save game", "Salvar %s", TextEncoding::Utf8), None);
        assert!(kinds(&invented).contains(&IssueKind::PlaceholderMismatch));

        let ok = validate_entry(&entry("HP {0}: %d", "PV {0}: %d", TextEncoding::Utf8), None);
        assert!(ok.is_empty(), "{ok:?}");
    }

    #[test]
    fn byte_limits_and_encodings() {
        // max_bytes explicito estourado -> Error.
        let mut e = entry("SAVE", "SALVAR O JOGO AGORA", TextEncoding::Ascii);
        e.max_bytes = Some(6);
        let issues = validate_entry(&e, None);
        assert!(issues
            .iter()
            .any(|i| i.kind == IssueKind::ByteOverflow && i.severity == Severity::Error));

        // Sem max_bytes: acima do espaco original -> Warning.
        let long = validate_entry(&entry("HI", "OLA MUNDO", TextEncoding::Ascii), None);
        assert!(long
            .iter()
            .any(|i| i.kind == IssueKind::ByteOverflow && i.severity == Severity::Warning));

        // Nao-ASCII em encoding ASCII -> Unencodable, com o caractere e onde.
        let bad = validate_entry(&entry("POTION", "POÇÃO", TextEncoding::Ascii), None);
        assert!(kinds(&bad).contains(&IssueKind::Unencodable));
        assert!(
            bad[0].message.contains("'Ç'") && bad[0].message.contains("posicao 3"),
            "{bad:?}"
        );

        // UTF-16: 2 bytes por unidade.
        assert_eq!(
            encoded_len("ABC", &TextEncoding::Utf16Le, None),
            EncodedLen::Bytes(6)
        );
        // Tabela: mede pelos bytes dela (aqui "AB" vale um byte so).
        let table = TblTable::parse("80=A\n81=B\n82=C\n90=AB\n").unwrap();
        let enc = TextEncoding::Table("x".into());
        assert_eq!(encoded_len("ABC", &enc, Some(&table)), EncodedLen::Bytes(2));
        assert!(matches!(
            encoded_len("AB!", &enc, Some(&table)),
            EncodedLen::Unencodable(why) if why.contains("'!'")
        ));
        // Sem a tabela nao ha como conferir: erro, nunca check pulado.
        assert!(matches!(
            encoded_len("ABC", &enc, None),
            EncodedLen::Unencodable(_)
        ));
        assert_eq!(
            encoded_len("ABCあ", &TextEncoding::ShiftJis, None),
            EncodedLen::Bytes(5)
        );
        assert!(matches!(
            encoded_len("Olá", &TextEncoding::ShiftJis, None),
            EncodedLen::Unencodable(why) if why.contains("'á'")
        ));
    }

    #[test]
    fn a_command_that_draws_the_next_letter_needs_one() {
        let jis16 = TextEncoding::Table(crate::kuten::TABLE_ID.to_string());
        let check = |answer: &str| {
            let mut e = entry("[FF05:0003]Ａ[FF05:0000]ボタン", answer, jis16.clone());
            e.metadata = serde_json::json!({ "lettered": ["[FF05:0003]", "[FF05:0000]"] });
            validate_entry(&e, None)
                .iter()
                .any(|i| i.message.contains("desenha a letra"))
        };
        assert!(!check("botao [FF05:0003]A[FF05:0000] ok"));
        assert!(check("botao [FF05:0003]A[FF05:0000]"));
        assert!(check("botao [FF05:0003][FF05:0000] A"));
    }

    #[test]
    fn a_translation_without_letters_is_not_written() {
        let jis16 = TextEncoding::Table(crate::kuten::TABLE_ID.to_string());
        let check = |source: &str, answer: &str| {
            kinds(&validate_entry(&entry(source, answer, jis16.clone()), None))
        };
        for answer in ["?????", "...", "※"] {
            assert_eq!(
                check("Ｙこ÷り＊", answer),
                [IssueKind::NoLetters],
                "{answer}"
            );
        }
        // O codigo que escreve um nome e conteudo.
        assert!(!check("◆[FF23:0001]さん！", "◆[FF23:0001]!").contains(&IssueKind::NoLetters));
        // Pontuacao no original, pontuacao na traducao: nada a dizer.
        assert!(!check("・・・！", "...!").contains(&IssueKind::NoLetters));
        assert!(!check("ＰＳＩ", "PSI").contains(&IssueKind::NoLetters));
        // Japones que voltou japones nao e traducao.
        assert_eq!(
            check("◆あなたは えがおで", "◆あなたは えがおで"),
            [IssueKind::SourceScript]
        );
        assert!(!check("◆あなたは えがおで", "◆Voce sorri?").contains(&IssueKind::SourceScript));
    }

    #[test]
    fn table_text_over_its_space_is_an_error_unless_relocatable() {
        let table = TblTable::parse("80=A\n81=B\n40= \n").unwrap();
        let over = |e: &TextEntry| -> Vec<Severity> {
            validate_entry(e, Some(&table))
                .iter()
                .filter(|i| i.kind == IssueKind::ByteOverflow)
                .map(|i| i.severity)
                .collect()
        };
        let mut e = entry("AB", "ABA", TextEncoding::Table("x".into()));
        e.original_bytes = vec![0x80, 0x81];
        assert_eq!(over(&e), [Severity::Error]);
        let message = &validate_entry(&e, Some(&table))[0].message;
        assert!(message.contains("3 bytes") && message.contains("tem 2"));
        assert!(message.contains("encurte"), "{message}");

        // Do tamanho do espaco ou menor: nada a dizer.
        e.translated_text = Some("BA".into());
        assert!(validate_entry(&e, Some(&table)).is_empty());
        e.translated_text = Some("B".into());
        assert!(validate_entry(&e, Some(&table)).is_empty());

        // Com ponteiro em tabela a string pode ser realocada: aviso, como nos
        // outros encodings.
        e.translated_text = Some("ABA".into());
        e.metadata = serde_json::json!({"pointers": [16]});
        assert_eq!(over(&e), [Severity::Warning]);

        // A tabela de outra entry nao muda a severidade do texto ASCII.
        let ascii = entry("HI", "OLA MUNDO", TextEncoding::Ascii);
        assert_eq!(over(&ascii), [Severity::Warning]);
    }

    #[test]
    fn empty_and_anomalous_translations() {
        let empty = validate_entry(&entry("WELCOME!", "  ", TextEncoding::Utf8), None);
        assert_eq!(kinds(&empty), vec![IssueKind::EmptyTranslation]);

        let bloated = validate_entry(
            &entry(
                "MENU",
                "este e um texto absurdamente maior que o original",
                TextEncoding::Utf8,
            ),
            None,
        );
        assert!(kinds(&bloated).contains(&IssueKind::LengthAnomaly));

        let untranslated = TextEntry {
            translated_text: None,
            ..entry("X", "y", TextEncoding::Utf8)
        };
        assert!(validate_entry(&untranslated, None).is_empty());
    }
}

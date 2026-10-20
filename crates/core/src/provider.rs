//! Contrato de provider de traducao (spec §11) + prompt (§12) + parse tolerante
//! da resposta do modelo. Nunca enviamos bytes da ROM — so os textos extraidos.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
    /// So em texto de tabela de caracteres gravado no lugar: quantos
    /// caracteres a traducao pode ter pra caber, com certeza, no espaco da
    /// original (`TblTable::chars_that_fit`). Limite duro, e em caracteres
    /// porque e o que o modelo escreve — quanto cada um custa em bytes e
    /// coisa da tabela. Fora desse caso o campo nem aparece no pedido.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<usize>,
    /// So em mensagem que pode crescer e tem a largura da caixa de texto
    /// conhecida (`adapters::bank`): quantos caracteres cabem numa linha.
    /// Vai no item, e a regra vai uma vez so no prompt: sao milhares de
    /// mensagens por rodada.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_line: Option<usize>,
    /// So em frase do perfil do jogo com codigo de pagina (`profile`):
    /// quantas linhas cada pagina da caixa de texto pode ter, na ordem — as
    /// da mesma pagina do original. Medido em 19/10/2026 num jogo de DS: sem
    /// isto no pedido, 24 frases voltaram com pagina a mais.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<Vec<usize>>,
    /// O codigo que fecha a pagina (`[E5]`): vai uma vez so no prompt, na
    /// regra de `max_lines`, e nao no item.
    #[serde(skip)]
    pub page_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryTerm {
    pub term: String,
    pub translation: Option<String>,
    pub no_translate: bool,
    pub case_sensitive: bool,
    pub note: Option<String>,
}

/// O que o pedido pede ao provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Task {
    /// Traduzir o texto de cada item.
    #[default]
    Translate,
    /// Por os acentos que faltam numa traducao ja pronta, sem mudar mais
    /// nada: o texto de cada item ja esta no idioma de destino.
    Accent,
}

#[derive(Debug, Clone)]
pub struct TranslationRequest {
    pub items: Vec<BatchItem>,
    pub source_language: Option<String>,
    pub target_language: String,
    /// Ja filtrado: so termos que aparecem nos textos deste batch.
    pub glossary: Vec<GlossaryTerm>,
    /// `Some` quando o texto do batch e gravado por uma tabela de caracteres:
    /// os caracteres que ela escreve (`TblTable::charset`). O batch inteiro e
    /// de uma tabela so.
    pub charset: Option<String>,
    pub task: Task,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranslatedItem {
    pub id: String,
    #[serde(alias = "text")]
    pub translation: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranslationResponse {
    pub translations: Vec<TranslatedItem>,
}

/// Tokens que o provider disse ter cobrado, somados desde que foi criado.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[async_trait]
pub trait TranslationProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse>;
    async fn health_check(&self) -> Result<()>;

    /// O que as respostas declararam de consumo ate agora. Provider que nao
    /// recebe essa conta (Codex CLI) fica em zero.
    fn usage(&self) -> Usage {
        Usage::default()
    }

    /// Responde a `prompt` olhando a imagem `png` (a leitura do texto
    /// desenhado, `images`). Provider que nao ve imagem recusa.
    async fn read_image(&self, _png: &[u8], _prompt: &str) -> Result<String> {
        Err(CoreError::Provider(format!(
            "o provider {} nao le imagem — use o ChatGPT pelo Codex CLI",
            self.id()
        )))
    }
}

/// A resposta do provider pro item que nao e texto (dado lido como texto): o
/// prompt pede exatamente isto.
pub const NOT_TEXT: &str = "<NOT_TEXT>";

pub fn build_system_prompt(req: &TranslationRequest) -> String {
    if req.task == Task::Accent {
        return accent_prompt(req);
    }
    let mut p = String::from("You are translating text extracted from a video game.\n");
    p.push_str(&format!("Target language: {}.\n", req.target_language));
    if let Some(src) = &req.source_language {
        p.push_str(&format!("Source language: {src}.\n"));
    }
    p.push_str(
        "Rules:\n\
         - Preserve ALL placeholders, control codes, tags and escape sequences exactly as-is (e.g. {0}, %s, <BR>, \\n).\n\
         - Do not add explanations, notes or romanization.\n\
         - Keep tone consistent with game UI/dialogue.\n\
         - When an item has \"maxBytes\", prefer concise wording.\n\
         - If an item is not text (binary data read as text: random symbols, a list of single characters), answer exactly <NOT_TEXT>.\n",
    );
    if !req.glossary.is_empty() {
        p.push_str("Glossary (MUST be respected):\n");
        for g in &req.glossary {
            if g.no_translate {
                p.push_str(&format!("- \"{}\" -> DO NOT TRANSLATE\n", g.term));
            } else if let Some(t) = &g.translation {
                p.push_str(&format!("- \"{}\" -> \"{}\"\n", g.term, t));
            }
        }
    }
    if let Some(charset) = &req.charset {
        // Depois do glossario de proposito: a tabela vence o termo acentuado.
        p.push_str(
            "Character set (STRICT): the game font only has the characters on the next line",
        );
        p.push_str(if charset.contains(' ') {
            ", plus the space:\n"
        } else {
            " and has NO space character:\n"
        });
        p.extend(charset.chars().filter(|c| *c != ' '));
        p.push_str(
            "\n- Write every translation using ONLY those characters: anything else cannot be stored in the game.\n\
             - The only exception are placeholders and control codes copied from the source text.\n",
        );
        if !charset.chars().any(|c| !c.is_ascii() && c.is_alphabetic()) {
            p.push_str(
                "- There are no accented letters: write the plain letter (a for á, c for ç, E for É).\n",
            );
        } else {
            // Com a lista e o teto de caracteres, um modelo leve escreve sem
            // acento mesmo com a fonte tendo todos: medido em 19/10/2026 num
            // jogo de DS, 110 de 1.158 traducoes com acento.
            p.push_str(
                "- The font HAS the accented letters listed above: write every word with the accents and diacritics the target language requires (for example ã, é, ç in Portuguese). A missing accent is a spelling mistake. An accented letter counts as one character, the same as a plain one.\n",
            );
        }
        p.push_str(
            "- If a punctuation mark or a digit is not listed, rephrase without it.\n\
             - This also applies to glossary terms: adapt them to the character set.\n",
        );
        // Marca de fala e enfeite que a fonte do jogo tem: sem isto o modelo
        // os tira da traducao junto com o japones.
        if charset.chars().any(|c| "◆◇☆★○●◎□■△▲▽▼※♪".contains(c)) {
            p.push_str(
                "- The symbols in that list that are not letters, digits or punctuation (such as ◆, ◇, ★, ♪) are marks the game shows: keep each one of the source text exactly where it is.\n",
            );
        }
    }
    if req.items.iter().any(|item| item.max_chars.is_some()) {
        p.push_str(
            "Length limit (STRICT): each text has a fixed space in the game.\n\
             - \"maxChars\" is the maximum number of characters of that item's translation, counting spaces and the placeholders and control codes copied from the source: a longer translation cannot be stored in the game.\n\
             - If the natural translation is longer, use shorter words or drop words until it fits. Shorter is always fine.\n",
        );
    }
    if req.items.iter().any(|item| item.max_line.is_some()) {
        p.push_str(
            "Line width (STRICT): some items have \"maxLine\", the number of characters the game's text box shows per line.\n\
             - In those items a line is the text between two codes in square brackets (and before the first and after the last one).\n\
             - Write every line of the translation with at most maxLine characters, counting spaces. The translation as a whole may be longer than the source.\n\
             - Keep every code in square brackets exactly as it is and in the same order: you cannot add, remove or move line breaks.\n",
        );
    }
    let pages: std::collections::BTreeSet<&str> = req
        .items
        .iter()
        .filter(|item| item.max_lines.is_some())
        .filter_map(|item| item.page_code.as_deref())
        .collect();
    if !pages.is_empty() {
        let codes = pages.into_iter().collect::<Vec<_>>().join(" or ");
        p.push_str(&format!(
            "Pages (STRICT): items with \"maxLines\" are shown in the game's text box one page at a time; a page ends at the code {codes}.\n\
             - \"maxLines\" lists, in order, how many lines each page of that item can have. The app breaks every line longer than \"maxLine\" at a space.\n\
             - Write each page so that, broken that way, it fits in its number of lines; if it does not, say it with fewer words. Keep every {codes} where it is.\n",
        ));
    }
    p.push_str(
        "Respond with ONLY valid JSON in this exact shape, one entry per input item, same ids:\n\
         {\"translations\":[{\"id\":\"...\",\"translation\":\"...\"}]}\n",
    );
    p
}

/// O pedido de acentuacao: o texto ja traduzido volta com os acentos, e nada
/// mais muda (quem confere e `pipeline::run_accent_pass`).
fn accent_prompt(req: &TranslationRequest) -> String {
    let mut p = format!(
        "You are fixing text from a video game translated into {lang}.\n\
         The text was written WITHOUT accents because the game font did not have accented \
         letters. Now it has them.\n\
         Rules:\n\
         - Return each text with every accent, cedilla and diacritic that correct {lang} \
         spelling requires (in Portuguese, for example: voce -> você, nao -> não, acao -> ação, \
         esta -> está when it is the verb).\n\
         - Change NOTHING else: the same words in the same order, the same spaces, the same \
         punctuation and the same capitalization. Do not translate, rephrase, fix grammar or \
         shorten.\n\
         - Copy every code in square brackets exactly as it is, in the same place.\n\
         - If a text needs no accent, return it unchanged.\n",
        lang = req.target_language
    );
    if let Some(charset) = &req.charset {
        p.push_str("- The game font has only these accented letters; use no other: ");
        p.push_str(charset);
        p.push('\n');
    }
    p.push_str(
        "Respond with ONLY valid JSON in this exact shape, one entry per input item, same ids:\n\
         {\"translations\":[{\"id\":\"...\",\"translation\":\"...\"}]}\n",
    );
    p
}

pub fn build_user_prompt(req: &TranslationRequest) -> String {
    // Serializacao direta dos items, sem espaco nem campo vazio: sao milhares
    // de frases por rodada, e cada caractere a mais conta em todas.
    serde_json::to_string(&req.items).unwrap_or_else(|_| "[]".to_string())
}

/// Parse tolerante: aceita `{"translations":[...]}` ou `[...]` direto, com ou
/// sem cercas ```json e com ou sem blocos <think> (modelos de raciocinio).
pub fn parse_model_response(raw: &str) -> Result<TranslationResponse> {
    let cleaned = strip_think_blocks(raw);

    if let Ok(resp) = serde_json::from_str::<TranslationResponse>(&cleaned) {
        return Ok(resp);
    }
    if let Ok(translations) = serde_json::from_str::<Vec<TranslatedItem>>(&cleaned) {
        return Ok(TranslationResponse { translations });
    }
    // Recorta o primeiro objeto/array plausivel no meio de texto/cercas.
    if let Some(json) = slice_between(&cleaned, '{', '}') {
        if let Ok(resp) = serde_json::from_str::<TranslationResponse>(json) {
            return Ok(resp);
        }
    }
    if let Some(json) = slice_between(&cleaned, '[', ']') {
        if let Ok(translations) = serde_json::from_str::<Vec<TranslatedItem>>(json) {
            return Ok(TranslationResponse { translations });
        }
    }
    Err(CoreError::Provider(format!(
        "resposta do modelo nao e o JSON esperado (primeiros 200 chars): {}",
        cleaned.chars().take(200).collect::<String>()
    )))
}

fn strip_think_blocks(s: &str) -> String {
    let mut out = s.to_string();
    while let (Some(a), Some(b)) = (out.find("<think>"), out.find("</think>")) {
        if b < a {
            break;
        }
        out.replace_range(a..b + "</think>".len(), "");
    }
    out.trim().to_string()
}

fn slice_between(s: &str, open: char, close: char) -> Option<&str> {
    let start = s.find(open)?;
    let end = s.rfind(close)?;
    (end > start).then(|| &s[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req_with_glossary() -> TranslationRequest {
        TranslationRequest {
            items: vec![BatchItem {
                id: "a".into(),
                text: "Use the POTION, {0}!".into(),
                context: None,
                max_bytes: Some(24),
                max_chars: None,
                max_line: None,
                max_lines: None,
                page_code: None,
            }],
            source_language: Some("en-US".into()),
            target_language: "pt-BR".into(),
            glossary: vec![
                GlossaryTerm {
                    term: "POTION".into(),
                    translation: Some("POÇÃO".into()),
                    no_translate: false,
                    case_sensitive: true,
                    note: None,
                },
                GlossaryTerm {
                    term: "HP".into(),
                    translation: None,
                    no_translate: true,
                    case_sensitive: true,
                    note: None,
                },
            ],
            charset: None,
            task: Default::default(),
        }
    }

    #[test]
    fn system_prompt_carries_rules_and_glossary() {
        let p = build_system_prompt(&req_with_glossary());
        assert!(p.contains("pt-BR"));
        assert!(p.contains("Preserve ALL placeholders"));
        assert!(p.contains("\"POTION\" -> \"POÇÃO\""));
        assert!(p.contains("\"HP\" -> DO NOT TRANSLATE"));
        assert!(p.contains("{\"translations\""));
    }

    /// O prompt de batch sem tabela, letra por letra: e o que os providers
    /// recebiam antes de existir a lista de caracteres.
    const PLAIN_PROMPT: &str = "You are translating text extracted from a video game.\n\
        Target language: pt-BR.\n\
        Source language: en-US.\n\
        Rules:\n\
        - Preserve ALL placeholders, control codes, tags and escape sequences exactly as-is (e.g. {0}, %s, <BR>, \\n).\n\
        - Do not add explanations, notes or romanization.\n\
        - Keep tone consistent with game UI/dialogue.\n\
        - When an item has \"maxBytes\", prefer concise wording.\n\
        - If an item is not text (binary data read as text: random symbols, a list of single characters), answer exactly <NOT_TEXT>.\n\
        Glossary (MUST be respected):\n\
        - \"POTION\" -> \"POÇÃO\"\n\
        - \"HP\" -> DO NOT TRANSLATE\n\
        Respond with ONLY valid JSON in this exact shape, one entry per input item, same ids:\n\
        {\"translations\":[{\"id\":\"...\",\"translation\":\"...\"}]}\n";

    #[test]
    fn prompt_without_a_table_is_unchanged() {
        assert_eq!(build_system_prompt(&req_with_glossary()), PLAIN_PROMPT);
    }

    #[test]
    fn table_batch_lists_the_characters_and_forbids_the_rest() {
        let plain = req_with_glossary();
        let letters = TranslationRequest {
            charset: Some(" ABCabc".into()),
            task: Default::default(),
            ..plain.clone()
        };
        let p = build_system_prompt(&letters);
        assert!(p.contains("\nABCabc\n"), "{p}");
        assert!(p.contains("plus the space"), "{p}");
        assert!(p.contains("ONLY those characters"), "{p}");
        assert!(p.contains("no accented letters"), "{p}");
        // O bloco e a unica diferenca pro prompt de sempre, e vem depois do
        // glossario.
        let block = &p[p.find("Character set").unwrap()..p.find("Respond with").unwrap()];
        assert_eq!(p.replace(block, ""), PLAIN_PROMPT);
        assert!(p.find("Glossary").unwrap() < p.find("Character set").unwrap());

        // Tabela com acento e sem espaco: o prompt nao manda tirar acento e
        // avisa que nao ha espaco.
        let accented = TranslationRequest {
            charset: Some("ABCÇçé!".into()),
            task: Default::default(),
            ..plain
        };
        let p = build_system_prompt(&accented);
        assert!(p.contains("\nABCÇçé!\n"), "{p}");
        assert!(p.contains("NO space character"), "{p}");
        assert!(!p.contains("no accented letters"), "{p}");
    }

    #[test]
    fn table_item_with_a_limit_gets_the_hard_rule() {
        let mut limited = TranslationRequest {
            charset: Some(" ABCabc".into()),
            task: Default::default(),
            ..req_with_glossary()
        };
        let unlimited = build_system_prompt(&limited);
        assert!(!unlimited.contains("maxChars"), "{unlimited}");

        limited.items[0].max_bytes = None;
        limited.items[0].max_chars = Some(18);
        let p = build_system_prompt(&limited);
        assert!(p.contains("Length limit (STRICT)"), "{p}");
        assert!(p.contains("\"maxChars\" is the maximum number of characters"));
        // O bloco e a unica diferenca, e vem depois da lista de caracteres.
        let block = &p[p.find("Length limit").unwrap()..p.find("Respond with").unwrap()];
        assert_eq!(p.replace(block, ""), unlimited);
        assert!(p.find("Character set").unwrap() < p.find("Length limit").unwrap());
        assert!(build_user_prompt(&limited).contains("\"maxChars\":18"));
    }

    #[test]
    fn user_prompt_is_item_json() {
        // Item sem limite em caracteres: o pedido e o de sempre, letra por
        // letra.
        assert_eq!(
            build_user_prompt(&req_with_glossary()),
            r#"[{"id":"a","text":"Use the POTION, {0}!","maxBytes":24}]"#
        );
    }

    #[test]
    fn parse_accepts_object_array_fences_and_think() {
        let object = r#"{"translations":[{"id":"a","translation":"Oi"}]}"#;
        let array = r#"[{"id":"a","translation":"Oi"}]"#;
        let fenced = format!("```json\n{object}\n```");
        let thought = format!("<think>hmm reasoning</think>\n{object}");
        let text_alias = r#"{"translations":[{"id":"a","text":"Oi"}]}"#;
        for raw in [object, array, fenced.as_str(), thought.as_str(), text_alias] {
            let resp = parse_model_response(raw).unwrap();
            assert_eq!(resp.translations[0].translation, "Oi", "raw: {raw}");
        }
    }

    #[test]
    fn parse_rejects_garbage_with_context() {
        let err = parse_model_response("desculpa, nao consigo").unwrap_err();
        assert!(err.to_string().contains("JSON"));
    }
}

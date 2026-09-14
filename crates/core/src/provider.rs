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
    pub context: Option<String>,
    pub max_bytes: Option<usize>,
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

#[derive(Debug, Clone)]
pub struct TranslationRequest {
    pub items: Vec<BatchItem>,
    pub source_language: Option<String>,
    pub target_language: String,
    /// Ja filtrado: so termos que aparecem nos textos deste batch.
    pub glossary: Vec<GlossaryTerm>,
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

#[async_trait]
pub trait TranslationProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse>;
    async fn health_check(&self) -> Result<()>;
}

pub fn build_system_prompt(req: &TranslationRequest) -> String {
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
         - When an item has \"maxBytes\", prefer concise wording.\n",
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
    p.push_str(
        "Respond with ONLY valid JSON in this exact shape, one entry per input item, same ids:\n\
         {\"translations\":[{\"id\":\"...\",\"translation\":\"...\"}]}\n",
    );
    p
}

pub fn build_user_prompt(req: &TranslationRequest) -> String {
    // So id/text/context/maxBytes — serializacao direta dos items.
    serde_json::to_string_pretty(&req.items).unwrap_or_else(|_| "[]".to_string())
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

    #[test]
    fn user_prompt_is_item_json() {
        let p = build_user_prompt(&req_with_glossary());
        assert!(p.contains("\"id\": \"a\""));
        assert!(p.contains("maxBytes"));
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

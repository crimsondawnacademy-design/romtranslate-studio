//! Provider Claude pela API da Anthropic, com a chave de API da pessoa: e o
//! caminho que a Anthropic indica pra app de terceiros (o login da assinatura
//! e reservado ao Claude Code e aos apps dela). HTTP direto — nao ha SDK
//! oficial pra Rust. Formato conferido na referencia da API em 12/10/2026:
//!
//! - `POST /v1/messages` com `x-api-key` e `anthropic-version: 2023-06-01`;
//! - a resposta e uma lista de blocos: o texto esta nos de tipo `text` (os de
//!   `thinking` vem antes e nao sao a resposta);
//! - `stop_reason` precisa ser lido ANTES do conteudo: `refusal` (HTTP 200,
//!   conteudo possivelmente vazio) e `max_tokens` nao sao resposta inteira;
//! - `usage.input_tokens` / `output_tokens` sao o que foi cobrado;
//! - `POST /v1/messages/count_tokens` conta a entrada sem cobrar;
//! - `GET /v1/models/{id}` confere chave e modelo sem gastar token.
//!
//! O pedido nao manda `thinking`, `effort`, `temperature` nem prefill: cada um
//! deles e recusado com 400 em alguma geracao de modelo, e o modelo aqui e
//! escolha do usuario.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{error_for_status, http_client, UsageMeter};
use crate::error::{CoreError, Result};
use crate::provider::{
    build_system_prompt, build_user_prompt, parse_model_response, TranslationProvider,
    TranslationRequest, TranslationResponse, Usage,
};

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
const API_VERSION: &str = "2023-06-01";
/// Teto de saida de um lote. Folgado de proposito: nos modelos que raciocinam
/// antes de responder, o raciocinio tambem sai deste teto.
const MAX_TOKENS: u32 = 16_000;

/// Modelos em que uma recusa do classificador de seguranca pode ser refeita
/// num modelo substituto pelo proprio servidor (`fallbacks: "default"`). Fora
/// desta lista o parametro e recusado ou nao faz nada.
const SERVER_FALLBACK: [&str; 4] = [
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
    "claude-fable-5-1",
];
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    meter: UsageMeter,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    content: Vec<Block>,
    stop_reason: Option<String>,
    stop_details: Option<Value>,
    #[serde(default)]
    usage: Billed,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize, Default)]
struct Billed {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Deserialize)]
struct TokenCount {
    input_tokens: u64,
}

impl AnthropicProvider {
    pub fn new(
        base_url: &str,
        model: &str,
        api_key: Option<String>,
        timeout_secs: u64,
    ) -> Result<Self> {
        let model = match model.trim() {
            "" => DEFAULT_MODEL,
            given => given,
        };
        let api_key = api_key
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| {
                CoreError::Provider(
                    "informe a chave de API do Claude (console da Anthropic) e salve".to_string(),
                )
            })?;
        let base_url = match base_url.trim().trim_end_matches('/') {
            "" => DEFAULT_BASE_URL,
            given => given,
        };
        Ok(AnthropicProvider {
            client: http_client(timeout_secs)?,
            base_url: base_url.to_string(),
            model: model.to_string(),
            api_key,
            meter: UsageMeter::default(),
        })
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}{path}", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
    }

    /// `system` + uma mensagem de usuario com os itens do lote.
    fn prompt(&self, request: &TranslationRequest) -> Value {
        json!({
            "model": self.model,
            "system": build_system_prompt(request),
            "messages": [{"role": "user", "content": build_user_prompt(request)}],
        })
    }

    fn with_fallback(&self) -> bool {
        SERVER_FALLBACK.contains(&self.model.as_str())
    }

    /// Corpo de `POST /v1/messages` pra um lote.
    pub fn request_body(&self, request: &TranslationRequest) -> Value {
        let mut body = self.prompt(request);
        body["max_tokens"] = json!(MAX_TOKENS);
        if self.with_fallback() {
            body["fallbacks"] = json!("default");
        }
        body
    }

    /// Quantos tokens de entrada este lote custa, pela conta do proprio
    /// modelo. Nao cobra nada.
    pub async fn count_input_tokens(&self, request: &TranslationRequest) -> Result<u64> {
        let resp = self
            .post("/v1/messages/count_tokens")
            .json(&self.prompt(request))
            .send()
            .await
            .map_err(|e| CoreError::Provider(format!("API da Anthropic inacessivel: {e}")))?;
        if !resp.status().is_success() {
            return Err(error_for_status(resp, "POST /v1/messages/count_tokens").await);
        }
        let count: TokenCount = resp
            .json()
            .await
            .map_err(|e| CoreError::Provider(format!("contagem de tokens invalida: {e}")))?;
        Ok(count.input_tokens)
    }
}

/// Texto da resposta e o que ela cobrou. `stop_reason` e lido antes do
/// conteudo: recusa e resposta cortada chegam com HTTP 200.
fn read_message(raw: &str) -> Result<(String, Usage)> {
    let message: Message = serde_json::from_str(raw)
        .map_err(|e| CoreError::Provider(format!("resposta invalida da API da Anthropic: {e}")))?;
    let usage = Usage {
        input_tokens: message.usage.input_tokens,
        output_tokens: message.usage.output_tokens,
    };
    match message.stop_reason.as_deref() {
        Some("refusal") => {
            let category = message
                .stop_details
                .as_ref()
                .and_then(|d| d.get("category"))
                .and_then(Value::as_str)
                .unwrap_or("sem categoria informada");
            return Err(CoreError::Provider(format!(
                "o modelo recusou este lote por politica de seguranca ({category}); as frases \
                 dele continuam pendentes"
            )));
        }
        Some("max_tokens") => {
            return Err(CoreError::Provider(
                "a resposta foi cortada no limite de saida do modelo — diminua as frases por \
                 chamada"
                    .to_string(),
            ));
        }
        _ => {}
    }
    let text: String = message
        .content
        .iter()
        .filter(|b| b.kind == "text")
        .map(|b| b.text.as_str())
        .collect();
    if text.trim().is_empty() {
        return Err(CoreError::Provider(
            "a API da Anthropic respondeu sem texto".to_string(),
        ));
    }
    Ok((text, usage))
}

#[async_trait]
impl TranslationProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse> {
        let mut call = self.post("/v1/messages");
        if self.with_fallback() {
            call = call.header("anthropic-beta", FALLBACK_BETA);
        }
        let resp = call
            .json(&self.request_body(request))
            .send()
            .await
            .map_err(|e| CoreError::Provider(format!("API da Anthropic inacessivel: {e}")))?;
        if !resp.status().is_success() {
            return Err(error_for_status(resp, "POST /v1/messages").await);
        }
        let raw = resp
            .text()
            .await
            .map_err(|e| CoreError::Provider(format!("resposta ilegivel da API: {e}")))?;
        // O que foi cobrado conta mesmo quando a resposta nao serve.
        if let Ok(billed) = serde_json::from_str::<Message>(&raw) {
            self.meter
                .add(billed.usage.input_tokens, billed.usage.output_tokens);
        }
        let (text, _) = read_message(&raw)?;
        parse_model_response(&text)
    }

    async fn health_check(&self) -> Result<()> {
        let resp = self
            .client
            .get(format!("{}/v1/models/{}", self.base_url, self.model))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .send()
            .await
            .map_err(|e| CoreError::Provider(format!("API da Anthropic inacessivel: {e}")))?;
        if !resp.status().is_success() {
            return Err(error_for_status(resp, "GET /v1/models").await);
        }
        Ok(())
    }

    fn usage(&self) -> Usage {
        self.meter.total()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::BatchItem;

    fn request() -> TranslationRequest {
        TranslationRequest {
            items: vec![BatchItem {
                id: "a".to_string(),
                text: "NEW GAME".to_string(),
                context: None,
                max_bytes: Some(12),
                max_chars: None,
                max_line: None,
                max_lines: None,
                page_code: None,
            }],
            source_language: None,
            target_language: "pt-BR".to_string(),
            glossary: Vec::new(),
            charset: None,
        }
    }

    #[test]
    fn request_carries_the_batch_and_nothing_a_model_might_reject() {
        let provider = AnthropicProvider::new("", "", Some("sk-test".into()), 30).unwrap();
        let body = provider.request_body(&request());
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["max_tokens"], MAX_TOKENS);
        assert_eq!(body["fallbacks"], "default");
        assert!(body["system"].as_str().unwrap().contains("pt-BR"));
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("NEW GAME"));
        // Uma mensagem so, de usuario: sem prefill de assistente.
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        for rejected in ["thinking", "temperature", "top_p", "output_config"] {
            assert!(body.get(rejected).is_none(), "{rejected}");
        }

        // Modelo fora da lista nao leva o parametro de fallback.
        let other = AnthropicProvider::new("", "claude-haiku-5-5", Some("k".into()), 30).unwrap();
        assert!(other.request_body(&request()).get("fallbacks").is_none());
    }

    #[test]
    fn key_is_required_and_blank_fields_fall_back_to_defaults() {
        assert!(AnthropicProvider::new("", "", None, 30).is_err());
        assert!(AnthropicProvider::new("", "", Some("  ".into()), 30).is_err());
        let p =
            AnthropicProvider::new(" https://proxy.local/ ", " m ", Some("k".into()), 30).unwrap();
        assert_eq!(p.base_url, "https://proxy.local");
        assert_eq!(p.model, "m");
    }

    #[test]
    fn answer_is_the_text_blocks_and_the_bill_is_read() {
        let raw = r#"{"content":[
            {"type":"thinking","thinking":"","signature":"x"},
            {"type":"text","text":"{\"translations\":[{\"id\":\"a\","},
            {"type":"text","text":"\"translation\":\"NOVO JOGO\"}]}"}],
            "stop_reason":"end_turn","usage":{"input_tokens":120,"output_tokens":45}}"#;
        let (text, usage) = read_message(raw).unwrap();
        assert_eq!(
            parse_model_response(&text).unwrap().translations[0].translation,
            "NOVO JOGO"
        );
        assert_eq!(
            usage,
            Usage {
                input_tokens: 120,
                output_tokens: 45
            }
        );
    }

    #[test]
    fn refusals_and_cut_answers_are_errors_not_translations() {
        let refusal = r#"{"content":[],"stop_reason":"refusal",
            "stop_details":{"type":"refusal","category":"cyber"},
            "usage":{"input_tokens":10,"output_tokens":0}}"#;
        let err = read_message(refusal).unwrap_err().to_string();
        assert!(err.contains("recusou") && err.contains("cyber"), "{err}");
        // Categoria nula e um estado valido da API.
        let bare = r#"{"content":[],"stop_reason":"refusal","stop_details":null}"#;
        assert!(read_message(bare)
            .unwrap_err()
            .to_string()
            .contains("recusou"));

        let cut = r#"{"content":[{"type":"text","text":"{\"translations\":[{"}],
            "stop_reason":"max_tokens","usage":{"input_tokens":10,"output_tokens":16000}}"#;
        assert!(read_message(cut)
            .unwrap_err()
            .to_string()
            .contains("cortada"));

        let empty = r#"{"content":[{"type":"thinking","thinking":""}],"stop_reason":"end_turn"}"#;
        assert!(read_message(empty).is_err());
        assert!(read_message("nao e json").is_err());
    }
}

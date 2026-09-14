//! Provider Ollama (local): POST /api/chat com format=json, stream desligado.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{error_for_status, http_client};
use crate::error::{CoreError, Result};
use crate::provider::{
    build_system_prompt, build_user_prompt, parse_model_response, TranslationProvider,
    TranslationRequest, TranslationResponse,
};

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str, model: &str, timeout_secs: u64) -> Result<Self> {
        if model.trim().is_empty() {
            return Err(CoreError::Provider(
                "configure um modelo do Ollama (ex.: llama3.2:3b)".to_string(),
            ));
        }
        Ok(OllamaProvider {
            client: http_client(timeout_secs)?,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
        })
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

#[async_trait]
impl TranslationProvider for OllamaProvider {
    fn id(&self) -> &'static str {
        "ollama"
    }

    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "format": "json",
            "options": {"temperature": 0.2},
            "messages": [
                {"role": "system", "content": build_system_prompt(request)},
                {"role": "user", "content": build_user_prompt(request)},
            ],
        });
        let resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| CoreError::Provider(format!("ollama inacessivel: {e}")))?;
        if !resp.status().is_success() {
            return Err(error_for_status(resp, "ollama /api/chat").await);
        }
        let chat: ChatResponse = resp
            .json()
            .await
            .map_err(|e| CoreError::Provider(format!("resposta invalida do ollama: {e}")))?;
        parse_model_response(&chat.message.content)
    }

    async fn health_check(&self) -> Result<()> {
        let resp = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .map_err(|e| CoreError::Provider(format!("ollama inacessivel: {e}")))?;
        if !resp.status().is_success() {
            return Err(error_for_status(resp, "ollama /api/tags").await);
        }
        Ok(())
    }
}

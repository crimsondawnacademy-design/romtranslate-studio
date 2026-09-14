//! Provider OpenAI-compatible: POST /chat/completions (OpenAI, OpenRouter,
//! LM Studio, vLLM etc.). Nao hardcoda modelo; api key opcional (local nao usa).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{error_for_status, http_client};
use crate::error::{CoreError, Result};
use crate::provider::{
    build_system_prompt, build_user_prompt, parse_model_response, TranslationProvider,
    TranslationRequest, TranslationResponse,
};

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiCompatProvider {
    pub fn new(
        base_url: &str,
        model: &str,
        api_key: Option<String>,
        timeout_secs: u64,
    ) -> Result<Self> {
        if model.trim().is_empty() {
            return Err(CoreError::Provider(
                "configure o modelo do endpoint OpenAI-compatible".to_string(),
            ));
        }
        Ok(OpenAiCompatProvider {
            client: http_client(timeout_secs)?,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: api_key.filter(|k| !k.trim().is_empty()),
        })
    }

    fn request(&self, path: &str) -> reqwest::RequestBuilder {
        let mut builder = self.client.get(format!("{}{path}", self.base_url));
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        builder
    }
}

#[derive(Deserialize)]
struct ChatCompletion {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: String,
}

#[async_trait]
impl TranslationProvider for OpenAiCompatProvider {
    fn id(&self) -> &'static str {
        "openai_compatible"
    }

    async fn translate_batch(&self, request: &TranslationRequest) -> Result<TranslationResponse> {
        let body = json!({
            "model": self.model,
            "temperature": 0.2,
            "messages": [
                {"role": "system", "content": build_system_prompt(request)},
                {"role": "user", "content": build_user_prompt(request)},
            ],
        });
        let mut builder = self
            .client
            .post(format!("{}/chat/completions", self.base_url));
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        let resp = builder
            .json(&body)
            .send()
            .await
            .map_err(|e| CoreError::Provider(format!("endpoint inacessivel: {e}")))?;
        if !resp.status().is_success() {
            return Err(error_for_status(resp, "chat/completions").await);
        }
        let completion: ChatCompletion = resp
            .json()
            .await
            .map_err(|e| CoreError::Provider(format!("resposta invalida do endpoint: {e}")))?;
        let content = completion
            .choices
            .first()
            .map(|c| c.message.content.as_str())
            .ok_or_else(|| CoreError::Provider("resposta sem choices".to_string()))?;
        parse_model_response(content)
    }

    async fn health_check(&self) -> Result<()> {
        let resp = self
            .request("/models")
            .send()
            .await
            .map_err(|e| CoreError::Provider(format!("endpoint inacessivel: {e}")))?;
        if !resp.status().is_success() {
            return Err(error_for_status(resp, "GET /models").await);
        }
        Ok(())
    }
}

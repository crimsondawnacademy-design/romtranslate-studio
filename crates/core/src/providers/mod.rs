pub mod ollama;
pub mod openai_compat;

use std::time::Duration;

use crate::error::{CoreError, Result};

pub(crate) fn http_client(timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(5)))
        .build()
        .map_err(|e| CoreError::Provider(format!("falha criando cliente HTTP: {e}")))
}

/// Erro legivel para respostas nao-2xx, com um trecho curto do corpo.
pub(crate) async fn error_for_status(resp: reqwest::Response, what: &str) -> CoreError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let snippet: String = body.chars().take(300).collect();
    CoreError::Provider(format!("{what}: HTTP {status} — {snippet}"))
}

pub mod anthropic;
pub mod codex_cli;
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

/// Contador do consumo que as respostas de um provider declaram.
#[derive(Default)]
pub(crate) struct UsageMeter {
    input: std::sync::atomic::AtomicU64,
    output: std::sync::atomic::AtomicU64,
}

impl UsageMeter {
    pub(crate) fn add(&self, input: u64, output: u64) {
        use std::sync::atomic::Ordering::Relaxed;
        self.input.fetch_add(input, Relaxed);
        self.output.fetch_add(output, Relaxed);
    }

    pub(crate) fn total(&self) -> crate::provider::Usage {
        use std::sync::atomic::Ordering::Relaxed;
        crate::provider::Usage {
            input_tokens: self.input.load(Relaxed),
            output_tokens: self.output.load(Relaxed),
        }
    }
}

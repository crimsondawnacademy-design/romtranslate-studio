//! Pipeline de traducao (spec §25 Sprint 3): TM lookup -> batches no provider,
//! com retry/backoff, cancelamento e progresso. Sequencial por enquanto.
// ponytail: batches sequenciais; concorrencia limitada entra se medir que uma
// API remota vira gargalo (Ollama local nao paraleliza de verdade).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use tracing::{info, warn};

use crate::db::ProjectDb;
use crate::error::Result;
use crate::provider::{BatchItem, GlossaryTerm, TranslationProvider, TranslationRequest};
use crate::types::{TextEntry, TranslationStatus};

#[derive(Debug, Clone)]
pub struct TranslateOptions {
    pub source_language: Option<String>,
    pub target_language: String,
    pub batch_size: usize,
    /// Tentativas por batch (1 = sem retry).
    pub max_attempts: u32,
    pub retry_delay_ms: u64,
}

impl TranslateOptions {
    pub fn new(target_language: impl Into<String>) -> Self {
        TranslateOptions {
            source_language: None,
            target_language: target_language.into(),
            batch_size: 10,
            max_attempts: 3,
            retry_delay_ms: 1000,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateSummary {
    pub translated: usize,
    pub tm_hits: usize,
    pub failed: usize,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub phase: &'static str,
}

/// Traduz todas as entries ainda nao traduzidas do banco do projeto.
/// `cancel` interrompe entre batches; progresso via callback.
pub async fn run_translation(
    db: &mut ProjectDb,
    provider: &dyn TranslationProvider,
    model_label: &str,
    opts: &TranslateOptions,
    cancel: &AtomicBool,
    on_progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<TranslateSummary> {
    let started_at = Utc::now().to_rfc3339();
    let src_lang = opts.source_language.clone().unwrap_or_default();

    let pending: Vec<TextEntry> = db
        .load_entries()?
        .into_iter()
        .filter(|e| e.translated_text.is_none())
        .collect();
    let total = pending.len();
    let mut summary = TranslateSummary {
        translated: 0,
        tm_hits: 0,
        failed: 0,
        cancelled: false,
    };
    if total == 0 {
        return Ok(summary);
    }

    // Fase 1 — translation memory.
    let mut remaining: Vec<TextEntry> = Vec::new();
    for entry in pending {
        if let Some(hit) = db.tm_lookup(&entry.source_text, &src_lang, &opts.target_language)? {
            db.update_translation(&entry.id, &hit, TranslationStatus::Machine)?;
            summary.tm_hits += 1;
        } else {
            remaining.push(entry);
        }
    }
    on_progress(Progress {
        done: summary.tm_hits,
        total,
        phase: "tm",
    });

    // Fase 2 — provider, em batches sequenciais com retry.
    let glossary = db.glossary_list()?;
    let mut done = summary.tm_hits;
    for batch in remaining.chunks(opts.batch_size.max(1)) {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        let request = TranslationRequest {
            items: batch
                .iter()
                .map(|e| BatchItem {
                    id: e.id.clone(),
                    text: e.source_text.clone(),
                    context: e.context.clone(),
                    max_bytes: e.max_bytes,
                })
                .collect(),
            source_language: opts.source_language.clone(),
            target_language: opts.target_language.clone(),
            glossary: relevant_terms(&glossary, batch),
        };

        match call_with_retry(provider, &request, opts).await {
            Ok(response) => {
                for item in &response.translations {
                    let Some(entry) = batch.iter().find(|e| e.id == item.id) else {
                        continue; // id inventado pelo modelo: ignora
                    };
                    db.update_translation(
                        &entry.id,
                        &item.translation,
                        TranslationStatus::Machine,
                    )?;
                    db.tm_store(
                        &entry.source_text,
                        &src_lang,
                        &opts.target_language,
                        &item.translation,
                    )?;
                    summary.translated += 1;
                }
                let missing = batch.len()
                    - response
                        .translations
                        .iter()
                        .filter(|t| batch.iter().any(|e| e.id == t.id))
                        .count();
                summary.failed += missing;
            }
            Err(e) => {
                warn!(batch = batch.len(), error = %e, "batch falhou apos retries");
                summary.failed += batch.len();
            }
        }
        done += batch.len();
        on_progress(Progress {
            done,
            total,
            phase: "translate",
        });
    }

    db.record_run(
        provider.id(),
        model_label,
        &started_at,
        summary.translated,
        summary.tm_hits,
        summary.failed,
    )?;
    info!(?summary, "traducao concluida");
    Ok(summary)
}

async fn call_with_retry(
    provider: &dyn TranslationProvider,
    request: &TranslationRequest,
    opts: &TranslateOptions,
) -> Result<crate::provider::TranslationResponse> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match provider.translate_batch(request).await {
            Ok(resp) => return Ok(resp),
            Err(e) if attempt < opts.max_attempts.max(1) => {
                warn!(attempt, error = %e, "batch falhou; retry com backoff");
                let delay = opts.retry_delay_ms.saturating_mul(2u64.pow(attempt - 1));
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// So os termos do glossario que aparecem em algum texto do batch — prompt curto.
fn relevant_terms(glossary: &[GlossaryTerm], batch: &[TextEntry]) -> Vec<GlossaryTerm> {
    glossary
        .iter()
        .filter(|g| {
            batch.iter().any(|e| {
                if g.case_sensitive {
                    e.source_text.contains(&g.term)
                } else {
                    e.source_text
                        .to_lowercase()
                        .contains(&g.term.to_lowercase())
                }
            })
        })
        .cloned()
        .collect()
}

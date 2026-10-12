//! Pipeline de traducao (spec §25 Sprint 3): TM lookup -> batches no provider,
//! com retry/backoff, cancelamento e progresso. Sequencial por enquanto.
// ponytail: batches sequenciais; concorrencia limitada entra se medir que uma
// API remota vira gargalo (Ollama local nao paraleliza de verdade).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use tracing::{info, warn};

use crate::db::{GlobalTm, ProjectDb};
use crate::error::Result;
use crate::provider::{BatchItem, GlossaryTerm, TranslationProvider, TranslationRequest};
use crate::tbl::{ProjectTables, TblTable};
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

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
    mut global_tm: Option<&mut GlobalTm>,
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

    let mut tables = ProjectTables::new(db.project_dir());

    // Fase 1 — translation memory em cascata: projeto primeiro, global depois.
    let mut remaining: Vec<TextEntry> = Vec::new();
    for entry in pending {
        // Tabela ausente do projeto e erro ja aqui: sem ela nao ha o que
        // dizer pro provider nem como conferir a resposta.
        let table = tables.for_entry(&entry)?;
        let mut hit = db.tm_lookup(&entry.source_text, &src_lang, &opts.target_language)?;
        if hit.is_none() {
            if let Some(global) = global_tm.as_deref_mut() {
                hit = global.tm_lookup(&entry.source_text, &src_lang, &opts.target_language)?;
            }
        }
        // Memoria que a tabela nao escreve nem sem os acentos (veio de texto
        // de outro encoding) nao serve: o provider recebe a lista de
        // caracteres e responde algo que cabe.
        let hit = hit.filter(|hit| {
            table.is_none_or(|table| {
                let fitted = table.drop_missing_accents(hit);
                table.encode(fitted.as_deref().unwrap_or(hit)).is_ok()
            })
        });
        if let Some(hit) = hit {
            save_translation(db, &entry, &hit, table)?;
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

    // Fase 2 — provider, em batches sequenciais com retry. Um batch, uma
    // tabela: o prompt lista os caracteres de UMA tabela. A ordenacao e
    // estavel — texto sem tabela vem primeiro, na ordem de sempre.
    let glossary = db.glossary_list()?;
    let mut done = summary.tm_hits;
    remaining.sort_by(|a, b| table_id(a).cmp(&table_id(b)));
    let batches = remaining
        .chunk_by(|a, b| table_id(a) == table_id(b))
        .flat_map(|same_table| same_table.chunks(opts.batch_size.max(1)));
    for batch in batches {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        let table = tables.for_entry(&batch[0])?;
        let request = TranslationRequest {
            items: batch
                .iter()
                .map(|e| BatchItem {
                    id: e.id.clone(),
                    text: e.source_text.clone(),
                    context: e.context.clone(),
                    max_bytes: e.max_bytes,
                    // Texto de tabela sem ponteiro em tabela so e gravado no
                    // lugar: o espaco da original e o teto, e o modelo
                    // precisa saber dele.
                    max_chars: table
                        .filter(|_| e.pointer_offsets().is_empty())
                        .map(|table| table.chars_that_fit(e.original_bytes.len())),
                })
                .collect(),
            source_language: opts.source_language.clone(),
            target_language: opts.target_language.clone(),
            glossary: relevant_terms(&glossary, batch),
            charset: table.map(TblTable::charset),
        };

        match call_with_retry(provider, &request, opts).await {
            Ok(response) => {
                for item in &response.translations {
                    let Some(entry) = batch.iter().find(|e| e.id == item.id) else {
                        continue; // id inventado pelo modelo: ignora
                    };
                    save_translation(db, entry, &item.translation, table)?;
                    db.tm_store(
                        &entry.source_text,
                        &src_lang,
                        &opts.target_language,
                        &item.translation,
                    )?;
                    // Resposta moldada pela tabela de UM jogo (sem acento,
                    // sem a pontuacao que falta) nao vai pra memoria global:
                    // outro projeto a receberia sem perguntar ao provider.
                    if let Some(global) = global_tm.as_deref_mut().filter(|_| table.is_none()) {
                        global.tm_store(
                            &entry.source_text,
                            &src_lang,
                            &opts.target_language,
                            &item.translation,
                        )?;
                    }
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

/// O id da tabela de caracteres que le e grava a entry, se houver.
fn table_id(entry: &TextEntry) -> Option<&str> {
    match &entry.encoding {
        TextEncoding::Table(id) => Some(id),
        _ => None,
    }
}

/// Grava a traducao automatica da entry. Texto de tabela passa antes pela
/// rede de seguranca: acento que a tabela nao tem vira a letra base
/// (`TblTable::drop_missing_accents`), e a troca fica registrada pra
/// validacao mostrar como aviso. O que continuar fora da tabela e gravado
/// como veio — e erro de validacao, nunca byte inventado.
fn save_translation(
    db: &mut ProjectDb,
    entry: &TextEntry,
    answer: &str,
    table: Option<&TblTable>,
) -> Result<()> {
    let written = table.and_then(|table| table.drop_missing_accents(answer));
    db.update_translation(
        &entry.id,
        written.as_deref().unwrap_or(answer),
        TranslationStatus::Machine,
    )?;
    if let Some(written) = &written {
        db.record_accent_swap(&entry.id, answer, written)?;
    }
    Ok(())
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

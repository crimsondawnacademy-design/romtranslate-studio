//! Pipeline de traducao (spec §25 Sprint 3): TM lookup -> batches no provider,
//! com retry/backoff, cancelamento e progresso. Sequencial por enquanto.
// ponytail: batches sequenciais; concorrencia limitada entra se medir que uma
// API remota vira gargalo (Ollama local nao paraleliza de verdade).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use tracing::{info, warn};

use crate::adapters::inplace::terminated;
use crate::db::{GlobalTm, ProjectDb};
use crate::error::Result;
use crate::provider::{BatchItem, GlossaryTerm, TranslationProvider, TranslationRequest};
use crate::tbl::{plain_equivalent, ProjectTables, TblTable};
use crate::types::{TextEncoding, TextEntry, TranslationStatus};
use crate::validate::extract_tokens;

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
        // Nome interno do jogo nao e texto: nem memoria, nem provider.
        .filter(|e| e.translated_text.is_none() && e.status != TranslationStatus::Internal)
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
        let charset = Charset::of(&entry, &mut tables)?;
        let mut hit = db.tm_lookup(&entry.source_text, &src_lang, &opts.target_language)?;
        if hit.is_none() {
            if let Some(global) = global_tm.as_deref_mut() {
                hit = global.tm_lookup(&entry.source_text, &src_lang, &opts.target_language)?;
            }
        }
        // Memoria que o conjunto da entry nao escreve nem sem os acentos, ou
        // que nao cabe no espaco de uma string de tabela (veio de texto de
        // outro encoding, de outro projeto ou de uma resposta que estourou),
        // nao serve: o provider recebe a lista de caracteres e o teto, e
        // responde de novo.
        let hit = hit.filter(|hit| charset.is_none_or(|charset| charset.accepts(&entry, hit)));
        if let Some(hit) = hit {
            save_translation(db, &entry, &hit, charset)?;
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

    // Fase 2 — provider, em batches sequenciais com retry. Um batch, um
    // conjunto de caracteres: o prompt lista os caracteres de UM conjunto, e
    // texto que escreve tudo (UTF-8, UTF-16) nunca divide batch com texto
    // que nao tem acento. A ordenacao e estavel — dentro de cada grupo, a
    // ordem de sempre.
    let glossary = db.glossary_list()?;
    let mut done = summary.tm_hits;
    remaining.sort_by(|a, b| charset_group(a).cmp(&charset_group(b)));
    let batches = remaining
        .chunk_by(|a, b| charset_group(a) == charset_group(b))
        .flat_map(|same_charset| same_charset.chunks(opts.batch_size.max(1)));
    for batch in batches {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        let charset = Charset::of(&batch[0], &mut tables)?;
        let table = match charset {
            Some(Charset::Table(table)) => Some(table),
            _ => None,
        };
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
                    // precisa saber dele. O que a traducao e obrigada a
                    // copiar do original (codigo de controle, placeholder)
                    // entra pelo que custa, nao pelo que o modelo conta.
                    max_chars: table
                        .filter(|_| e.pointer_offsets().is_empty())
                        .map(|table| {
                            table.chars_that_fit_keeping(
                                e.original_bytes.len(),
                                &extract_tokens(&e.source_text),
                            )
                        }),
                })
                .collect(),
            source_language: opts.source_language.clone(),
            target_language: opts.target_language.clone(),
            glossary: relevant_terms(&glossary, batch),
            charset: charset.map(Charset::chars),
        };

        match call_with_retry(provider, &request, opts).await {
            Ok(response) => {
                for item in &response.translations {
                    let Some(entry) = batch.iter().find(|e| e.id == item.id) else {
                        continue; // id inventado pelo modelo: ignora
                    };
                    save_translation(db, entry, &item.translation, charset)?;
                    db.tm_store(
                        &entry.source_text,
                        &src_lang,
                        &opts.target_language,
                        &item.translation,
                    )?;
                    // Resposta moldada pela tabela de UM jogo (sem acento,
                    // sem a pontuacao que falta) nao vai pra memoria global:
                    // outro projeto a receberia sem perguntar ao provider.
                    // ponytail: a de batch ASCII vai, como sempre foi — todo
                    // adapter extrai ASCII, e sem ela a memoria global nao
                    // serviria de um jogo pro outro. O custo: entry UTF-8 ou
                    // UTF-16 de outro projeto pode receber dela uma traducao
                    // sem acento. Marcar na memoria o conjunto que moldou a
                    // resposta se aparecer projeto que misture os dois.
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

/// O que o encoding da entry escreve, quando e menos que o Unicode. Decide o
/// batch (um batch, um conjunto), vai pro prompt e e a rede por onde a
/// traducao automatica passa antes de ser gravada. UTF-8 e UTF-16 escrevem
/// tudo: nao tem conjunto, e nada e trocado neles.
// ponytail: Shift-JIS fica sem conjunto — nao ha encoder nem extrator dele
// aqui, e o conjunto nao cabe numa linha de prompt.
#[derive(Clone, Copy)]
enum Charset<'a> {
    /// `TextEncoding::Ascii`: o ASCII imprimivel.
    Ascii,
    /// `TextEncoding::Table`: o que a tabela do projeto escreve.
    Table(&'a TblTable),
}

impl<'a> Charset<'a> {
    fn of(entry: &TextEntry, tables: &'a mut ProjectTables) -> Result<Option<Self>> {
        Ok(match &entry.encoding {
            TextEncoding::Ascii => Some(Charset::Ascii),
            TextEncoding::Table(id) => Some(Charset::Table(tables.get(id)?)),
            _ => None,
        })
    }

    /// A lista de caracteres que vai pro prompt do batch.
    fn chars(self) -> String {
        match self {
            Charset::Ascii => (' '..='~').collect(),
            Charset::Table(table) => table.charset(),
        }
    }

    /// `text` com o que o conjunto nao escreve trocado pelo equivalente
    /// simples (`tbl::plain_equivalent`: a letra sem acento, o sinal ASCII
    /// das aspas e do travessao tipograficos). `None` = nada a trocar. O que
    /// nao tem equivalente fica como veio, e a validacao acusa.
    fn fit(self, text: &str) -> Option<String> {
        match self {
            Charset::Ascii => {
                let plain = |ch: char| plain_equivalent(ch).unwrap_or_else(|| ch.to_string());
                let fitted: String = text.chars().map(plain).collect();
                (fitted != text).then_some(fitted)
            }
            Charset::Table(table) => table.drop_missing_accents(text),
        }
    }

    /// A reinsercao aceita `text` no lugar desta entry? O conjunto tem que
    /// escreve-lo (sem os acentos que faltam) e, em texto de tabela que so e
    /// gravado no lugar, ele tem que caber no espaco da string — a conta da
    /// validacao, sobra de largura fixa inclusive.
    fn accepts(self, entry: &TextEntry, text: &str) -> bool {
        let fitted = self.fit(text);
        let text = fitted.as_deref().unwrap_or(text);
        match self {
            Charset::Ascii => text.is_ascii(),
            Charset::Table(table) => {
                let slot = entry.original_bytes.len();
                table
                    .encode_slot(text, slot, terminated(entry))
                    .is_ok_and(|bytes| bytes.len() <= slot || !entry.pointer_offsets().is_empty())
            }
        }
    }
}

/// Entries do mesmo grupo escrevem com o mesmo conjunto de caracteres e
/// podem dividir um batch: sem restricao (UTF-8, UTF-16), ASCII, e cada
/// tabela. Os mesmos casos de `Charset::of`.
fn charset_group(entry: &TextEntry) -> (u8, &str) {
    match &entry.encoding {
        TextEncoding::Ascii => (1, ""),
        TextEncoding::Table(id) => (2, id),
        _ => (0, ""),
    }
}

/// Grava a traducao automatica da entry. Texto de conjunto restrito (ASCII,
/// tabela) passa antes pela rede de seguranca: o que o conjunto nao escreve
/// e tem equivalente simples e trocado por ele (`Charset::fit`), e a troca
/// fica registrada pra validacao mostrar como aviso. O que continuar fora do
/// conjunto e gravado como veio — e erro de validacao, nunca byte inventado.
fn save_translation(
    db: &mut ProjectDb,
    entry: &TextEntry,
    answer: &str,
    charset: Option<Charset>,
) -> Result<()> {
    let written = charset.and_then(|charset| charset.fit(answer));
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

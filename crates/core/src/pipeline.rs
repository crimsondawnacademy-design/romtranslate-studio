//! Pipeline de traducao (spec §25 Sprint 3): TM lookup -> batches no provider,
//! varios ao mesmo tempo, com retry/backoff, cancelamento e progresso; o que
//! volta maior que o espaco da frase e repedido ao provider.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;
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
use crate::validate::{encoded_len, extract_tokens, EncodedLen};

#[derive(Debug, Clone)]
pub struct TranslateOptions {
    pub source_language: Option<String>,
    pub target_language: String,
    pub batch_size: usize,
    /// Tentativas por batch (1 = sem retry).
    pub max_attempts: u32,
    pub retry_delay_ms: u64,
    /// Batches mandados ao provider ao mesmo tempo (1 = um por vez). Medido
    /// em 13/10/2026 num jogo real, um por vez: 25 a 40 s por chamada do
    /// Codex CLI, ~2,5 h pro jogo inteiro.
    pub parallel: usize,
    /// Quantas vezes a traducao que nao coube no espaco da frase volta ao
    /// provider, com o tamanho que passou (0 = nao repede).
    pub shorten_rounds: u32,
}

impl TranslateOptions {
    pub fn new(target_language: impl Into<String>) -> Self {
        TranslateOptions {
            source_language: None,
            target_language: target_language.into(),
            batch_size: 25,
            max_attempts: 3,
            retry_delay_ms: 1000,
            parallel: 4,
            shorten_rounds: 2,
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
    /// O provider parou de responder (limite da conta, sem login, sem rede):
    /// a rodada foi interrompida, e este e o erro que ele deu. O que nao foi
    /// tentado continua pendente.
    pub stopped: Option<String>,
    /// Traducoes desta rodada que nao cabiam no espaco da frase e passaram a
    /// caber depois de repedidas ao provider.
    pub shortened: usize,
    /// Traducoes desta rodada que continuam maiores que o espaco da frase.
    pub too_long: usize,
    /// Tokens que o provider declarou ter cobrado nesta rodada (zero quando
    /// ele nao informa, como o Codex CLI).
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Caracteres por token usados quando nao ha contagem do proprio modelo.
// ponytail: chute medio pra JSON em ingles. A API do Claude conta de verdade
// (`AnthropicProvider::count_input_tokens`) e quem chama calibra por ela.
pub const DEFAULT_CHARS_PER_TOKEN: f64 = 3.5;

/// O tamanho da rodada que "Traduzir pendentes" faria agora, sem chamar o
/// provider. A memoria de traducao nao entra na conta: o que ela resolver
/// sai de graca, entao isto e um teto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationEstimate {
    pub entries: usize,
    pub batches: usize,
    /// Tamanho de tudo o que seria enviado (prompt de sistema + itens).
    pub input_chars: usize,
    /// Tamanho esperado das respostas: a traducao costuma ficar perto do
    /// original, mais o JSON em volta. Raciocinio do modelo nao esta aqui.
    pub output_chars: usize,
}

/// Monta os mesmos lotes de `run_translation` e mede. Devolve tambem o
/// primeiro pedido, pra quem quiser calibrar a conta de tokens por ele.
pub fn estimate_translation(
    db: &ProjectDb,
    opts: &TranslateOptions,
) -> Result<(TranslationEstimate, Option<TranslationRequest>)> {
    let mut pending: Vec<TextEntry> = db
        .load_entries()?
        .into_iter()
        .filter(|e| e.translated_text.is_none() && e.status != TranslationStatus::Internal)
        .collect();
    let mut tables = ProjectTables::new(db.project_dir());
    let glossary = db.glossary_list()?;
    pending.sort_by(|a, b| charset_group(a).cmp(&charset_group(b)));
    let mut estimate = TranslationEstimate {
        entries: pending.len(),
        batches: 0,
        input_chars: 0,
        output_chars: 0,
    };
    let mut sample = None;
    let batches = pending
        .chunk_by(|a, b| charset_group(a) == charset_group(b))
        .flat_map(|same_charset| same_charset.chunks(opts.batch_size.max(1)));
    for batch in batches {
        let charset = Charset::of(&batch[0], &mut tables)?;
        let request = batch_request(batch, charset, &glossary, opts);
        estimate.batches += 1;
        estimate.input_chars += crate::provider::build_system_prompt(&request).len()
            + crate::provider::build_user_prompt(&request).len();
        estimate.output_chars += 20
            + batch
                .iter()
                .map(|e| e.id.len() + e.source_text.len() * 12 / 10 + 40)
                .sum::<usize>();
        sample.get_or_insert(request);
    }
    Ok((estimate, sample))
}

/// O pedido de um lote: os itens, o glossario que aparece neles e, quando o
/// texto e gravado por um conjunto de caracteres, a lista e o teto de cada um.
fn batch_request(
    batch: &[TextEntry],
    charset: Option<Charset>,
    glossary: &[GlossaryTerm],
    opts: &TranslateOptions,
) -> TranslationRequest {
    let table = match charset {
        Some(Charset::Table(table)) => Some(table),
        _ => None,
    };
    TranslationRequest {
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
                    })
                    // Japones diz em 2 bytes o que o portugues diz em 5 ou
                    // 6 letras: sem o teto ja no primeiro pedido, quase toda
                    // frase presa ao espaco dela voltaria grande demais.
                    .or_else(|| {
                        (e.encoding == TextEncoding::ShiftJis && e.pointer_offsets().is_empty())
                            .then(|| e.max_bytes.unwrap_or(e.original_bytes.len()))
                    }),
            })
            .collect(),
        source_language: opts.source_language.clone(),
        target_language: opts.target_language.clone(),
        glossary: relevant_terms(glossary, batch),
        charset: charset.map(Charset::chars),
    }
}

/// Lotes seguidos que podem falhar (cada um ja com as suas tentativas) antes
/// de a rodada parar. Sem isto, uma conta que bateu no limite levava a fila
/// inteira a falhar em silencio, lote por lote, por horas.
const STOP_AFTER_FAILED_BATCHES: usize = 2;

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
        stopped: None,
        shortened: 0,
        too_long: 0,
        input_tokens: 0,
        output_tokens: 0,
    };
    let billed_before = provider.usage();
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

    // Fase 2 — provider, `opts.parallel` batches por vez, cada um com retry.
    // Um batch, um conjunto de caracteres: o prompt lista os caracteres de UM
    // conjunto, e texto que escreve tudo (UTF-8, UTF-16) nunca divide batch
    // com texto que nao tem acento. A ordenacao e estavel — dentro de cada
    // grupo, a ordem de sempre.
    let glossary = db.glossary_list()?;
    let mut done = summary.tm_hits;
    let mut failed_in_a_row = 0;
    // O que o provider respondeu nesta rodada: so isto e repedido na fase 3.
    let mut answered: HashSet<String> = HashSet::new();
    remaining.sort_by(|a, b| charset_group(a).cmp(&charset_group(b)));
    // A mesma frase aparece varias vezes num jogo (menu, nome de item, fala
    // repetida): so a primeira vai ao provider, e a traducao que ficar pra ela
    // vale pras outras. Medido em 13/10/2026 num jogo real: 8.717 textos
    // distintos em 11.161 frases — uma em cada cinco chamadas era repeticao.
    // "A mesma" = mesmo texto, mesmo conjunto de caracteres e mesmo espaco.
    let mut first_of: HashMap<(u8, String, String, usize, bool), String> = HashMap::new();
    let mut copies: Vec<(TextEntry, String)> = Vec::new();
    let mut unique: Vec<TextEntry> = Vec::with_capacity(remaining.len());
    for entry in remaining {
        let (kind, table_id) = charset_group(&entry);
        let key = (
            kind,
            table_id.to_string(),
            entry.source_text.clone(),
            entry.max_bytes.unwrap_or(entry.original_bytes.len()),
            entry.pointer_offsets().is_empty(),
        );
        match first_of.get(&key) {
            Some(first) => copies.push((entry, first.clone())),
            None => {
                first_of.insert(key, entry.id.clone());
                unique.push(entry);
            }
        }
    }
    let remaining = unique;
    let batches: Vec<&[TextEntry]> = remaining
        .chunk_by(|a, b| charset_group(a) == charset_group(b))
        .flat_map(|same_charset| same_charset.chunks(opts.batch_size.max(1)))
        .collect();
    for wave in batches.chunks(opts.parallel.max(1)) {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        // Os pedidos da leva ficam prontos antes das chamadas: o conjunto de
        // caracteres e emprestado de `tables`, um batch de cada vez.
        let mut requests = Vec::with_capacity(wave.len());
        for batch in wave {
            let charset = Charset::of(&batch[0], &mut tables)?;
            requests.push(batch_request(batch, charset, &glossary, opts));
        }
        let Some(responses) = call_together(provider, &requests, opts, cancel).await else {
            summary.cancelled = true;
            break;
        };

        for (batch, response) in wave.iter().zip(responses) {
            let charset = Charset::of(&batch[0], &mut tables)?;
            let table = match charset {
                Some(Charset::Table(table)) => Some(table),
                _ => None,
            };
            match response {
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
                        answered.insert(entry.id.clone());
                        summary.translated += 1;
                    }
                    let missing = batch.len()
                        - response
                            .translations
                            .iter()
                            .filter(|t| batch.iter().any(|e| e.id == t.id))
                            .count();
                    summary.failed += missing;
                    failed_in_a_row = 0;
                }
                Err(e) => {
                    warn!(batch = batch.len(), error = %e, "batch falhou apos retries");
                    summary.failed += batch.len();
                    failed_in_a_row += 1;
                    // As outras respostas da leva ja chegaram: o que veio
                    // certo e gravado, e a rodada para depois dela.
                    if failed_in_a_row >= STOP_AFTER_FAILED_BATCHES && summary.stopped.is_none() {
                        summary.stopped = Some(e.to_string());
                    }
                }
            }
            done += batch.len();
            on_progress(Progress {
                done,
                total,
                phase: "translate",
            });
        }
        if summary.stopped.is_some() {
            break;
        }
    }

    // Fase 3 — o que voltou maior que o espaco da frase volta ao provider,
    // com a traducao anterior e o teto em caracteres. Medido em 13/10/2026 num
    // jogo real: 7% das respostas nao cabiam (batch de 25 e de 50) e ficavam
    // de fora da reinsercao; repedindo com o tamanho, o estouro some. So entra
    // o que o provider respondeu NESTA rodada: traducao escrita a mao ou vinda
    // da memoria nao e trocada sem a pessoa pedir.
    for _ in 0..opts.shorten_rounds {
        if summary.cancelled || summary.stopped.is_some() || cancel.load(Ordering::Relaxed) {
            break;
        }
        let mut too_long = too_long_entries(db, &answered, &mut tables)?;
        if too_long.is_empty() {
            break;
        }
        let asking = too_long.len();
        let mut fixed = 0;
        too_long.sort_by(|a, b| charset_group(a).cmp(&charset_group(b)));
        let batches: Vec<&[TextEntry]> = too_long
            .chunk_by(|a, b| charset_group(a) == charset_group(b))
            .flat_map(|same_charset| same_charset.chunks(opts.batch_size.max(1)))
            .collect();
        for wave in batches.chunks(opts.parallel.max(1)) {
            if cancel.load(Ordering::Relaxed) {
                summary.cancelled = true;
                break;
            }
            let mut requests = Vec::with_capacity(wave.len());
            for batch in wave {
                let charset = Charset::of(&batch[0], &mut tables)?;
                let mut request = batch_request(batch, charset, &glossary, opts);
                for (item, entry) in request.items.iter_mut().zip(batch.iter()) {
                    shorter_please(item, entry, charset);
                }
                requests.push(request);
            }
            let Some(responses) = call_together(provider, &requests, opts, cancel).await else {
                summary.cancelled = true;
                break;
            };

            for (batch, response) in wave.iter().zip(responses) {
                let charset = Charset::of(&batch[0], &mut tables)?;
                // Batch que falha aqui nao para nada: a traducao de antes fica.
                let Ok(response) = response else { continue };
                for item in &response.translations {
                    let Some(entry) = batch.iter().find(|e| e.id == item.id) else {
                        continue;
                    };
                    let fitted = charset.and_then(|charset| charset.fit(&item.translation));
                    let answer = fitted.as_deref().unwrap_or(&item.translation);
                    // Resposta vazia "cabe" em qualquer espaco: nao e traducao.
                    if answer.trim().is_empty() || excess(entry, answer, charset) != Some(0) {
                        continue; // ainda nao cabe: fica a de antes
                    }
                    save_translation(db, entry, &item.translation, charset)?;
                    db.tm_store(
                        &entry.source_text,
                        &src_lang,
                        &opts.target_language,
                        &item.translation,
                    )?;
                    fixed += 1;
                }
                on_progress(Progress {
                    done: fixed,
                    total: asking,
                    phase: "shorten",
                });
            }
        }
        summary.shortened += fixed;
        if fixed == 0 {
            break; // o provider nao encurta: repedir de novo so gasta chamada
        }
    }
    summary.too_long = too_long_entries(db, &answered, &mut tables)?.len();

    // As repeticoes recebem a traducao que ficou pra primeira (ja encurtada,
    // se foi o caso). A que ficou sem resposta deixa as copias pendentes.
    if !copies.is_empty() {
        let translated: HashMap<String, String> = db
            .load_entries()?
            .into_iter()
            .filter(|e| answered.contains(&e.id))
            .filter_map(|e| e.translated_text.map(|text| (e.id, text)))
            .collect();
        for (copy, first) in &copies {
            match translated.get(first) {
                Some(text) => {
                    db.update_translation(&copy.id, text, TranslationStatus::Machine)?;
                    summary.translated += 1;
                    done += 1;
                }
                None if !summary.cancelled && summary.stopped.is_none() => summary.failed += 1,
                None => {}
            }
        }
        on_progress(Progress {
            done,
            total,
            phase: "translate",
        });
    }

    let billed = provider.usage();
    summary.input_tokens = billed.input_tokens - billed_before.input_tokens;
    summary.output_tokens = billed.output_tokens - billed_before.output_tokens;

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
// ponytail: Shift-JIS entra como ASCII. O conjunto dele (quase 7 mil
// caracteres) nao cabe numa linha de prompt, e a traducao pra um idioma de
// alfabeto latino so usa mesmo o ASCII: o JIS X 0208 nao tem letra acentuada.
// Traducao manual com o que o Shift-JIS escreve (kana, pontuacao japonesa)
// passa na validacao, que usa o encoder de verdade. Rever se o idioma de
// destino for japones.
#[derive(Clone, Copy)]
enum Charset<'a> {
    /// `TextEncoding::Ascii` e `TextEncoding::ShiftJis`: o ASCII imprimivel.
    Ascii,
    /// `TextEncoding::Table`: o que a tabela do projeto escreve.
    Table(&'a TblTable),
}

impl<'a> Charset<'a> {
    fn of(entry: &TextEntry, tables: &'a mut ProjectTables) -> Result<Option<Self>> {
        Ok(match &entry.encoding {
            TextEncoding::Ascii | TextEncoding::ShiftJis => Some(Charset::Ascii),
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
        TextEncoding::Ascii | TextEncoding::ShiftJis => (1, ""),
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

/// Bytes que `text` passa do espaco da frase de `entry` (0 = cabe). `None`
/// quando o tamanho nao decide nada: a frase pode ir pra outro lugar (ponteiro
/// em tabela), o encoding escreve tudo, ou `text` nem se escreve nele.
fn excess(entry: &TextEntry, text: &str, charset: Option<Charset>) -> Option<usize> {
    let charset = charset?;
    if !entry.pointer_offsets().is_empty() {
        return None;
    }
    let table = match charset {
        Charset::Table(table) => Some(table),
        Charset::Ascii => None,
    };
    let EncodedLen::Bytes(len) = encoded_len(text, &entry.encoding, table) else {
        return None;
    };
    let slot = entry.max_bytes.unwrap_or(entry.original_bytes.len());
    Some(len.saturating_sub(slot))
}

/// As entries de `ids` cuja traducao gravada passa do espaco da frase.
fn too_long_entries(
    db: &ProjectDb,
    ids: &HashSet<String>,
    tables: &mut ProjectTables,
) -> Result<Vec<TextEntry>> {
    let mut out = Vec::new();
    for entry in db.load_entries()? {
        if !ids.contains(&entry.id) {
            continue;
        }
        let charset = Charset::of(&entry, tables)?;
        let over = entry
            .translated_text
            .as_deref()
            .and_then(|text| excess(&entry, text, charset));
        if over.is_some_and(|over| over > 0) {
            out.push(entry);
        }
    }
    Ok(out)
}

/// Transforma o item do pedido numa repergunta: diz ao provider a traducao
/// que nao coube e quantos caracteres cabem. O teto vai em `maxChars`, que o
/// prompt ja trata como limite duro.
fn shorter_please(item: &mut BatchItem, entry: &TextEntry, charset: Option<Charset>) {
    let (Some(charset), Some(previous)) = (charset, entry.translated_text.as_deref()) else {
        return;
    };
    let room = match charset {
        Charset::Ascii => entry.max_bytes.unwrap_or(entry.original_bytes.len()),
        Charset::Table(table) => table.chars_that_fit_keeping(
            entry.original_bytes.len(),
            &extract_tokens(&entry.source_text),
        ),
    };
    let note = format!(
        "Your previous translation \"{previous}\" does not fit: it has {} characters and the \
         limit is {room}. Write a shorter one.",
        previous.chars().count()
    );
    item.max_chars = Some(room);
    item.context = Some(match item.context.take() {
        Some(context) => format!("{context}\n{note}"),
        None => note,
    });
}

/// Manda os pedidos ao provider ao mesmo tempo e devolve as respostas na
/// ordem dos pedidos. `None` = cancelado no meio: as chamadas em andamento
/// sao largadas na hora (o Codex CLI morre junto, o pedido HTTP e abortado) e
/// as frases delas continuam pendentes.
async fn call_together(
    provider: &dyn TranslationProvider,
    requests: &[TranslationRequest],
    opts: &TranslateOptions,
    cancel: &AtomicBool,
) -> Option<Vec<Result<crate::provider::TranslationResponse>>> {
    type Call<'a> =
        Pin<Box<dyn Future<Output = Result<crate::provider::TranslationResponse>> + Send + 'a>>;
    // O `join_all` do crate `futures`, sem a dependencia: os futuros andam
    // juntos nesta task, e cada um guarda a propria saida.
    let mut calls: Vec<Option<Call>> = requests
        .iter()
        .map(|request| Some(Box::pin(call_with_retry(provider, request, opts)) as Call))
        .collect();
    let mut answers: Vec<Option<Result<crate::provider::TranslationResponse>>> =
        requests.iter().map(|_| None).collect();
    // O relogio acorda a task mesmo com todas as chamadas paradas esperando o
    // provider: e ele que faz o "Cancelar" valer em menos de um segundo, e nao
    // so quando a chamada mais lenta da leva termina (ate 40 s no Codex CLI).
    let mut clock = tokio::time::interval(Duration::from_millis(150));
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let cancelled = std::future::poll_fn(|cx| {
        while clock.poll_tick(cx).is_ready() {}
        if cancel.load(Ordering::Relaxed) {
            return Poll::Ready(true);
        }
        let mut waiting = false;
        for (call, answer) in calls.iter_mut().zip(answers.iter_mut()) {
            let Some(future) = call else { continue };
            match future.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    *answer = Some(value);
                    *call = None;
                }
                Poll::Pending => waiting = true,
            }
        }
        if waiting {
            Poll::Pending
        } else {
            Poll::Ready(false)
        }
    })
    .await;
    if cancelled {
        return None;
    }
    Some(answers.into_iter().flatten().collect())
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

//! CLI do core: os mesmos passos que os botoes do app chamam, sem a janela.
//! Casca fina — a logica mora toda em `romtranslate_core`. Aqui so se le
//! argumento, se chama o core e se imprime o resultado (texto, ou `--json`).
//!
//! Saida: 0 = o comando fez o que foi pedido; 1 = erro (a mensagem vai pro
//! stderr); 2 = argumento invalido. Relatorio com problema (validacao com
//! erros, frase pulada na reinsercao) nao e erro do comando: sai 0.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use romtranslate_core::adapters::extract_project;
use romtranslate_core::db::ProjectDb;
use romtranslate_core::detect::inspect;
use romtranslate_core::export::{export_csv, export_json};
use romtranslate_core::patch::{export_patch, PatchFormat};
use romtranslate_core::pipeline::{
    estimate_translation, run_translation, TranslateOptions, DEFAULT_CHARS_PER_TOKEN,
};
use romtranslate_core::project::{create_project, load_project, CreateProjectArgs};
use romtranslate_core::reinsert::{reinsert_project_with, ErrorPolicy};
use romtranslate_core::scan::{scan_project, ScanConfig, ScanEncoding};
use romtranslate_core::settings;
use romtranslate_core::types::{GameProject, TranslationStatus};
use romtranslate_core::validate::{validate_project_db, Severity, ValidationReport};
use romtranslate_core::CoreError;

/// O `identifier` de `tauri.conf.json`: a pasta de configuracao da CLI e a
/// do app (`dirs::config_dir()/<identifier>`, a conta que o Tauri faz).
const APP_IDENTIFIER: &str = "studio.romtranslate.desktop";

/// Quantos erros de validacao a saida em texto lista (o `--json` traz todos).
const LISTED_ERRORS: usize = 20;

#[derive(Parser)]
#[command(
    name = "romtranslate",
    version,
    about = "O pipeline do RomTranslate Studio sem a janela: os mesmos passos dos botoes do app."
)]
struct Cli {
    /// Saida em JSON numa linha, para automacao
    #[arg(long, global = true)]
    json: bool,
    /// Pasta de configuracao (padrao: a do app)
    #[arg(long, global = true, value_name = "PASTA")]
    config_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Identifica o arquivo: formato, plataforma e SHA-256
    Inspect {
        #[arg(value_name = "ARQUIVO")]
        file: PathBuf,
    },
    /// Cria o projeto de um arquivo (o original nao e copiado nem alterado)
    Create {
        #[arg(value_name = "ARQUIVO")]
        file: PathBuf,
        /// Pasta do projeto, que sera criada (ex.: jogo.rtsproj)
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
        /// Idioma de destino
        #[arg(long, default_value = "pt-BR", value_name = "IDIOMA")]
        target: String,
        /// Idioma de origem
        #[arg(long, value_name = "IDIOMA")]
        source: Option<String>,
    },
    /// Extracao estruturada do adapter da plataforma, gravada no projeto
    Extract {
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
    },
    /// Escaneia o arquivo atras de texto e grava o que achar no projeto
    Scan {
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
        /// ascii, utf8, utf16_le, utf16_be ou table [padrao: ascii, ou table com --tbl]
        #[arg(long, value_parser = named::<ScanEncoding>)]
        encoding: Option<ScanEncoding>,
        /// Tabela de caracteres (.tbl) do jogo
        #[arg(long, value_name = "ARQUIVO")]
        tbl: Option<PathBuf>,
        /// So esta parte do arquivo, em bytes (o fim nao entra; aceita 0x; um lado pode faltar)
        #[arg(long, value_name = "INICIO..FIM", value_parser = parse_region)]
        region: Option<Region>,
        /// Minimo de caracteres para virar frase
        #[arg(long, value_name = "N", default_value_t = ScanConfig::default().min_chars)]
        min_chars: usize,
        /// Descarta o que nao parece texto
        #[arg(long)]
        only_text: bool,
    },
    /// O tamanho da rodada de traducao, sem chamar o provider
    Estimate {
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
        /// Frases por chamada, de 1 a 50 [padrao: o das configuracoes do app]
        #[arg(long, value_name = "N")]
        batch: Option<usize>,
    },
    /// Traduz as frases pendentes com o provider das configuracoes do app
    Translate {
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
        /// Frases por chamada, de 1 a 50 [padrao: o das configuracoes do app]
        #[arg(long, value_name = "N")]
        batch: Option<usize>,
    },
    /// Grava traducoes de um arquivo (uma por linha: id<TAB>traducao) e valida
    Apply {
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
        #[arg(value_name = "ARQUIVO.TSV")]
        tsv: PathBuf,
    },
    /// Valida as traducoes do projeto, com a contagem por tipo de problema
    Validate {
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
    },
    /// Grava as traducoes numa copia do arquivo e confere a copia
    Reinsert {
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
        /// Traducao com erro: block (nada e gravado), skip (a frase fica no original) ou force
        #[arg(long, default_value = "skip", value_parser = named::<ErrorPolicy>)]
        policy: ErrorPolicy,
    },
    /// Exporta o patch da copia traduzida
    Patch {
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
        /// ips ou bps [padrao: IPS se couber, senao BPS]
        #[arg(long, value_parser = named::<PatchFormat>)]
        format: Option<PatchFormat>,
    },
    /// Exporta as frases do projeto (os ids que o `apply` espera)
    Export {
        #[arg(value_name = "PROJETO")]
        project: PathBuf,
        #[arg(value_name = "ARQUIVO")]
        file: PathBuf,
        #[arg(long, default_value = "json", value_parser = ["json", "csv"])]
        format: String,
    },
}

/// `--region`: (inicio, fim), cada lado opcional.
type Region = (Option<u64>, Option<u64>);

fn parse_region(arg: &str) -> Result<Region, String> {
    let (start, end) = arg
        .split_once("..")
        .ok_or("use INICIO..FIM, em bytes (ex.: 0x1000..0x2000)")?;
    let bound = |text: &str| {
        if text.is_empty() {
            return Ok(None);
        }
        let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            Some(hex) => u64::from_str_radix(hex, 16),
            None => text.parse(),
        };
        parsed
            .map(Some)
            .map_err(|_| format!("offset invalido: {text}"))
    };
    Ok((bound(start)?, bound(end)?))
}

/// Le o valor de uma opcao pelo nome que o core ja da ao enum (serde): a
/// lista de valores aceitos e a do core, sem copia aqui.
fn named<T: DeserializeOwned>(arg: &str) -> Result<T, String> {
    serde_json::from_value(Value::String(arg.to_string())).map_err(|e| e.to_string())
}

/// O nome que o core da a um valor de enum (o mesmo do JSON).
fn name(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(name)) => name,
        _ => "?".to_string(),
    }
}

/// O resultado de um comando nas duas formas: texto pra gente, JSON pra
/// automacao.
struct Output {
    text: String,
    json: Value,
    /// O comando rodou mas nao chegou ao fim (traducao parada ou cancelada):
    /// o resultado sai mesmo assim, e o processo termina com este erro.
    unfinished: Option<String>,
}

impl Output {
    fn new(text: String, json: Value) -> Self {
        Output {
            text,
            json,
            unfinished: None,
        }
    }
}

/// Erro de qualquer comando, ja como a mensagem que vai pro terminal.
struct Failure(String);

impl From<CoreError> for Failure {
    fn from(e: CoreError) -> Self {
        Failure(e.to_string())
    }
}

impl From<serde_json::Error> for Failure {
    fn from(e: serde_json::Error) -> Self {
        Failure(format!("serializacao: {e}"))
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure(message)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let as_json = cli.json;
    let fail = |message: &str| {
        if as_json {
            eprintln!("{}", json!({ "error": message }));
        } else {
            eprintln!("erro: {message}");
        }
        ExitCode::FAILURE
    };
    match run(cli) {
        Ok(out) => {
            let line = if as_json {
                out.json.to_string()
            } else {
                out.text
            };
            // Saida fechada do outro lado (`| head`) nao e erro deste comando.
            let _ = writeln!(std::io::stdout(), "{line}");
            match out.unfinished {
                Some(reason) => fail(&reason),
                None => ExitCode::SUCCESS,
            }
        }
        Err(Failure(message)) => fail(&message),
    }
}

fn run(cli: Cli) -> Result<Output, Failure> {
    let config_dir = || {
        cli.config_dir
            .clone()
            .or_else(|| dirs::config_dir().map(|dir| dir.join(APP_IDENTIFIER)))
            .ok_or_else(|| {
                Failure("pasta de configuracao do app nao encontrada: use --config-dir".to_string())
            })
    };
    match cli.command {
        Command::Inspect { file } => {
            let report = inspect(&file)?;
            let mut text = format!(
                "arquivo: {}\ntamanho: {} bytes\nsha256: {}\nformato: ",
                report.path.display(),
                report.size,
                report.sha256
            );
            match &report.best {
                Some(best) => text.push_str(&format!(
                    "{} ({}), confianca {:.2}, suporte {}",
                    best.platform.display_name(),
                    best.adapter_id,
                    best.confidence,
                    name(&best.support_level)
                )),
                None => text.push_str("nao reconhecido"),
            }
            for result in &report.results {
                let _ = write!(text, "\n  {} {:.2}", result.adapter_id, result.confidence);
                for evidence in &result.evidence {
                    let _ = write!(text, "\n    - {evidence}");
                }
            }
            Ok(Output::new(text, serde_json::to_value(&report)?))
        }
        Command::Create {
            file,
            project,
            target,
            source,
        } => {
            // O projeto guarda o caminho do arquivo: relativo, ele quebraria ao
            // rodar de outra pasta.
            let file = std::path::absolute(&file).map_err(|e| CoreError::io(&file, e))?;
            let report = inspect(&file)?;
            let best = report
                .best
                .ok_or_else(|| format!("formato nao reconhecido: {}", report.path.display()))?;
            let game = create_project(CreateProjectArgs {
                source_path: report.path,
                platform: best.platform,
                adapter_id: best.adapter_id,
                source_language: source,
                target_language: target,
                project_dir: project.clone(),
            })?;
            let text = format!(
                "projeto criado: {}\nformato: {} ({}), destino {}",
                project.display(),
                game.platform.display_name(),
                game.adapter_id,
                game.target_language
            );
            Ok(Output::new(text, serde_json::to_value(&game)?))
        }
        Command::Extract { project } => {
            load(&project)?;
            let outcome = extract_project(&project)?;
            let mut text = format!(
                "gravadas: {}, descartadas (nao parecem texto): {}, removidas do projeto antigo: {}",
                outcome.saved, outcome.discarded, outcome.removed
            );
            if outcome.truncated {
                text.push_str("\ncortou no teto: pode haver mais texto");
            }
            Ok(Output::new(text, serde_json::to_value(&outcome)?))
        }
        Command::Scan {
            project,
            encoding,
            tbl,
            region,
            min_chars,
            only_text,
        } => {
            load(&project)?;
            let encoding = encoding.unwrap_or(if tbl.is_some() {
                ScanEncoding::Table
            } else {
                ScanEncoding::Ascii
            });
            if tbl.is_some() && encoding != ScanEncoding::Table {
                return Err(Failure("--tbl so vale com --encoding table".to_string()));
            }
            let (region_start, region_end) = region.unwrap_or_default();
            let outcome = scan_project(
                &project,
                &ScanConfig {
                    encoding,
                    tbl_path: tbl,
                    min_chars,
                    region_start,
                    region_end,
                    only_text,
                    ..ScanConfig::default()
                },
            )?;
            let saved = ProjectDb::open(&project)?.upsert_entries(&outcome.entries)?;
            let mut text = format!(
                "achadas: {}, gravadas: {saved}, descartadas (nao parecem texto): {}, bytes lidos: {}",
                outcome.entries.len(),
                outcome.discarded,
                outcome.scanned_bytes
            );
            if outcome.truncated {
                text.push_str("\ncortou no teto: ha mais texto alem do que foi gravado");
            }
            if let Some(tbl) = &outcome.tbl_path {
                let _ = write!(text, "\ntabela do projeto: {}", tbl.display());
            }
            // As frases ficam no banco (`export` as entrega): o resumo nao as repete.
            let json = json!({
                "found": outcome.entries.len(),
                "saved": saved,
                "discarded": outcome.discarded,
                "truncated": outcome.truncated,
                "scannedBytes": outcome.scanned_bytes,
                "tblPath": outcome.tbl_path.as_ref().map(|tbl| tbl.display().to_string()),
            });
            Ok(Output::new(text, json))
        }
        Command::Estimate { project, batch } => {
            let game = load(&project)?;
            let batch = match batch {
                Some(batch) => batch,
                None => settings::load_settings(&config_dir()?).batch_size,
            };
            let (estimate, _) =
                estimate_translation(&ProjectDb::open(&project)?, &options(game, batch))?;
            // Media de caracteres por token: a tela calibra pelo Claude quando ha
            // chave; aqui a conta e sempre a media.
            let tokens = |chars: usize| (chars as f64 / DEFAULT_CHARS_PER_TOKEN).ceil() as u64;
            let (input, output) = (tokens(estimate.input_chars), tokens(estimate.output_chars));
            let text = format!(
                "pendentes: {}, chamadas: {}\nenvio: {} caracteres (~{input} tokens), resposta: {} caracteres (~{output} tokens)",
                estimate.entries, estimate.batches, estimate.input_chars, estimate.output_chars
            );
            let mut json = serde_json::to_value(&estimate)?;
            json["inputTokens"] = json!(input);
            json["outputTokens"] = json!(output);
            Ok(Output::new(text, json))
        }
        Command::Translate { project, batch } => translate(&project, batch, &config_dir()?),
        Command::Apply { project, tsv } => apply(&project, &tsv),
        Command::Validate { project } => {
            load(&project)?;
            let report = validate_project_db(&mut ProjectDb::open(&project)?)?;
            let (text, json) = validation(&report)?;
            Ok(Output::new(text, json))
        }
        Command::Reinsert { project, policy } => {
            load(&project)?;
            let outcome = reinsert_project_with(&project, policy)?;
            let mut text = format!(
                "arquivo traduzido: {}\naplicadas: {} ({} em outro lugar, com o ponteiro reapontado), no original: {}, fora da extracao estruturada: {}\npuladas por erro: {}, gravadas com erro: {}\nverificacao: {}",
                outcome.working_path.display(),
                outcome.apply.applied,
                outcome.apply.relocated,
                outcome.apply.kept_original,
                outcome.apply.ignored_generic,
                outcome.skipped_errors,
                outcome.forced_errors,
                if outcome.verification.ok { "ok" } else { "FALHOU" }
            );
            for problem in &outcome.verification.problems {
                let _ = write!(text, "\n  problema: {problem}");
            }
            Ok(Output::new(text, serde_json::to_value(&outcome)?))
        }
        Command::Patch { project, format } => {
            load(&project)?;
            let outcome = export_patch(&project, format)?;
            let text = format!(
                "patch {} ({} bytes): {}\nmanifesto: {}\ntraducoes: {}",
                outcome.patch_format,
                outcome.patch_size,
                outcome.patch_path.display(),
                outcome.manifest_path.display(),
                outcome.csv_path.display()
            );
            Ok(Output::new(text, serde_json::to_value(&outcome)?))
        }
        Command::Export {
            project,
            file,
            format,
        } => {
            load(&project)?;
            let entries = ProjectDb::open(&project)?.load_entries()?;
            match format.as_str() {
                "csv" => export_csv(&entries, &file)?,
                _ => export_json(&entries, &file)?,
            }
            let text = format!("{} frases em {}", entries.len(), file.display());
            Ok(Output::new(
                text,
                json!({ "entries": entries.len(), "path": file.display().to_string() }),
            ))
        }
    }
}

/// O manifesto do projeto. Conferido antes de qualquer comando: abrir o banco
/// numa pasta que nao e projeto criaria um banco vazio nela.
fn load(project: &Path) -> Result<GameProject, Failure> {
    load_project(project).map_err(|e| {
        Failure(format!(
            "{} nao e um projeto do RomTranslate: {e}",
            project.display()
        ))
    })
}

/// As opcoes de traducao que a tela monta: os idiomas do projeto e as frases
/// por chamada.
fn options(game: GameProject, batch: usize) -> TranslateOptions {
    let mut opts = TranslateOptions::new(game.target_language);
    opts.source_language = game.source_language;
    opts.batch_size = batch.clamp(1, 50);
    opts
}

/// O relatorio de validacao com a contagem por tipo de problema.
fn validation(report: &ValidationReport) -> Result<(String, Value), Failure> {
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for issue in &report.issues {
        *kinds
            .entry(format!("{}/{}", name(&issue.severity), name(&issue.kind)))
            .or_default() += 1;
    }
    let mut text = format!(
        "conferidas: {}, erros: {}, avisos: {}",
        report.checked, report.errors, report.warnings
    );
    for (kind, count) in &kinds {
        let _ = write!(text, "\n  {kind}: {count}");
    }
    let errors = || {
        report
            .issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
    };
    for issue in errors().take(LISTED_ERRORS) {
        let _ = write!(text, "\n  erro em {}: {}", issue.entry_id, issue.message);
    }
    let hidden = errors().count().saturating_sub(LISTED_ERRORS);
    if hidden > 0 {
        let _ = write!(text, "\n  ... e mais {hidden} (o --json lista todos)");
    }
    let mut json = serde_json::to_value(report)?;
    json["byKind"] = json!(kinds);
    Ok((text, json))
}

fn apply(project: &Path, tsv: &Path) -> Result<Output, Failure> {
    load(project)?;
    let content = std::fs::read_to_string(tsv).map_err(|e| CoreError::io(tsv, e))?;
    let mut db = ProjectDb::open(project)?;
    // O arquivo inteiro e conferido antes de gravar a primeira linha: TSV
    // quebrado nao deixa o projeto traduzido pela metade.
    let mut rows = Vec::new();
    for (index, line) in content.trim_start_matches('\u{feff}').lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let at = format!("{}, linha {}", tsv.display(), index + 1);
        // ponytail: uma linha, uma frase, sem escape — traducao com quebra de
        // linha de verdade nao cabe neste formato. Se um jogo pedir, o caminho
        // e importar o JSON do `export`.
        let (id, translation) = line
            .split_once('\t')
            .ok_or_else(|| format!("{at}: esperado id<TAB>traducao"))?;
        if db.get_entry(id)?.is_none() {
            return Err(Failure(format!("{at}: o projeto nao tem a frase {id}")));
        }
        rows.push((id, translation));
    }
    for (id, translation) in &rows {
        db.update_translation(id, translation, TranslationStatus::Machine)?;
    }
    let (text, report) = validation(&validate_project_db(&mut db)?)?;
    Ok(Output::new(
        format!("gravadas: {}\n{text}", rows.len()),
        json!({ "applied": rows.len(), "validation": report }),
    ))
}

fn translate(project: &Path, batch: Option<usize>, cfg_dir: &Path) -> Result<Output, Failure> {
    let game = load(project)?;
    let cfg = settings::load_settings(cfg_dir);
    let (provider, model) = settings::build_provider(&cfg, cfg_dir)?;
    let opts = options(game, batch.unwrap_or(cfg.batch_size));
    let mut db = ProjectDb::open(project)?;
    let mut global = settings::open_global_tm(cfg_dir);
    eprintln!(
        "provider: {} ({model}), {} frases por chamada",
        cfg.provider, opts.batch_size
    );

    let cancel = Arc::new(AtomicBool::new(false));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("nao foi possivel iniciar a traducao: {e}"))?;
    let summary = runtime.block_on(async {
        let flag = cancel.clone();
        tokio::spawn(async move {
            // Primeiro Ctrl-C: para depois do lote em andamento, com o que ja
            // foi traduzido gravado. Segundo: encerra na hora.
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("cancelando depois do lote em andamento (Ctrl-C de novo encerra agora)");
                flag.store(true, Ordering::Relaxed);
            }
            if tokio::signal::ctrl_c().await.is_ok() {
                std::process::exit(130);
            }
        });
        run_translation(
            &mut db,
            global.as_mut(),
            provider.as_ref(),
            &model,
            &opts,
            &cancel,
            &|p| eprintln!("  {} de {} ({})", p.done, p.total, p.phase),
        )
        .await
    })?;

    let mut text = format!(
        "traduzidas pelo provider: {}, pela memoria: {}, falharam: {}",
        summary.translated, summary.tm_hits, summary.failed
    );
    if summary.shortened + summary.too_long > 0 {
        let _ = write!(
            text,
            "\nencurtadas na repergunta: {}, ainda maiores que o espaco da frase: {}",
            summary.shortened, summary.too_long
        );
    }
    if summary.input_tokens + summary.output_tokens > 0 {
        let _ = write!(
            text,
            "\ntokens cobrados: {} de entrada, {} de saida",
            summary.input_tokens, summary.output_tokens
        );
    }
    let unfinished = match &summary.stopped {
        Some(reason) => Some(format!(
            "a traducao parou, e o que nao foi tentado continua pendente: {reason}"
        )),
        None if summary.cancelled => {
            Some("traducao cancelada: o que nao foi tentado continua pendente".to_string())
        }
        None => None,
    };
    let mut json = serde_json::to_value(&summary)?;
    json["provider"] = json!(cfg.provider);
    json["model"] = json!(model);
    Ok(Output {
        text,
        json,
        unfinished,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_accepts_decimal_hex_and_open_sides() {
        assert_eq!(parse_region("16..0x20"), Ok((Some(16), Some(0x20))));
        assert_eq!(parse_region("0X10.."), Ok((Some(0x10), None)));
        assert_eq!(parse_region("..4096"), Ok((None, Some(4096))));
        for bad in ["", "10", "a..b", "0x..10", "-1..5", "1...5"] {
            assert!(parse_region(bad).is_err(), "{bad}");
        }
    }

    /// A CLI le a configuracao da pasta do app: o identificador daqui tem que
    /// ser o do `tauri.conf.json`.
    #[test]
    fn config_dir_is_the_app_one() {
        let conf: Value =
            serde_json::from_str(include_str!("../../desktop/src-tauri/tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], APP_IDENTIFIER);
    }

    #[test]
    fn arguments_are_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}

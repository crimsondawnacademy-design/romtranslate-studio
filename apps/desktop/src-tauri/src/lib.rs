use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use romtranslate_core::db::ProjectDb;
use romtranslate_core::detect::{inspect, InspectionReport};
use romtranslate_core::export;
use romtranslate_core::pipeline::{run_translation, TranslateOptions, TranslateSummary};
use romtranslate_core::project::{self, CreateProjectArgs, OpenProjectReport};
use romtranslate_core::provider::GlossaryTerm;
use romtranslate_core::relative::{self, RelativeCandidate};
use romtranslate_core::scan::{self, ScanConfig, ScanOutcome};
use romtranslate_core::settings::{
    self, anthropic, build_provider, codex_cli, open_global_tm, ApiKey, AppSettings,
};
use romtranslate_core::types::{GameProject, TextEntry};
use romtranslate_core::validate;
use romtranslate_core::xray::{self, XrayReport};

/// Flag de cancelamento da traducao em andamento (uma por vez).
struct TranslationState(Mutex<Option<Arc<AtomicBool>>>);

/// Flag de cancelamento do login do Codex em andamento (um por vez).
struct CodexLoginState(Mutex<Option<Arc<AtomicBool>>>);

/// Todo comando de IO roda em spawn_blocking: nao trava a UI.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> romtranslate_core::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("task falhou: {e}"))?
        .map_err(|e| e.to_string())
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

// ---- Sprint 1-2 ----

#[tauri::command]
async fn inspect_file(path: String) -> Result<InspectionReport, String> {
    blocking(move || inspect(&PathBuf::from(path))).await
}

#[tauri::command]
async fn create_project(args: CreateProjectArgs) -> Result<GameProject, String> {
    blocking(move || project::create_project(args)).await
}

#[tauri::command]
async fn open_project(dir: String) -> Result<OpenProjectReport, String> {
    blocking(move || project::open_project(&PathBuf::from(dir))).await
}

#[tauri::command]
async fn scan_project(project_dir: String, config: ScanConfig) -> Result<ScanOutcome, String> {
    blocking(move || scan::scan_project(&PathBuf::from(project_dir), &config)).await
}

#[tauri::command]
async fn export_entries(
    entries: Vec<TextEntry>,
    path: String,
    format: String,
) -> Result<String, String> {
    let out = PathBuf::from(path);
    blocking(move || {
        match format.as_str() {
            "json" => export::export_json(&entries, &out)?,
            "csv" => export::export_csv(&entries, &out)?,
            other => {
                return Err(romtranslate_core::CoreError::Project(format!(
                    "formato de export desconhecido: {other}"
                )))
            }
        }
        Ok(out.display().to_string())
    })
    .await
}

// ---- Descoberta: texto fora do alcance do scanner ASCII ----

#[tauri::command]
async fn relative_search(
    project_dir: String,
    word: String,
) -> Result<Vec<RelativeCandidate>, String> {
    blocking(move || relative::search_project(&PathBuf::from(project_dir), &word)).await
}

#[tauri::command]
async fn save_relative_tbl(
    project_dir: String,
    upper_base: Option<u8>,
    lower_base: Option<u8>,
) -> Result<String, String> {
    blocking(move || {
        let path = relative::save_tbl(&PathBuf::from(project_dir), upper_base, lower_base)?;
        Ok(path.display().to_string())
    })
    .await
}

#[tauri::command]
async fn xray_project(project_dir: String) -> Result<XrayReport, String> {
    blocking(move || xray::xray_project(&PathBuf::from(project_dir))).await
}

// ---- Sprint 3: persistencia de entries ----

#[tauri::command]
async fn save_entries(project_dir: String, entries: Vec<TextEntry>) -> Result<usize, String> {
    blocking(move || ProjectDb::open(&PathBuf::from(project_dir))?.upsert_entries(&entries)).await
}

#[tauri::command]
async fn load_entries(project_dir: String) -> Result<Vec<TextEntry>, String> {
    blocking(move || ProjectDb::open(&PathBuf::from(project_dir))?.load_entries()).await
}

// ---- Sprint 5: extracao estruturada + reinsercao ----

/// Imagens com letra do arquivo do projeto (`images::find`), com a folha de
/// cada uma gravada em `<projeto>/imagens`.
#[tauri::command]
async fn list_pictures(
    project_dir: String,
) -> Result<Vec<romtranslate_core::images::Picture>, String> {
    blocking(move || romtranslate_core::images::list_pictures(&PathBuf::from(project_dir))).await
}

/// O PNG da folha de uma imagem (gravado por `list_pictures`), ou o da folha
/// de trechos que foi pra IA (`leitura`).
#[tauri::command]
async fn picture_png(project_dir: String, id: String, reading: bool) -> Result<Vec<u8>, String> {
    // O id vem da tela: so nome de arquivo, nada de caminho.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("id de imagem invalido: {id}"));
    }
    let name = if reading {
        format!("{id}-leitura.png")
    } else {
        format!("{id}.png")
    };
    let path = PathBuf::from(project_dir)
        .join(romtranslate_core::images::PICTURES_DIR)
        .join(name);
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A IA le o texto das imagens marcadas; cada trecho com japones vira frase.
/// `preview`: so grava a folha de trechos que iria pra IA (`<id>-leitura.png`),
/// sem chamar ninguem. `answer_path`: a leitura vem desse arquivo (o JSON que
/// a IA devolveria pra folha de trechos), de uma imagem por vez.
#[tauri::command]
async fn read_pictures(
    app: AppHandle,
    project_dir: String,
    ids: Vec<String>,
    preview: Option<bool>,
    answer_path: Option<String>,
) -> Result<romtranslate_core::images::ReadOutcome, String> {
    use romtranslate_core::images::{read_pictures, Reader};
    let dir = PathBuf::from(project_dir);
    if let Some(path) = answer_path {
        if ids.len() != 1 {
            return Err(format!(
                "a leitura de um arquivo vale pra uma imagem por vez (vieram {})",
                ids.len()
            ));
        }
        let answer = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        return read_pictures(&dir, Reader::Answer(&answer), &ids)
            .await
            .map_err(|e| e.to_string());
    }
    if preview.unwrap_or(false) {
        return read_pictures(&dir, Reader::Preview, &ids)
            .await
            .map_err(|e| e.to_string());
    }
    let cfg_dir = config_dir(&app)?;
    let cfg = settings::load_settings(&cfg_dir);
    let (provider, _) = build_provider(&cfg, &cfg_dir)?;
    read_pictures(&dir, Reader::Provider(provider.as_ref()), &ids)
        .await
        .map_err(|e| e.to_string())
}

/// Marca imagens como conferidas, sem texto pra traduzir: a auditoria para
/// de cobrar por elas. Devolve quantas sao novas.
#[tauri::command]
async fn mark_pictures_without_text(
    project_dir: String,
    ids: Vec<String>,
) -> Result<usize, String> {
    blocking(move || romtranslate_core::audit::mark_without_text(&PathBuf::from(project_dir), &ids))
        .await
}

/// Le a copia traduzida de novo e conta o que segue no idioma de origem.
#[tauri::command]
async fn audit_project(
    project_dir: String,
) -> Result<romtranslate_core::audit::AuditReport, String> {
    blocking(move || romtranslate_core::audit::audit_project(&PathBuf::from(project_dir))).await
}

/// As fontes do perfil do jogo (`perfil.toml`) e as frases de cada uma;
/// `None` = o projeto nao tem perfil.
#[tauri::command]
async fn profile_summary(
    project_dir: String,
) -> Result<Option<Vec<romtranslate_core::profile::SourceSummary>>, String> {
    blocking(move || {
        let dir = PathBuf::from(project_dir);
        let entries = ProjectDb::open(&dir)?.load_entries()?;
        romtranslate_core::profile::summary(&dir, &entries)
    })
    .await
}

/// A regiao e a tabela do ultimo scan, guardadas no projeto.
#[tauri::command]
async fn last_scan(project_dir: String) -> Result<Option<ScanConfig>, String> {
    blocking(move || Ok(scan::last_scan(&PathBuf::from(project_dir)))).await
}

/// Alfabeto ensinado depois de um comando do jogo.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Alphabet {
    command: u16,
    letters: String,
}

#[tauri::command]
async fn list_alphabets(project_dir: String) -> Result<Vec<Alphabet>, String> {
    blocking(move || {
        Ok(
            romtranslate_core::adapters::alphabets(&PathBuf::from(project_dir))?
                .into_iter()
                .map(|(command, letters)| Alphabet { command, letters })
                .collect(),
        )
    })
    .await
}

/// Grava o alfabeto (letras dos codigos 1, 2, 3...); vazio tira do projeto.
#[tauri::command]
async fn save_alphabet(
    project_dir: String,
    command: u16,
    letters: String,
) -> Result<String, String> {
    blocking(move || {
        let path = romtranslate_core::adapters::save_alphabet(
            &PathBuf::from(project_dir),
            command,
            &letters,
        )?;
        Ok(path.display().to_string())
    })
    .await
}

#[tauri::command]
async fn extract_structured(
    project_dir: String,
) -> Result<romtranslate_core::adapters::ExtractOutcome, String> {
    blocking(move || romtranslate_core::adapters::extract_project(&PathBuf::from(project_dir)))
        .await
}

#[tauri::command]
async fn reinsert_project(
    project_dir: String,
    policy: romtranslate_core::reinsert::ErrorPolicy,
) -> Result<romtranslate_core::reinsert::ReinsertOutcome, String> {
    blocking(move || {
        romtranslate_core::reinsert::reinsert_project_with(&PathBuf::from(project_dir), policy)
    })
    .await
}

#[tauri::command]
async fn export_patch(
    project_dir: String,
    format: Option<romtranslate_core::patch::PatchFormat>,
) -> Result<romtranslate_core::patch::PatchExportOutcome, String> {
    blocking(move || romtranslate_core::patch::export_patch(&PathBuf::from(project_dir), format))
        .await
}

// ---- Sprint 4: editor + validacao ----

#[tauri::command]
async fn update_entry(
    app: AppHandle,
    project_dir: String,
    id: String,
    translation: String,
) -> Result<Vec<validate::ValidationIssue>, String> {
    let cfg_dir = config_dir(&app)?;
    blocking(move || {
        let dir = PathBuf::from(&project_dir);
        let game = project::load_project(&dir)?;
        let mut db = ProjectDb::open(&dir)?;
        let mut global = open_global_tm(&cfg_dir);
        validate::apply_manual_translation(
            &mut db,
            global.as_mut(),
            &id,
            &translation,
            game.source_language.as_deref().unwrap_or(""),
            &game.target_language,
        )
    })
    .await
}

#[tauri::command]
async fn set_entry_reviewed(
    project_dir: String,
    id: String,
    reviewed: bool,
) -> Result<String, String> {
    blocking(move || {
        let mut db = ProjectDb::open(&PathBuf::from(project_dir))?;
        let status = validate::set_reviewed(&mut db, &id, reviewed)?;
        Ok(serde_json::to_string(&status)?
            .trim_matches('"')
            .to_string())
    })
    .await
}

#[tauri::command]
async fn unmark_internal(project_dir: String, id: String) -> Result<(), String> {
    blocking(move || {
        let mut db = ProjectDb::open(&PathBuf::from(project_dir))?;
        validate::unmark_internal(&mut db, &id)
    })
    .await
}

/// Marca frases como nao-texto: as `ids` e, com `kind`, todas com esse erro
/// de validacao. Devolve quantas mudaram.
#[tauri::command]
async fn mark_not_text(
    project_dir: String,
    ids: Vec<String>,
    kind: Option<validate::IssueKind>,
) -> Result<usize, String> {
    blocking(move || {
        let mut db = ProjectDb::open(&PathBuf::from(project_dir))?;
        Ok(validate::mark_not_text(&mut db, &ids, kind)?.0)
    })
    .await
}

#[tauri::command]
async fn validate_project(project_dir: String) -> Result<validate::ValidationReport, String> {
    blocking(move || {
        let mut db = ProjectDb::open(&PathBuf::from(project_dir))?;
        validate::validate_project_db(&mut db)
    })
    .await
}

// ---- Sprint 3: glossario ----

#[tauri::command]
async fn glossary_list(project_dir: String) -> Result<Vec<GlossaryTerm>, String> {
    blocking(move || ProjectDb::open(&PathBuf::from(project_dir))?.glossary_list()).await
}

#[tauri::command]
async fn glossary_upsert(project_dir: String, term: GlossaryTerm) -> Result<(), String> {
    blocking(move || ProjectDb::open(&PathBuf::from(project_dir))?.glossary_upsert(&term)).await
}

#[tauri::command]
async fn glossary_delete(project_dir: String, term: String) -> Result<(), String> {
    blocking(move || ProjectDb::open(&PathBuf::from(project_dir))?.glossary_delete(&term)).await
}

// ---- Sprint 3: settings ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsReport {
    settings: AppSettings,
    api_key_set: bool,
    anthropic_key_set: bool,
}

fn settings_report(dir: &Path, settings: AppSettings) -> SettingsReport {
    SettingsReport {
        settings,
        api_key_set: settings::load_api_key(dir, ApiKey::OpenAi).is_some(),
        anthropic_key_set: settings::load_api_key(dir, ApiKey::Anthropic).is_some(),
    }
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<SettingsReport, String> {
    let dir = config_dir(&app)?;
    Ok(settings_report(&dir, settings::load_settings(&dir)))
}

/// `api_key` / `anthropic_key`: `None` mantem a chave que ja esta guardada.
#[tauri::command]
fn save_settings(
    app: AppHandle,
    new_settings: AppSettings,
    api_key: Option<String>,
    anthropic_key: Option<String>,
) -> Result<SettingsReport, String> {
    let dir = config_dir(&app)?;
    settings::save_settings(&dir, &new_settings)?;
    if let Some(key) = api_key {
        settings::save_api_key(&dir, ApiKey::OpenAi, &key)?;
    }
    if let Some(key) = anthropic_key {
        settings::save_api_key(&dir, ApiKey::Anthropic, &key)?;
    }
    Ok(settings_report(&dir, new_settings))
}

#[tauri::command]
async fn test_provider(app: AppHandle) -> Result<String, String> {
    let dir = config_dir(&app)?;
    let cfg = settings::load_settings(&dir);
    let (provider, model) = build_provider(&cfg, &dir)?;
    provider.health_check().await.map_err(|e| e.to_string())?;
    Ok(model)
}

/// O tamanho da rodada que "Traduzir pendentes" faria agora, em tokens.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EstimateReport {
    entries: usize,
    batches: usize,
    /// Chamadas de margem pelas reperguntas.
    retry_batches: usize,
    /// Da entrada, os tokens das instrucoes que o provider poe em cada chamada.
    overhead_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    /// `true` = a conta de tokens foi calibrada pelo proprio modelo (API do
    /// Claude); `false` = media de caracteres por token.
    calibrated: bool,
}

#[tauri::command]
async fn estimate_translation(
    app: AppHandle,
    project_dir: String,
) -> Result<EstimateReport, String> {
    let cfg_dir = config_dir(&app)?;
    let cfg = settings::load_settings(&cfg_dir);
    let options_cfg = cfg.clone();
    let (estimate, sample) = blocking(move || {
        let dir = PathBuf::from(project_dir);
        let opts = translate_options(project::load_project(&dir)?, &options_cfg, false);
        romtranslate_core::pipeline::estimate_translation(&ProjectDb::open(&dir)?, &opts)
    })
    .await?;

    // Com a chave do Claude salva, o proprio modelo conta o primeiro lote (de
    // graca) e a media de caracteres por token sai dele, nao de um chute.
    let mut chars_per_token = romtranslate_core::pipeline::DEFAULT_CHARS_PER_TOKEN;
    let mut calibrated = false;
    if let (true, Some(sample)) = (cfg.provider == "anthropic", &sample) {
        if let Ok(provider) = anthropic(&cfg, &cfg_dir) {
            let chars = romtranslate_core::provider::build_system_prompt(sample).len()
                + romtranslate_core::provider::build_user_prompt(sample).len();
            if let Ok(tokens) = provider.count_input_tokens(sample).await {
                if tokens > 0 {
                    chars_per_token = chars as f64 / tokens as f64;
                    calibrated = true;
                }
            }
        }
    }
    let (input_tokens, output_tokens) = estimate.tokens(chars_per_token, &cfg.provider);
    Ok(EstimateReport {
        entries: estimate.entries,
        batches: estimate.batches,
        retry_batches: estimate.retry_batches,
        overhead_tokens: (estimate.batches + estimate.retry_batches) as u64
            * romtranslate_core::pipeline::call_overhead_tokens(&cfg.provider),
        input_tokens,
        output_tokens,
        calibrated,
    })
}

/// As opcoes da rodada com as configuracoes do app: a mesma conta pra medir,
/// exportar os lotes e traduzir.
fn translate_options(game: GameProject, cfg: &AppSettings, redo_errors: bool) -> TranslateOptions {
    let mut opts = TranslateOptions::new(game.target_language);
    opts.source_language = game.source_language;
    opts.batch_size = cfg.batch_size.clamp(1, 50);
    opts.parallel = cfg.parallel.max(1);
    opts.shorten_rounds = cfg.shorten_rounds;
    opts.redo_errors = redo_errors;
    opts
}

/// Grava em `path` os lotes exatos da rodada de agora (prompt e frases, em
/// JSON): pra traduzir fora do app. Devolve quantos lotes.
#[tauri::command]
async fn export_requests(
    app: AppHandle,
    project_dir: String,
    path: String,
    redo_errors: Option<bool>,
) -> Result<usize, String> {
    let cfg = settings::load_settings(&config_dir(&app)?);
    blocking(move || {
        let dir = PathBuf::from(project_dir);
        let opts = translate_options(
            project::load_project(&dir)?,
            &cfg,
            redo_errors.unwrap_or(false),
        );
        romtranslate_core::pipeline::write_requests(
            &ProjectDb::open(&dir)?,
            &opts,
            Path::new(&path),
        )
    })
    .await
}

/// Grava as traducoes de um TSV (`id<TAB>traducao`). Devolve quantas.
#[tauri::command]
async fn import_translations(project_dir: String, path: String) -> Result<usize, String> {
    blocking(move || {
        let mut db = ProjectDb::open(&PathBuf::from(project_dir))?;
        export::import_tsv(&mut db, Path::new(&path))
    })
    .await
}

// ---- Conta do ChatGPT pelo Codex CLI ----

/// Ha login na conta escolhida? (A do computador ou a do app.)
#[tauri::command]
async fn codex_account_status(
    app: AppHandle,
    command: String,
    own_account: bool,
) -> Result<bool, String> {
    codex_cli(&config_dir(&app)?, &command, "", own_account, 30)?
        .logged_in()
        .await
        .map_err(|e| e.to_string())
}

/// Abre o login do ChatGPT no navegador pra conta DO APP e espera o usuario
/// concluir. A conta do computador nunca e trocada daqui.
#[tauri::command]
async fn codex_login(
    app: AppHandle,
    state: State<'_, CodexLoginState>,
    command: String,
) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("ja existe um login em andamento".to_string());
        }
        *guard = Some(cancel.clone());
    }
    let provider = codex_cli(&config_dir(&app)?, &command, "", true, 30);
    let result = match provider {
        Ok(provider) => provider.login(&cancel).await.map_err(|e| e.to_string()),
        Err(e) => Err(e),
    };
    if let Ok(mut guard) = state.0.lock() {
        *guard = None;
    }
    result
}

#[tauri::command]
fn codex_login_cancel(state: State<'_, CodexLoginState>) -> Result<(), String> {
    if let Some(flag) = state.0.lock().map_err(|e| e.to_string())?.as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Apaga o login da conta DO APP.
#[tauri::command]
async fn codex_logout(app: AppHandle, command: String) -> Result<(), String> {
    codex_cli(&config_dir(&app)?, &command, "", true, 30)?
        .logout()
        .await
        .map_err(|e| e.to_string())
}

// ---- Sprint 3: traducao ----

#[tauri::command]
async fn translate_project(
    app: AppHandle,
    state: State<'_, TranslationState>,
    project_dir: String,
    redo_errors: Option<bool>,
) -> Result<TranslateSummary, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("ja existe uma traducao em andamento".to_string());
        }
        *guard = Some(cancel.clone());
    }

    let result = do_translate(&app, &project_dir, &cancel, redo_errors.unwrap_or(false)).await;

    if let Ok(mut guard) = state.0.lock() {
        *guard = None;
    }
    result
}

async fn do_translate(
    app: &AppHandle,
    project_dir: &str,
    cancel: &AtomicBool,
    redo_errors: bool,
) -> Result<TranslateSummary, String> {
    let dir = PathBuf::from(project_dir);
    let cfg_dir = config_dir(app)?;
    let cfg = settings::load_settings(&cfg_dir);
    let (provider, model) = build_provider(&cfg, &cfg_dir)?;

    let game = project::load_project(&dir).map_err(|e| e.to_string())?;
    let opts = translate_options(game, &cfg, redo_errors);

    let mut db = ProjectDb::open(&dir).map_err(|e| e.to_string())?;
    let mut global = open_global_tm(&cfg_dir);
    let emitter = app.clone();
    run_translation(
        &mut db,
        global.as_mut(),
        provider.as_ref(),
        &model,
        &opts,
        cancel,
        &move |p| {
            let _ = emitter.emit("translation-progress", &p);
        },
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_translation(state: State<'_, TranslationState>) -> Result<(), String> {
    if let Some(flag) = state.0.lock().map_err(|e| e.to_string())?.as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(TranslationState(Mutex::new(None)))
        .manage(CodexLoginState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            list_pictures,
            picture_png,
            read_pictures,
            mark_pictures_without_text,
            audit_project,
            profile_summary,
            last_scan,
            list_alphabets,
            save_alphabet,
            mark_not_text,
            export_requests,
            import_translations,
            inspect_file,
            create_project,
            open_project,
            scan_project,
            relative_search,
            save_relative_tbl,
            xray_project,
            export_entries,
            save_entries,
            load_entries,
            extract_structured,
            reinsert_project,
            export_patch,
            update_entry,
            set_entry_reviewed,
            unmark_internal,
            validate_project,
            glossary_list,
            glossary_upsert,
            glossary_delete,
            get_settings,
            save_settings,
            test_provider,
            estimate_translation,
            translate_project,
            cancel_translation,
            codex_account_status,
            codex_login,
            codex_login_cancel,
            codex_logout
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

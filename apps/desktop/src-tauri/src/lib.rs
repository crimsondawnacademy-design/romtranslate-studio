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
    let batch_size = cfg.batch_size.clamp(1, 50);
    let (estimate, sample) = blocking(move || {
        let dir = PathBuf::from(project_dir);
        let game = project::load_project(&dir)?;
        let mut opts = TranslateOptions::new(game.target_language);
        opts.source_language = game.source_language;
        opts.batch_size = batch_size;
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
    let tokens = |chars: usize| (chars as f64 / chars_per_token).ceil() as u64;
    Ok(EstimateReport {
        entries: estimate.entries,
        batches: estimate.batches,
        input_tokens: tokens(estimate.input_chars),
        output_tokens: tokens(estimate.output_chars),
        calibrated,
    })
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
) -> Result<TranslateSummary, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("ja existe uma traducao em andamento".to_string());
        }
        *guard = Some(cancel.clone());
    }

    let result = do_translate(&app, &project_dir, &cancel).await;

    if let Ok(mut guard) = state.0.lock() {
        *guard = None;
    }
    result
}

async fn do_translate(
    app: &AppHandle,
    project_dir: &str,
    cancel: &AtomicBool,
) -> Result<TranslateSummary, String> {
    let dir = PathBuf::from(project_dir);
    let cfg_dir = config_dir(app)?;
    let cfg = settings::load_settings(&cfg_dir);
    let (provider, model) = build_provider(&cfg, &cfg_dir)?;

    let game = project::load_project(&dir).map_err(|e| e.to_string())?;
    let mut opts = TranslateOptions::new(game.target_language);
    opts.source_language = game.source_language;
    opts.batch_size = cfg.batch_size.clamp(1, 50);

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

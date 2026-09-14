mod settings;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use romtranslate_core::db::ProjectDb;
use romtranslate_core::detect::{inspect, InspectionReport};
use romtranslate_core::export;
use romtranslate_core::pipeline::{run_translation, TranslateOptions, TranslateSummary};
use romtranslate_core::project::{self, CreateProjectArgs, OpenProjectReport};
use romtranslate_core::provider::{GlossaryTerm, TranslationProvider};
use romtranslate_core::providers::ollama::OllamaProvider;
use romtranslate_core::providers::openai_compat::OpenAiCompatProvider;
use romtranslate_core::scan::{self, ScanConfig, ScanOutcome};
use romtranslate_core::types::{GameProject, TextEntry};
use romtranslate_core::validate;

use settings::AppSettings;

/// Flag de cancelamento da traducao em andamento (uma por vez).
struct TranslationState(Mutex<Option<Arc<AtomicBool>>>);

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

fn is_localhost(url: &str) -> bool {
    ["://localhost", "://127.", "://0.0.0.0", "://[::1]"]
        .iter()
        .any(|h| url.contains(h))
}

/// Monta o provider a partir dos settings, aplicando a guarda de privacidade
/// (spec §18: traducao remota so com allow_remote_translation).
fn build_provider(
    cfg: &AppSettings,
    api_key: Option<String>,
) -> Result<(Box<dyn TranslationProvider>, String), String> {
    match cfg.provider.as_str() {
        "ollama" => {
            let p = OllamaProvider::new(&cfg.ollama.base_url, &cfg.ollama.model, cfg.timeout_secs)
                .map_err(|e| e.to_string())?;
            Ok((Box::new(p), cfg.ollama.model.clone()))
        }
        "openai_compatible" => {
            let base = &cfg.openai_compatible.base_url;
            if !cfg.allow_remote_translation && !is_localhost(base) {
                return Err(
                    "traducao remota desabilitada: ative 'permitir traducao remota' nas \
                     configuracoes para usar um endpoint fora do localhost"
                        .to_string(),
                );
            }
            let p = OpenAiCompatProvider::new(
                base,
                &cfg.openai_compatible.model,
                api_key,
                cfg.timeout_secs,
            )
            .map_err(|e| e.to_string())?;
            Ok((Box::new(p), cfg.openai_compatible.model.clone()))
        }
        other => Err(format!("provider desconhecido: {other}")),
    }
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
async fn scan_file(path: String, config: ScanConfig) -> Result<ScanOutcome, String> {
    blocking(move || scan::scan_file(&PathBuf::from(path), &config)).await
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
async fn extract_structured(project_dir: String) -> Result<usize, String> {
    blocking(move || {
        let dir = PathBuf::from(&project_dir);
        let game = project::load_project(&dir)?;
        let adapter = romtranslate_core::adapters::find(&game.adapter_id).ok_or_else(|| {
            romtranslate_core::CoreError::Project(format!("adapter {} nao existe", game.adapter_id))
        })?;
        let size = std::fs::metadata(&game.source_path)
            .map_err(|e| romtranslate_core::CoreError::io(&game.source_path, e))?
            .len();
        if size > romtranslate_core::adapter::MAX_FILE_SIZE {
            return Err(romtranslate_core::CoreError::FileTooLarge {
                size,
                limit: romtranslate_core::adapter::MAX_FILE_SIZE,
            });
        }
        let data = std::fs::read(&game.source_path)
            .map_err(|e| romtranslate_core::CoreError::io(&game.source_path, e))?;
        let entries = adapter.extract_structured(&data)?;
        ProjectDb::open(&dir)?.upsert_entries(&entries)
    })
    .await
}

#[tauri::command]
async fn reinsert_project(
    project_dir: String,
    allow_errors: bool,
) -> Result<romtranslate_core::reinsert::ReinsertOutcome, String> {
    blocking(move || {
        romtranslate_core::reinsert::reinsert_project(&PathBuf::from(project_dir), allow_errors)
    })
    .await
}

// ---- Sprint 4: editor + validacao ----

#[tauri::command]
async fn update_entry(
    project_dir: String,
    id: String,
    translation: String,
) -> Result<Vec<validate::ValidationIssue>, String> {
    blocking(move || {
        let dir = PathBuf::from(&project_dir);
        let game = project::load_project(&dir)?;
        let mut db = ProjectDb::open(&dir)?;
        validate::apply_manual_translation(
            &mut db,
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
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<SettingsReport, String> {
    let dir = config_dir(&app)?;
    Ok(SettingsReport {
        settings: settings::load_settings(&dir),
        api_key_set: settings::load_api_key(&dir).is_some(),
    })
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    new_settings: AppSettings,
    api_key: Option<String>,
) -> Result<SettingsReport, String> {
    let dir = config_dir(&app)?;
    settings::save_settings(&dir, &new_settings)?;
    if let Some(key) = api_key {
        settings::save_api_key(&dir, &key)?;
    }
    Ok(SettingsReport {
        settings: new_settings,
        api_key_set: settings::load_api_key(&dir).is_some(),
    })
}

#[tauri::command]
async fn test_provider(app: AppHandle) -> Result<String, String> {
    let dir = config_dir(&app)?;
    let cfg = settings::load_settings(&dir);
    let api_key = settings::load_api_key(&dir);
    let (provider, model) = build_provider(&cfg, api_key)?;
    provider.health_check().await.map_err(|e| e.to_string())?;
    Ok(model)
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
    let api_key = settings::load_api_key(&cfg_dir);
    let (provider, model) = build_provider(&cfg, api_key)?;

    let game = project::load_project(&dir).map_err(|e| e.to_string())?;
    let mut opts = TranslateOptions::new(game.target_language);
    opts.source_language = game.source_language;
    opts.batch_size = cfg.batch_size.clamp(1, 50);

    let mut db = ProjectDb::open(&dir).map_err(|e| e.to_string())?;
    let emitter = app.clone();
    run_translation(
        &mut db,
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
        .invoke_handler(tauri::generate_handler![
            inspect_file,
            create_project,
            open_project,
            scan_file,
            export_entries,
            save_entries,
            load_entries,
            extract_structured,
            reinsert_project,
            update_entry,
            set_entry_reviewed,
            validate_project,
            glossary_list,
            glossary_upsert,
            glossary_delete,
            get_settings,
            save_settings,
            test_provider,
            translate_project,
            cancel_translation
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

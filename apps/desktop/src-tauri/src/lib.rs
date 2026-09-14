use std::path::PathBuf;

use romtranslate_core::detect::{inspect, InspectionReport};
use romtranslate_core::export;
use romtranslate_core::project::{self, CreateProjectArgs, OpenProjectReport};
use romtranslate_core::scan::{self, ScanConfig, ScanOutcome};
use romtranslate_core::types::{GameProject, TextEntry};

/// Todo comando roda em spawn_blocking: hash/scan de arquivos grandes nao pode
/// travar a UI.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> romtranslate_core::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("task falhou: {e}"))?
        .map_err(|e| e.to_string())
}

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
        .invoke_handler(tauri::generate_handler![
            inspect_file,
            create_project,
            open_project,
            scan_file,
            export_entries
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

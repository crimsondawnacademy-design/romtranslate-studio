use std::path::PathBuf;

use romtranslate_core::detect::{inspect, InspectionReport};
use romtranslate_core::project::{self, CreateProjectArgs};
use romtranslate_core::types::GameProject;

/// Comandos rodam em spawn_blocking: hash de arquivos grandes nao pode travar a UI.
#[tauri::command]
async fn inspect_file(path: String) -> Result<InspectionReport, String> {
    tauri::async_runtime::spawn_blocking(move || inspect(&PathBuf::from(path)))
        .await
        .map_err(|e| format!("task falhou: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_project(args: CreateProjectArgs) -> Result<GameProject, String> {
    tauri::async_runtime::spawn_blocking(move || project::create_project(args))
        .await
        .map_err(|e| format!("task falhou: {e}"))?
        .map_err(|e| e.to_string())
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
        .invoke_handler(tauri::generate_handler![inspect_file, create_project])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

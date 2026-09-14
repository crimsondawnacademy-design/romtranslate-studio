use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::error::{CoreError, Result};
use crate::hash::sha256_file;
use crate::types::{GameProject, Platform, ProjectStatus};

pub const PROJECT_FILE: &str = "project.json";
pub const PROJECT_DIR_EXT: &str = "rtsproj";
const SUBDIRS: [&str; 4] = ["cache", "extracted", "working", "exports"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectArgs {
    pub source_path: PathBuf,
    pub platform: Platform,
    pub adapter_id: String,
    pub source_language: Option<String>,
    pub target_language: String,
    /// Diretorio do projeto (sera criado). Ex.: `/roms/MyGame.rtsproj`.
    pub project_dir: PathBuf,
}

/// Cria um projeto local: diretorio `.rtsproj` + `project.json`.
/// O arquivo original NUNCA e copiado nem alterado — guardamos path + SHA-256.
pub fn create_project(args: CreateProjectArgs) -> Result<GameProject> {
    if !args.source_path.is_file() {
        return Err(CoreError::Project(format!(
            "arquivo de origem nao encontrado: {}",
            args.source_path.display()
        )));
    }
    let manifest_path = args.project_dir.join(PROJECT_FILE);
    if manifest_path.exists() {
        return Err(CoreError::Project(format!(
            "ja existe um projeto em {}",
            args.project_dir.display()
        )));
    }

    let source_sha256 = sha256_file(&args.source_path)?;
    let source_size = fs::metadata(&args.source_path)
        .map_err(|e| CoreError::io(&args.source_path, e))?
        .len();

    let project = GameProject {
        id: Uuid::new_v4(),
        source_path: args.source_path,
        source_sha256,
        source_size,
        platform: args.platform,
        adapter_id: args.adapter_id,
        source_language: args.source_language,
        target_language: args.target_language,
        status: ProjectStatus::Created,
        created_at: Utc::now(),
    };

    for sub in SUBDIRS {
        fs::create_dir_all(args.project_dir.join(sub))
            .map_err(|e| CoreError::io(&args.project_dir, e))?;
    }
    write_json_atomic(&manifest_path, &project)?;

    info!(project_id = %project.id, dir = %args.project_dir.display(), "projeto criado");
    Ok(project)
}

/// Reabre um projeto a partir do diretorio `.rtsproj`.
pub fn load_project(project_dir: &Path) -> Result<GameProject> {
    let manifest_path = project_dir.join(PROJECT_FILE);
    let data = fs::read_to_string(&manifest_path).map_err(|e| CoreError::io(&manifest_path, e))?;
    Ok(serde_json::from_str(&data)?)
}

/// Escrita atomica: escreve num `.tmp` e faz rename (mesmo filesystem).
fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value)?;
    fs::write(&tmp, json).map_err(|e| CoreError::io(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| CoreError::io(path, e))?;
    Ok(())
}

//! Orquestracao da reinsercao (spec §15): verifica origem, valida traducoes,
//! aplica via adapter numa WORKING COPY (o original nunca e tocado), grava
//! atomicamente, rele do disco e roda verify().

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tracing::info;

use crate::adapter::{ApplyReport, VerificationReport, IN_MEMORY_MAX};
use crate::adapters;
use crate::db::ProjectDb;
use crate::error::{CoreError, Result};
use crate::hash::sha256_file;
use crate::project::load_project;
use crate::types::TranslationStatus;
use crate::validate::validate_project_db;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReinsertOutcome {
    pub working_path: PathBuf,
    pub apply: ApplyReport,
    pub verification: VerificationReport,
    /// Entries com status Error que entraram mesmo assim (so no modo avancado).
    pub forced_errors: usize,
}

/// Reinsercao completa. `allow_errors` e o "modo avancado" da spec §14:
/// sem ele, qualquer entry com status Error bloqueia tudo.
pub fn reinsert_project(project_dir: &Path, allow_errors: bool) -> Result<ReinsertOutcome> {
    let project = load_project(project_dir)?;

    // 1. Origem existe e nao mudou desde a criacao do projeto.
    if !project.source_path.is_file() {
        return Err(CoreError::Project(format!(
            "arquivo de origem nao encontrado: {}",
            project.source_path.display()
        )));
    }
    if sha256_file(&project.source_path)? != project.source_sha256 {
        return Err(CoreError::Project(
            "o arquivo de origem mudou desde a criacao do projeto (SHA-256 diferente); \
             reinsercao abortada para nao corromper"
                .to_string(),
        ));
    }

    // 2. Validacao fresca + bloqueio de erros criticos.
    let mut db = ProjectDb::open(project_dir)?;
    validate_project_db(&mut db)?;
    let entries = db.load_entries()?;
    let error_count = entries
        .iter()
        .filter(|e| e.status == TranslationStatus::Error)
        .count();
    if error_count > 0 && !allow_errors {
        return Err(CoreError::Project(format!(
            "{error_count} entries com erro de validacao; corrija-as (ou use o modo avancado) \
             antes de reinserir"
        )));
    }

    // 3. Adapter declarado no projeto precisa suportar reinsercao.
    let adapter = adapters::find(&project.adapter_id)
        .ok_or_else(|| CoreError::Project(format!("adapter {} nao existe", project.adapter_id)))?;
    if !adapter.capabilities().reinsert {
        return Err(CoreError::Project(format!(
            "adapter {} nao suporta reinsercao automatica ainda",
            adapter.id()
        )));
    }

    // 4. Aplica em memoria (all-or-nothing) sobre os bytes do original.
    let size = fs::metadata(&project.source_path)
        .map_err(|e| CoreError::io(&project.source_path, e))?
        .len();
    if size > IN_MEMORY_MAX {
        return Err(CoreError::FileTooLarge {
            size,
            limit: IN_MEMORY_MAX,
        });
    }
    let original =
        fs::read(&project.source_path).map_err(|e| CoreError::io(&project.source_path, e))?;
    let applied = adapter.apply_text(&original, &entries)?;

    // 5. Grava working copy atomicamente (nunca o original).
    let working_dir = project_dir.join("working");
    fs::create_dir_all(&working_dir).map_err(|e| CoreError::io(&working_dir, e))?;
    let file_name = project
        .source_path
        .file_name()
        .ok_or_else(|| CoreError::Project("origem sem nome de arquivo".to_string()))?;
    let working_path = working_dir.join(file_name);
    let tmp = working_path.with_extension("tmp");
    fs::write(&tmp, &applied.bytes).map_err(|e| CoreError::io(&tmp, e))?;
    fs::rename(&tmp, &working_path).map_err(|e| CoreError::io(&working_path, e))?;

    // 6. Rele DO DISCO e verifica — o que foi persistido e o que vale.
    let written = fs::read(&working_path).map_err(|e| CoreError::io(&working_path, e))?;
    let verification = adapter.verify(&written)?;
    if !verification.ok {
        return Err(CoreError::Project(format!(
            "verificacao da working copy falhou: {} (arquivo mantido em {} para inspecao)",
            verification.problems.join("; "),
            working_path.display()
        )));
    }

    info!(
        working = %working_path.display(),
        applied = applied.report.applied,
        "reinsercao concluida e verificada"
    );
    Ok(ReinsertOutcome {
        working_path,
        apply: applied.report,
        verification,
        forced_errors: if allow_errors { error_count } else { 0 },
    })
}

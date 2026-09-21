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
use crate::fileio::read_view;
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
    reinsert_project_with_limit(project_dir, allow_errors, IN_MEMORY_MAX)
}

/// Igual a `reinsert_project`, com o teto em memoria injetavel — acima dele
/// a aplicacao vira streaming (copia + escrita pontual). Exposto pra testar
/// o caminho streaming sem precisar de um DVD real de 4.7 GiB.
pub fn reinsert_project_with_limit(
    project_dir: &Path,
    allow_errors: bool,
    in_memory_max: u64,
) -> Result<ReinsertOutcome> {
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

    // 4. Aplica sobre os bytes do original (all-or-nothing) e grava a
    //    working copy atomicamente (nunca o original). Ate o teto: tudo em
    //    memoria via adapter. Acima: streaming (copia + escrita pontual).
    let size = fs::metadata(&project.source_path)
        .map_err(|e| CoreError::io(&project.source_path, e))?
        .len();
    let working_dir = project_dir.join("working");
    fs::create_dir_all(&working_dir).map_err(|e| CoreError::io(&working_dir, e))?;
    let file_name = project
        .source_path
        .file_name()
        .ok_or_else(|| CoreError::Project("origem sem nome de arquivo".to_string()))?;
    let working_path = working_dir.join(file_name);
    let tmp = working_path.with_extension("tmp");

    let apply_report = if size <= in_memory_max {
        let original =
            fs::read(&project.source_path).map_err(|e| CoreError::io(&project.source_path, e))?;
        let applied = adapter.apply_text(&original, &entries)?;
        fs::write(&tmp, &applied.bytes).map_err(|e| CoreError::io(&tmp, e))?;
        applied.report
    } else {
        stream_apply_iso(&project.source_path, &entries, &tmp, in_memory_max)?
    };
    fs::rename(&tmp, &working_path).map_err(|e| CoreError::io(&working_path, e))?;

    // 6. Rele DO DISCO (mmap) e verifica — o que foi persistido e o que vale.
    let written = read_view(&working_path)?;
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
        applied = apply_report.applied,
        "reinsercao concluida e verificada"
    );
    Ok(ReinsertOutcome {
        working_path,
        apply: apply_report,
        verification,
        forced_errors: if allow_errors { error_count } else { 0 },
    })
}

/// Reinsercao streaming pra imagens acima do teto em memoria: mmap da
/// origem pra validar e planejar, copia do arquivo (clone barato no APFS) e
/// escrita so dos trechos alterados. So ISO 9660 2048/setor (DVD de PS2):
/// formato sem checksum global nem EDC/ECC de setor — raw 2352 e CD, cabe
/// com folga no caminho em memoria.
fn stream_apply_iso(
    source: &Path,
    entries: &[crate::types::TextEntry],
    dest_tmp: &Path,
    in_memory_max: u64,
) -> Result<ApplyReport> {
    use std::io::{Seek, SeekFrom, Write};

    use crate::adapters::iso9660::{detect_map, parse_pvd, SectorMap};

    let view = read_view(source)?;
    if detect_map(&view) != SectorMap::Plain2048 || parse_pvd(&view, SectorMap::Plain2048).is_err()
    {
        return Err(CoreError::Project(format!(
            "imagem de {} bytes passa do teto em memoria ({in_memory_max} bytes) e a \
             reinsercao streaming so cobre ISO 9660 2048 bytes/setor (DVD de PS2)",
            view.len()
        )));
    }
    let plan = crate::adapters::inplace::plan_in_place(&view, entries, false)?;
    drop(view);

    fs::copy(source, dest_tmp).map_err(|e| CoreError::io(dest_tmp, e))?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(dest_tmp)
        .map_err(|e| CoreError::io(dest_tmp, e))?;
    for (offset, patch) in &plan.writes {
        file.seek(SeekFrom::Start(*offset as u64))
            .and_then(|_| file.write_all(patch))
            .map_err(|e| CoreError::io(dest_tmp, e))?;
    }
    file.sync_all().map_err(|e| CoreError::io(dest_tmp, e))?;
    Ok(plan.report)
}

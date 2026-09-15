use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::adapter::GameInput;
use crate::adapters;
use crate::error::Result;
use crate::hash::sha256_file;
use crate::types::ProbeResult;

/// Abaixo disso nenhum adapter e apresentado como "melhor match".
const BEST_MATCH_THRESHOLD: f32 = 0.5;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionReport {
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
    /// Todos os probes com confidence > 0, ordenados do mais confiante.
    pub results: Vec<ProbeResult>,
    /// Melhor match acima do threshold; None = formato nao reconhecido.
    pub best: Option<ProbeResult>,
}

/// Pipeline de inspecao do Sprint 1: hash + probe de todos os adapters.
/// Um `.cue` e resolvido pro BIN do track de dados antes de tudo — o report
/// (path/hash/probes) e sobre o BIN, que e o que o projeto vai usar.
pub fn inspect(path: &Path) -> Result<InspectionReport> {
    let (resolved, cue) = crate::cue::resolve_source(path)?;
    let path = resolved.as_path();
    let input = GameInput::load(path)?;
    let sha256 = sha256_file(path)?;

    let mut results: Vec<ProbeResult> = adapters::all()
        .iter()
        .map(|a| {
            let mut r = a.probe(&input);
            r.support_level = a.capabilities().support_level;
            r
        })
        .filter(|r| r.confidence > 0.0)
        .collect();
    if let Some(cue) = cue {
        let note = format!(
            "cue sheet: track de dados {} ({}) em \"{}\"; {} track(s) de audio intactos",
            cue.data_track,
            cue.data_mode,
            cue.bin_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            cue.audio_tracks
        );
        for r in &mut results {
            r.evidence.insert(0, note.clone());
        }
    }
    results.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));

    let best = results
        .first()
        .filter(|r| r.confidence >= BEST_MATCH_THRESHOLD)
        .cloned();

    info!(
        path = %path.display(),
        size = input.size,
        best = best.as_ref().map(|b| b.adapter_id.as_str()).unwrap_or("none"),
        "arquivo inspecionado"
    );

    Ok(InspectionReport {
        path: path.to_path_buf(),
        size: input.size,
        sha256,
        results,
        best,
    })
}

//! Patching (spec §16). Primeiro backend: IPS em Rust puro (create + apply,
//! incl. RLE e truncate extension na leitura). BPS/xdelta entram como novos
//! variants quando houver backend confiavel.
//!
//! Todo patch criado passa por round-trip interno (apply(original) == modificado)
//! antes de ser exportado — nunca distribuimos patch quebrado.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tracing::info;

use crate::db::ProjectDb;
use crate::error::{CoreError, Result};
use crate::export::export_csv;
use crate::hash::{sha256_bytes, sha256_file};
use crate::project::load_project;
use crate::types::TranslationStatus;

const IPS_MAGIC: &[u8] = b"PATCH";
const IPS_EOF: &[u8] = b"EOF";
/// Offset que colide com a marca "EOF" — um record nunca pode COMECAR aqui.
const IPS_EOF_OFFSET: usize = 0x454F46;
const IPS_MAX_OFFSET: usize = 0xFF_FFFF;
const IPS_MAX_RECORD: usize = 0xFFFF;
/// Gaps de bytes iguais menores que isso sao absorvidos no record vizinho
/// (5 bytes de header por record; fundir gaps curtos gera patch menor).
const MERGE_GAP: usize = 6;

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("patch ips: {}", msg.into()))
}

/// Gera um patch IPS que transforma `original` em `modified`.
pub fn create_ips(original: &[u8], modified: &[u8]) -> Result<Vec<u8>> {
    if modified.len() < original.len() {
        return Err(err(
            "arquivo modificado menor que o original — IPS nao trunca; use um formato futuro (BPS)",
        ));
    }
    // Runs de bytes diferentes (extensao alem do original conta como diferenca).
    let mut diffs: Vec<(usize, usize)> = Vec::new(); // [start, end)
    let mut i = 0;
    while i < modified.len() {
        if original.get(i) == Some(&modified[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < modified.len() && original.get(i) != Some(&modified[i]) {
            i += 1;
        }
        match diffs.last_mut() {
            Some((_, prev_end)) if start - *prev_end < MERGE_GAP => *prev_end = i,
            _ => diffs.push((start, i)),
        }
    }

    let mut out = Vec::from(IPS_MAGIC);
    for (mut start, end) in diffs {
        if end > IPS_MAX_OFFSET + IPS_MAX_RECORD {
            return Err(err(format!(
                "mudanca em 0x{start:X} passa do limite de 16 MiB do formato IPS"
            )));
        }
        // Um record comecando exatamente em 0x454F46 seria lido como "EOF":
        // recua 1 byte (o byte anterior e igual nos dois arquivos, reescreve-lo e inocuo).
        if start == IPS_EOF_OFFSET {
            start -= 1;
        }
        let mut pos = start;
        while pos < end {
            if pos > IPS_MAX_OFFSET {
                return Err(err(format!(
                    "offset 0x{pos:X} nao representavel em IPS (limite 16 MiB)"
                )));
            }
            let mut len = (end - pos).min(IPS_MAX_RECORD);
            // O chunk seguinte tambem nao pode cair em cima do offset-EOF.
            if pos + len == IPS_EOF_OFFSET {
                len -= 1;
            }
            out.extend_from_slice(&(pos as u32).to_be_bytes()[1..4]);
            out.extend_from_slice(&(len as u16).to_be_bytes());
            out.extend_from_slice(&modified[pos..pos + len]);
            pos += len;
        }
    }
    out.extend_from_slice(IPS_EOF);
    Ok(out)
}

/// Aplica um patch IPS sobre `original` (suporta records normais, RLE e a
/// truncate extension de 3 bytes apos o EOF).
pub fn apply_ips(original: &[u8], patch: &[u8]) -> Result<Vec<u8>> {
    if patch.len() < IPS_MAGIC.len() + IPS_EOF.len() || &patch[..5] != IPS_MAGIC {
        return Err(err("arquivo nao e um patch IPS (magic PATCH ausente)"));
    }
    let mut out = original.to_vec();
    let mut i = IPS_MAGIC.len();
    loop {
        let header = patch
            .get(i..i + 3)
            .ok_or_else(|| err("patch truncado (sem EOF)"))?;
        if header == IPS_EOF {
            i += 3;
            break;
        }
        let offset = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
        let size_bytes = patch
            .get(i + 3..i + 5)
            .ok_or_else(|| err("record truncado"))?;
        let size = u16::from_be_bytes([size_bytes[0], size_bytes[1]]) as usize;
        i += 5;
        if size == 0 {
            // RLE: u16 tamanho + 1 byte repetido.
            let rle = patch.get(i..i + 3).ok_or_else(|| err("RLE truncado"))?;
            let rle_len = u16::from_be_bytes([rle[0], rle[1]]) as usize;
            let end = offset
                .checked_add(rle_len)
                .ok_or_else(|| err("RLE com overflow de offset"))?;
            if end > out.len() {
                out.resize(end, 0);
            }
            out[offset..end].fill(rle[2]);
            i += 3;
        } else {
            let data = patch
                .get(i..i + size)
                .ok_or_else(|| err("dados do record truncados"))?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| err("record com overflow de offset"))?;
            if end > out.len() {
                out.resize(end, 0);
            }
            out[offset..end].copy_from_slice(data);
            i += size;
        }
    }
    // Truncate extension (nao-padrao, mas comum): u24 com o tamanho final.
    if let Some(trunc) = patch.get(i..i + 3) {
        let new_len = u32::from_be_bytes([0, trunc[0], trunc[1], trunc[2]]) as usize;
        if new_len < out.len() {
            out.truncate(new_len);
        }
    }
    Ok(out)
}

/// Manifest distribuido junto com o patch (spec §16) — snake_case por ser
/// artefato publico de intercambio, nao ponte de UI.
#[derive(Debug, Serialize)]
struct PatchManifest {
    project: &'static str,
    manifest_version: u32,
    source_sha256: String,
    patched_sha256: String,
    source_language: Option<String>,
    target_locale: String,
    platform: String,
    adapter: String,
    patch_format: &'static str,
    total_entries: usize,
    translated_entries: usize,
    reviewed_entries: usize,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchExportOutcome {
    pub patch_path: PathBuf,
    pub manifest_path: PathBuf,
    pub csv_path: PathBuf,
    pub patch_format: &'static str,
    pub patched_sha256: String,
    pub patch_size: usize,
}

/// Exporta patch + manifest + CSV de traducoes para `exports/` do projeto.
/// Exige a working copy gerada pela reinsercao (spec §15: verify antes do export).
pub fn export_patch(project_dir: &Path) -> Result<PatchExportOutcome> {
    let project = load_project(project_dir)?;

    if sha256_file(&project.source_path)? != project.source_sha256 {
        return Err(CoreError::Project(
            "o arquivo de origem mudou desde a criacao do projeto; export abortado".to_string(),
        ));
    }
    let file_name = project
        .source_path
        .file_name()
        .ok_or_else(|| CoreError::Project("origem sem nome de arquivo".to_string()))?;
    let working_path = project_dir.join("working").join(file_name);
    if !working_path.is_file() {
        return Err(CoreError::Project(
            "working copy nao encontrada — rode a reinsercao antes de exportar o patch".to_string(),
        ));
    }

    let original =
        fs::read(&project.source_path).map_err(|e| CoreError::io(&project.source_path, e))?;
    let modified = fs::read(&working_path).map_err(|e| CoreError::io(&working_path, e))?;

    let patch = create_ips(&original, &modified)?;
    // Round-trip de seguranca: o patch reproduz EXATAMENTE a working copy.
    if apply_ips(&original, &patch)? != modified {
        return Err(err(
            "round-trip interno falhou (bug no gerador) — patch NAO exportado",
        ));
    }

    let db = ProjectDb::open(project_dir)?;
    let entries = db.load_entries()?;
    let translated = entries
        .iter()
        .filter(|e| e.translated_text.is_some())
        .count();
    let reviewed = entries
        .iter()
        .filter(|e| e.status == TranslationStatus::Reviewed)
        .count();

    let exports_dir = project_dir.join("exports");
    fs::create_dir_all(&exports_dir).map_err(|e| CoreError::io(&exports_dir, e))?;
    let stem = project
        .source_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    let base = format!("{stem}.{}", project.target_language);

    let patch_path = exports_dir.join(format!("{base}.ips"));
    write_atomic(&patch_path, &patch)?;

    let manifest = PatchManifest {
        project: "RomTranslate Studio",
        manifest_version: 1,
        source_sha256: project.source_sha256.clone(),
        patched_sha256: sha256_bytes(&modified),
        source_language: project.source_language.clone(),
        target_locale: project.target_language.clone(),
        platform: format!("{:?}", project.platform),
        adapter: project.adapter_id.clone(),
        patch_format: "IPS",
        total_entries: entries.len(),
        translated_entries: translated,
        reviewed_entries: reviewed,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let manifest_path = exports_dir.join(format!("{base}.manifest.json"));
    write_atomic(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)?.as_bytes(),
    )?;

    let csv_path = exports_dir.join(format!("{base}.translations.csv"));
    export_csv(&entries, &csv_path)?;

    info!(patch = %patch_path.display(), size = patch.len(), "patch exportado");
    Ok(PatchExportOutcome {
        patch_path,
        manifest_path,
        csv_path,
        patch_format: "IPS",
        patched_sha256: manifest.patched_sha256,
        patch_size: patch.len(),
    })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| CoreError::io(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| CoreError::io(path, e))?;
    Ok(())
}

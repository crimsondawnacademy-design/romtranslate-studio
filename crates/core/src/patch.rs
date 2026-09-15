//! Patching (spec §16). Backends: IPS (compatibilidade maxima, ate 16 MiB,
//! nao trunca) e BPS/beat (qualquer tamanho, truncamento, CRC32 do source,
//! do target e do proprio patch — aplica so no arquivo certo).
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

// ---------------------------------------------------------------- IPS ----

const IPS_MAGIC: &[u8] = b"PATCH";
const IPS_EOF: &[u8] = b"EOF";
/// Offset que colide com a marca "EOF" — um record nunca pode COMECAR aqui.
const IPS_EOF_OFFSET: usize = 0x454F46;
const IPS_MAX_OFFSET: usize = 0xFF_FFFF;
const IPS_MAX_RECORD: usize = 0xFFFF;
/// Gaps de bytes iguais menores que isso sao absorvidos no record vizinho.
const MERGE_GAP: usize = 6;

fn ips_err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("patch ips: {}", msg.into()))
}

/// Runs de bytes diferentes entre os dois buffers (extensao conta como diff),
/// com gaps menores que `merge_gap` fundidos.
fn diff_runs(original: &[u8], modified: &[u8], merge_gap: usize) -> Vec<(usize, usize)> {
    let mut diffs: Vec<(usize, usize)> = Vec::new();
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
            Some((_, prev_end)) if start - *prev_end < merge_gap => *prev_end = i,
            _ => diffs.push((start, i)),
        }
    }
    diffs
}

/// Gera um patch IPS que transforma `original` em `modified`.
pub fn create_ips(original: &[u8], modified: &[u8]) -> Result<Vec<u8>> {
    if modified.len() < original.len() {
        return Err(ips_err(
            "arquivo modificado menor que o original — IPS nao trunca; use BPS",
        ));
    }
    let mut out = Vec::from(IPS_MAGIC);
    for (mut start, end) in diff_runs(original, modified, MERGE_GAP) {
        if end > IPS_MAX_OFFSET + IPS_MAX_RECORD {
            return Err(ips_err(format!(
                "mudanca em 0x{start:X} passa do limite de 16 MiB do formato IPS — use BPS"
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
                return Err(ips_err(format!(
                    "offset 0x{pos:X} nao representavel em IPS (limite 16 MiB) — use BPS"
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

/// Aplica um patch IPS sobre `original` (records normais, RLE e a truncate
/// extension de 3 bytes apos o EOF).
pub fn apply_ips(original: &[u8], patch: &[u8]) -> Result<Vec<u8>> {
    if patch.len() < IPS_MAGIC.len() + IPS_EOF.len() || &patch[..5] != IPS_MAGIC {
        return Err(ips_err("arquivo nao e um patch IPS (magic PATCH ausente)"));
    }
    let mut out = original.to_vec();
    let mut i = IPS_MAGIC.len();
    loop {
        let header = patch
            .get(i..i + 3)
            .ok_or_else(|| ips_err("patch truncado (sem EOF)"))?;
        if header == IPS_EOF {
            i += 3;
            break;
        }
        let offset = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
        let size_bytes = patch
            .get(i + 3..i + 5)
            .ok_or_else(|| ips_err("record truncado"))?;
        let size = u16::from_be_bytes([size_bytes[0], size_bytes[1]]) as usize;
        i += 5;
        if size == 0 {
            let rle = patch.get(i..i + 3).ok_or_else(|| ips_err("RLE truncado"))?;
            let rle_len = u16::from_be_bytes([rle[0], rle[1]]) as usize;
            let end = offset
                .checked_add(rle_len)
                .ok_or_else(|| ips_err("RLE com overflow de offset"))?;
            if end > out.len() {
                out.resize(end, 0);
            }
            out[offset..end].fill(rle[2]);
            i += 3;
        } else {
            let data = patch
                .get(i..i + size)
                .ok_or_else(|| ips_err("dados do record truncados"))?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| ips_err("record com overflow de offset"))?;
            if end > out.len() {
                out.resize(end, 0);
            }
            out[offset..end].copy_from_slice(data);
            i += size;
        }
    }
    if let Some(trunc) = patch.get(i..i + 3) {
        let new_len = u32::from_be_bytes([0, trunc[0], trunc[1], trunc[2]]) as usize;
        if new_len < out.len() {
            out.truncate(new_len);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- BPS ----

const BPS_MAGIC: &[u8] = b"BPS1";

fn bps_err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("patch bps: {}", msg.into()))
}

/// CRC-32 (IEEE/zlib, poly refletido 0xEDB88320) — o CRC do formato beat.
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, slot) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Varint do beat: 7 bits por byte, bit 0x80 marca o ULTIMO, com +1 implicito
/// a cada byte de continuacao (encoding canonico do byuu).
fn bps_write_number(out: &mut Vec<u8>, mut data: u64) {
    loop {
        let x = (data & 0x7F) as u8;
        data >>= 7;
        if data == 0 {
            out.push(0x80 | x);
            break;
        }
        out.push(x);
        data -= 1;
    }
}

fn bps_read_number(patch: &[u8], pos: &mut usize) -> Result<u64> {
    let mut data: u64 = 0;
    let mut shift: u64 = 1;
    for _ in 0..10 {
        let x = *patch.get(*pos).ok_or_else(|| bps_err("varint truncado"))? as u64;
        *pos += 1;
        data = data
            .checked_add(
                (x & 0x7F)
                    .checked_mul(shift)
                    .ok_or_else(|| bps_err("varint estourou"))?,
            )
            .ok_or_else(|| bps_err("varint estourou"))?;
        if x & 0x80 != 0 {
            return Ok(data);
        }
        shift <<= 7;
        data = data
            .checked_add(shift)
            .ok_or_else(|| bps_err("varint estourou"))?;
    }
    Err(bps_err("varint longo demais"))
}

/// Gera um patch BPS (modo linear: SourceRead para trechos identicos no mesmo
/// offset, TargetRead para o resto). Suporta target maior OU menor que o source.
pub fn create_bps(original: &[u8], modified: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::from(BPS_MAGIC);
    bps_write_number(&mut out, original.len() as u64);
    bps_write_number(&mut out, modified.len() as u64);
    bps_write_number(&mut out, 0); // sem metadata

    // Trocar de action custa ~2-4 bytes: gaps iguais curtos ficam no TargetRead.
    let diffs = diff_runs(original, modified, 8);
    let mut cursor = 0usize;
    let emit_source_read = |out: &mut Vec<u8>, len: usize| {
        if len > 0 {
            bps_write_number(out, ((len - 1) as u64) << 2); // | 0 = SourceRead
        }
    };
    for (start, end) in diffs {
        emit_source_read(&mut out, start - cursor);
        let len = end - start;
        bps_write_number(&mut out, (((len - 1) as u64) << 2) | 1); // TargetRead
        out.extend_from_slice(&modified[start..end]);
        cursor = end;
    }
    emit_source_read(&mut out, modified.len() - cursor);

    out.extend_from_slice(&crc32(original).to_le_bytes());
    out.extend_from_slice(&crc32(modified).to_le_bytes());
    let patch_crc = crc32(&out);
    out.extend_from_slice(&patch_crc.to_le_bytes());
    Ok(out)
}

/// Aplica um patch BPS, validando os tres CRC-32 (patch integro, source
/// correto, target exato). Suporta os quatro commands do formato.
pub fn apply_bps(source: &[u8], patch: &[u8]) -> Result<Vec<u8>> {
    if patch.len() < BPS_MAGIC.len() + 12 || &patch[..4] != BPS_MAGIC {
        return Err(bps_err("arquivo nao e um patch BPS (magic BPS1 ausente)"));
    }
    let footer_at = patch.len() - 12;
    let stored_patch_crc = u32::from_le_bytes(patch[patch.len() - 4..].try_into().unwrap());
    if crc32(&patch[..patch.len() - 4]) != stored_patch_crc {
        return Err(bps_err("patch corrompido (CRC-32 do patch nao bate)"));
    }
    let stored_source_crc = u32::from_le_bytes(patch[footer_at..footer_at + 4].try_into().unwrap());
    if crc32(source) != stored_source_crc {
        return Err(bps_err(format!(
            "este patch e para OUTRO arquivo: CRC-32 do original esperado {:08X}, o seu e {:08X}",
            stored_source_crc,
            crc32(source)
        )));
    }
    let stored_target_crc =
        u32::from_le_bytes(patch[footer_at + 4..footer_at + 8].try_into().unwrap());

    let mut pos = BPS_MAGIC.len();
    let source_size = bps_read_number(patch, &mut pos)? as usize;
    let target_size = bps_read_number(patch, &mut pos)? as usize;
    if source_size != source.len() {
        return Err(bps_err(format!(
            "tamanho do original nao bate: patch espera {source_size} bytes, arquivo tem {}",
            source.len()
        )));
    }
    if target_size > crate::adapter::IN_MEMORY_MAX as usize {
        return Err(bps_err("target declarado maior que o limite em memoria"));
    }
    let metadata_size = bps_read_number(patch, &mut pos)? as usize;
    pos = pos
        .checked_add(metadata_size)
        .filter(|&p| p <= footer_at)
        .ok_or_else(|| bps_err("metadata passa do fim do patch"))?;

    let mut target = vec![0u8; target_size];
    let mut output = 0usize;
    let mut source_rel = 0usize;
    let mut target_rel = 0usize;

    while pos < footer_at {
        let data = bps_read_number(patch, &mut pos)?;
        let command = (data & 3) as u8;
        let length = (data >> 2) as usize + 1;
        let end = output
            .checked_add(length)
            .filter(|&e| e <= target_size)
            .ok_or_else(|| bps_err("action escreve alem do target"))?;
        match command {
            0 => {
                // SourceRead: mesmo offset do output.
                if end > source.len() {
                    return Err(bps_err("SourceRead alem do source"));
                }
                target[output..end].copy_from_slice(&source[output..end]);
            }
            1 => {
                // TargetRead: bytes literais do patch.
                let data_end = pos
                    .checked_add(length)
                    .filter(|&e| e <= footer_at)
                    .ok_or_else(|| bps_err("TargetRead alem do patch"))?;
                target[output..end].copy_from_slice(&patch[pos..data_end]);
                pos = data_end;
            }
            2 => {
                // SourceCopy: offset relativo com sinal.
                let raw = bps_read_number(patch, &mut pos)?;
                source_rel = signed_step(source_rel, raw, "SourceCopy")?;
                let src_end = source_rel
                    .checked_add(length)
                    .filter(|&e| e <= source.len())
                    .ok_or_else(|| bps_err("SourceCopy alem do source"))?;
                target[output..end].copy_from_slice(&source[source_rel..src_end]);
                source_rel = src_end;
            }
            3 => {
                // TargetCopy: pode sobrepor a regiao recem-escrita (byte a byte).
                let raw = bps_read_number(patch, &mut pos)?;
                target_rel = signed_step(target_rel, raw, "TargetCopy")?;
                if target_rel >= end {
                    return Err(bps_err("TargetCopy le area ainda nao escrita"));
                }
                for k in 0..length {
                    target[output + k] = target[target_rel];
                    target_rel += 1;
                }
            }
            _ => unreachable!(),
        }
        output = end;
    }
    if output != target_size {
        return Err(bps_err(format!(
            "patch terminou com {output} de {target_size} bytes escritos"
        )));
    }
    if crc32(&target) != stored_target_crc {
        return Err(bps_err("resultado corrompido (CRC-32 do target nao bate)"));
    }
    Ok(target)
}

fn signed_step(base: usize, raw: u64, what: &str) -> Result<usize> {
    let magnitude = (raw >> 1) as usize;
    if raw & 1 != 0 {
        base.checked_sub(magnitude)
            .ok_or_else(|| bps_err(format!("{what} com offset negativo alem do inicio")))
    } else {
        base.checked_add(magnitude)
            .ok_or_else(|| bps_err(format!("{what} com offset estourado")))
    }
}

// ------------------------------------------------------------- export ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatchFormat {
    Ips,
    Bps,
}

impl PatchFormat {
    fn label(self) -> &'static str {
        match self {
            PatchFormat::Ips => "IPS",
            PatchFormat::Bps => "BPS",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            PatchFormat::Ips => "ips",
            PatchFormat::Bps => "bps",
        }
    }
}

/// Auto: IPS quando o formato aguenta (ate 16 MiB, sem encolher) — maxima
/// compatibilidade com ferramentas antigas; BPS caso contrario.
pub fn choose_format(original_len: usize, modified_len: usize) -> PatchFormat {
    if modified_len >= original_len && modified_len <= IPS_MAX_OFFSET + IPS_MAX_RECORD {
        PatchFormat::Ips
    } else {
        PatchFormat::Bps
    }
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
/// `format: None` escolhe automaticamente (IPS se couber, senao BPS).
pub fn export_patch(project_dir: &Path, format: Option<PatchFormat>) -> Result<PatchExportOutcome> {
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

    let format = format.unwrap_or_else(|| choose_format(original.len(), modified.len()));
    let patch = match format {
        PatchFormat::Ips => create_ips(&original, &modified)?,
        PatchFormat::Bps => create_bps(&original, &modified)?,
    };
    // Round-trip de seguranca: o patch reproduz EXATAMENTE a working copy.
    let roundtrip = match format {
        PatchFormat::Ips => apply_ips(&original, &patch)?,
        PatchFormat::Bps => apply_bps(&original, &patch)?,
    };
    if roundtrip != modified {
        return Err(CoreError::Project(
            "round-trip interno do patch falhou (bug no gerador) — patch NAO exportado".to_string(),
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

    let patch_path = exports_dir.join(format!("{base}.{}", format.extension()));
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
        patch_format: format.label(),
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

    info!(patch = %patch_path.display(), size = patch.len(), format = format.label(), "patch exportado");
    Ok(PatchExportOutcome {
        patch_path,
        manifest_path,
        csv_path,
        patch_format: format.label(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn bps_varint_roundtrip() {
        for value in [
            0u64,
            1,
            0x7F,
            0x80,
            0x3FFF,
            0x4000,
            123_456_789,
            u32::MAX as u64,
        ] {
            let mut buf = Vec::new();
            bps_write_number(&mut buf, value);
            let mut pos = 0;
            assert_eq!(bps_read_number(&buf, &mut pos).unwrap(), value, "{value}");
            assert_eq!(pos, buf.len());
        }
    }

    /// Patch BPS construido a mao cobrindo SourceCopy e TargetCopy (que o
    /// nosso gerador linear nao emite, mas patches da cena usam).
    #[test]
    fn bps_apply_supports_all_four_commands() {
        let source = b"ABCDEFGH";
        // target: "ABZZCDCDCD" (10 bytes):
        //   SourceRead 2      -> "AB"
        //   TargetRead 2 "ZZ" -> "ZZ"
        //   SourceCopy 2 de offset 2 -> "CD"
        //   TargetCopy 4 de offset 4 -> "CDCD" (le o que acabou de escrever)
        let target = b"ABZZCDCDCD";
        let mut patch = Vec::from(&b"BPS1"[..]);
        bps_write_number(&mut patch, source.len() as u64);
        bps_write_number(&mut patch, target.len() as u64);
        bps_write_number(&mut patch, 0);
        bps_write_number(&mut patch, 1 << 2); // SourceRead, len 2
        bps_write_number(&mut patch, (1 << 2) | 1); // TargetRead
        patch.extend_from_slice(b"ZZ");
        bps_write_number(&mut patch, (1 << 2) | 2); // SourceCopy
        bps_write_number(&mut patch, 2 << 1); // +2
        bps_write_number(&mut patch, (3 << 2) | 3); // TargetCopy len 4
        bps_write_number(&mut patch, 4 << 1); // +4
        patch.extend_from_slice(&crc32(source).to_le_bytes());
        patch.extend_from_slice(&crc32(target).to_le_bytes());
        let pc = crc32(&patch);
        patch.extend_from_slice(&pc.to_le_bytes());

        assert_eq!(apply_bps(source, &patch).unwrap(), target);
    }

    #[test]
    fn choose_format_prefers_ips_when_it_fits() {
        assert_eq!(choose_format(1000, 1000), PatchFormat::Ips);
        assert_eq!(choose_format(1000, 500), PatchFormat::Bps, "encolheu");
        assert_eq!(
            choose_format(20 << 20, 20 << 20),
            PatchFormat::Bps,
            "alem de 16 MiB"
        );
    }
}

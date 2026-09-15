//! Adapter de GameCube (disco GCM/ISO) — formato confirmado no Dolphin
//! (DiscUtils/FileSystemGCWii): magic 0xC2339F3D em 0x1C; fst_offset u32 BE em
//! 0x424 e fst_size em 0x428; FST com entries de 12 bytes (3x u32 BE: name
//! [byte alto = flag de diretorio], offset, size/next) + string table.
//!
//! Disco GC NAO e cifrado: extracao por arquivo do filesystem (scan ASCII com
//! resource_path) e reinsercao in-place conservadora funcionam. Sem checksum
//! de disco a recalcular.

use crate::adapter::{AppliedImage, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::scan::{scan_bytes, ScanConfig, ScanEncoding};
use crate::types::{
    AdapterCapabilities, Platform, ProbeResult, ResourceDescriptor, SupportLevel, TextEntry,
};

pub struct GameCubeAdapter;

pub const MAGIC_OFFSET: usize = 0x1C;
pub const MAGIC: [u8; 4] = [0xC2, 0x33, 0x9F, 0x3D];
const TITLE_OFFSET: usize = 0x20;
pub const FST_OFFSET_FIELD: usize = 0x424;
pub const FST_SIZE_FIELD: usize = 0x428;
const HEADER_MIN: usize = 0x440;
const ENTRY_SIZE: usize = 12;
const MAX_ENTRIES: usize = 65_536;
const MAX_NAME: usize = 256;
const MAX_TEXT_ENTRIES: usize = 20_000;

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("gamecube: {}", msg.into()))
}

fn read_u32_be(data: &[u8], off: usize) -> Result<u32> {
    let bytes = data
        .get(off..off + 4)
        .ok_or_else(|| err(format!("truncado em 0x{off:X}")))?;
    Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
}

fn has_magic(data: &[u8]) -> bool {
    data.len() >= HEADER_MIN && data[MAGIC_OFFSET..MAGIC_OFFSET + 4] == MAGIC
}

/// Percorre a FST (entry 0 = raiz; diretorios delimitam filhos por range).
fn walk_fst(data: &[u8]) -> Result<Vec<ResourceDescriptor>> {
    let fst_offset = read_u32_be(data, FST_OFFSET_FIELD)? as usize;
    let fst_size = read_u32_be(data, FST_SIZE_FIELD)? as usize;
    let fst_end = fst_offset
        .checked_add(fst_size)
        .filter(|&e| e <= data.len())
        .ok_or_else(|| err("FST passa do fim do arquivo"))?;
    let fst = &data[fst_offset..fst_end];
    if fst.len() < ENTRY_SIZE {
        return Err(err("FST menor que uma entry"));
    }

    let entry = |i: usize| -> Result<(u32, u32, u32)> {
        let base = i * ENTRY_SIZE;
        Ok((
            read_u32_be(fst, base)?,
            read_u32_be(fst, base + 4)?,
            read_u32_be(fst, base + 8)?,
        ))
    };

    let (root_flags, _, total) = entry(0)?;
    if root_flags & 0xFF00_0000 == 0 {
        return Err(err("entry raiz da FST nao e diretorio"));
    }
    let total = total as usize;
    if !(1..=MAX_ENTRIES).contains(&total) || total * ENTRY_SIZE > fst.len() {
        return Err(err("contagem de entries da FST invalida"));
    }
    let string_table = &fst[total * ENTRY_SIZE..];

    let read_name = |name_offset: usize| -> Result<String> {
        let region = string_table
            .get(name_offset..string_table.len().min(name_offset + MAX_NAME))
            .ok_or_else(|| err("nome fora da string table"))?;
        let end = region
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| err("nome sem terminador na string table"))?;
        Ok(String::from_utf8_lossy(&region[..end]).into_owned())
    };

    let mut files = Vec::new();
    // Stack de diretorios abertos: (indice de fim, prefixo do path).
    let mut stack: Vec<(usize, String)> = vec![(total, String::new())];
    let mut i = 1;
    while i < total {
        while stack.len() > 1 && i >= stack.last().unwrap().0 {
            stack.pop();
        }
        let prefix = stack.last().unwrap().1.clone();
        let (word0, offset, size_or_next) = entry(i)?;
        let name = read_name((word0 & 0x00FF_FFFF) as usize)?;
        if word0 & 0xFF00_0000 != 0 {
            // Diretorio: size_or_next = primeira entry APOS os filhos.
            let next = size_or_next as usize;
            if next <= i || next > total {
                return Err(err(format!("diretorio \"{name}\" com range invalido")));
            }
            stack.push((next, format!("{prefix}{name}/")));
        } else {
            let start = offset as u64;
            let end = start
                .checked_add(size_or_next as u64)
                .filter(|&e| e <= data.len() as u64)
                .ok_or_else(|| err(format!("arquivo \"{prefix}{name}\" fora do disco")))?;
            files.push(ResourceDescriptor {
                path: format!("{prefix}{name}"),
                offset: start,
                size: end - start,
            });
        }
        i += 1;
    }
    Ok(files)
}

impl GameAdapter for GameCubeAdapter {
    fn id(&self) -> &'static str {
        "gamecube.generic"
    }

    fn display_name(&self) -> &'static str {
        "GameCube (disc)"
    }

    fn platform(&self) -> Platform {
        Platform::GameCube
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            detect: true,
            extract: true,
            reinsert: true, // conservadora: in-place, sem relocacao
            patch: true,
            compression: false,
            pointer_relocation: false,
            font_table: false,
            experimental: true,
            support_level: SupportLevel::Experimental,
        }
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        let head = &input.head;
        if head.len() < 0x60 || head[MAGIC_OFFSET..MAGIC_OFFSET + 4] != MAGIC {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let mut confidence: f32 = 0.93;
        let mut evidence = vec!["magic word de disco GameCube em 0x1C".to_string()];

        let code = &head[0..6];
        if code.iter().all(|&b| b.is_ascii_alphanumeric()) {
            confidence += 0.03;
            evidence.push(format!(
                "game code ASCII: {}",
                String::from_utf8_lossy(code)
            ));
        }
        let title: Vec<u8> = head[TITLE_OFFSET..(TITLE_OFFSET + 0x40).min(head.len())]
            .iter()
            .take_while(|&&b| b != 0)
            .copied()
            .collect();
        if !title.is_empty() && title.iter().all(|&b| (0x20..0x7F).contains(&b)) {
            confidence += 0.03;
            evidence.push(format!(
                "titulo interno: \"{}\"",
                String::from_utf8_lossy(&title)
            ));
        }
        match walk_fst(head) {
            Ok(files) => evidence.push(format!("filesystem FST valido: {} arquivos", files.len())),
            Err(_) => evidence.push(
                "filesystem fora da janela de probe (inspecao completa na extracao)".to_string(),
            ),
        }

        ProbeResult {
            confidence: confidence.min(0.99),
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }

    fn list_resources(&self, data: &[u8]) -> Result<Vec<ResourceDescriptor>> {
        if !has_magic(data) {
            return Err(err("magic de disco GameCube ausente"));
        }
        walk_fst(data)
    }

    /// Scan ASCII por arquivo do filesystem (e no header, onde vive o titulo),
    /// com `resource_path` e limite real por string (in-place).
    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        let files = self.list_resources(data)?;
        let mut regions: Vec<(String, u64, u64)> =
            vec![("boot.bin".to_string(), 0, HEADER_MIN as u64)];
        regions.extend(
            files
                .iter()
                .filter(|f| f.size > 0)
                .map(|f| (f.path.clone(), f.offset, f.offset + f.size)),
        );

        let mut entries: Vec<TextEntry> = Vec::new();
        for (path, start, end) in regions {
            let outcome = scan_bytes(
                data,
                &ScanConfig {
                    encoding: ScanEncoding::Ascii,
                    region_start: Some(start),
                    region_end: Some(end),
                    max_entries: MAX_TEXT_ENTRIES - entries.len(),
                    ..ScanConfig::default()
                },
            )?;
            for mut e in outcome.entries {
                e.resource_path = Some(path.clone());
                e.max_bytes = Some(e.original_bytes.len());
                e.context = Some("in-place: traducao limitada ao espaco original".to_string());
                entries.push(e);
            }
            if entries.len() >= MAX_TEXT_ENTRIES {
                break;
            }
        }
        Ok(entries)
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        if !has_magic(data) {
            return Err(err("magic de disco GameCube ausente"));
        }
        // Disco GC nao tem checksum sobre os dados: nada a finalizar.
        super::inplace::apply_in_place(data, entries, |_| {})
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        let mut checks = Vec::new();
        let mut problems = Vec::new();

        if !has_magic(data) {
            problems.push("magic de disco GameCube ausente".to_string());
        } else {
            checks.push("magic e header do disco preservados".to_string());
            match self.list_resources(data) {
                Ok(files) => checks.push(format!("filesystem integro: {} arquivos", files.len())),
                Err(e) => problems.push(format!("filesystem corrompido: {e}")),
            }
            match self.extract_structured(data) {
                Ok(entries) => checks.push(format!("{} strings re-extraidas", entries.len())),
                Err(e) => problems.push(format!("re-extracao falhou: {e}")),
            }
        }

        Ok(VerificationReport {
            ok: problems.is_empty(),
            checks,
            problems,
        })
    }
}

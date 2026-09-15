//! Adapter de Nintendo DS (GBATEK: cartridge header + filesystem FNT/FAT).
//!
//! Deteccao: CRC-16 do header (0x000..0x15E), campo do logo CRC == 0xCF56
//! (conferimos o CAMPO documentado — o bitmap do logo, material Nintendo,
//! nao entra no repo) e estrutura FNT/FAT dentro dos limites.
//!
//! Extracao: lista o filesystem e roda o scanner (ASCII + UTF-16LE, o encoding
//! tipico de texto em DS) POR ARQUIVO, com `resource_path` preenchido.
//! Reinsercao: in-place conservadora (mesmo espaco), CRC do header recalculado.

use crate::adapter::{AppliedImage, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::scan::{scan_bytes, ScanConfig, ScanEncoding};
use crate::types::{
    AdapterCapabilities, Platform, ProbeResult, ResourceDescriptor, SupportLevel, TextEntry,
};

pub struct NdsAdapter;

pub const HEADER_LEN: usize = 0x200;
const LOGO_CRC_OFFSET: usize = 0x15C;
const HEADER_CRC_OFFSET: usize = 0x15E;
/// Valor fixo do CRC do logo em ROMs validas (GBATEK).
pub const LOGO_CRC_EXPECTED: u16 = 0xCF56;
const MAX_FILES: usize = 8192;
const MAX_DIRS: u16 = 2048;
const MAX_ENTRIES_TOTAL: usize = 20_000;

/// CRC-16 (poly refletido 0xA001, init 0xFFFF) — o CRC usado pelo header NDS.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("nds: {}", msg.into()))
}

fn read_u32(data: &[u8], off: usize) -> Result<u32> {
    let bytes = data
        .get(off..off + 4)
        .ok_or_else(|| err(format!("truncado em 0x{off:X}")))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u16(data: &[u8], off: usize) -> Result<u16> {
    let bytes = data
        .get(off..off + 2)
        .ok_or_else(|| err(format!("truncado em 0x{off:X}")))?;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

struct FsLayout {
    fnt_offset: usize,
    fnt_size: usize,
    fat_offset: usize,
    fat_size: usize,
}

fn fs_layout(data: &[u8]) -> Result<FsLayout> {
    let layout = FsLayout {
        fnt_offset: read_u32(data, 0x40)? as usize,
        fnt_size: read_u32(data, 0x44)? as usize,
        fat_offset: read_u32(data, 0x48)? as usize,
        fat_size: read_u32(data, 0x4C)? as usize,
    };
    let check = |off: usize, size: usize, what: &str| -> Result<()> {
        let end = off
            .checked_add(size)
            .ok_or_else(|| err(format!("{what}: overflow")))?;
        if end > data.len() {
            return Err(err(format!(
                "{what}: 0x{off:X}+{size} passa do fim do arquivo"
            )));
        }
        Ok(())
    };
    check(layout.fnt_offset, layout.fnt_size, "FNT")?;
    check(layout.fat_offset, layout.fat_size, "FAT")?;
    if !layout.fat_size.is_multiple_of(8) || layout.fat_size / 8 > MAX_FILES {
        return Err(err("FAT com tamanho invalido"));
    }
    if layout.fnt_size < 8 {
        return Err(err("FNT menor que uma entrada de diretorio"));
    }
    Ok(layout)
}

/// Percorre a FNT (main table + subtables) casando file ids com a FAT.
fn walk_filesystem(data: &[u8], layout: &FsLayout) -> Result<Vec<ResourceDescriptor>> {
    let fnt = &data[layout.fnt_offset..layout.fnt_offset + layout.fnt_size];
    let total_dirs = read_u16(fnt, 6)?.min(MAX_DIRS);
    let mut files = Vec::new();

    // Cada diretorio e visitado exatamente uma vez pelo indice — sem ciclos.
    let mut dir_paths: Vec<String> = vec![String::new(); total_dirs as usize];
    for dir_index in 0..total_dirs as usize {
        let entry_off = dir_index * 8;
        let sub_off = read_u32(fnt, entry_off)? as usize;
        let mut file_id = read_u16(fnt, entry_off + 4)?;
        let prefix = dir_paths[dir_index].clone();

        let mut pos = sub_off;
        loop {
            let type_len = *fnt
                .get(pos)
                .ok_or_else(|| err("subtable da FNT truncada"))?;
            pos += 1;
            if type_len == 0 {
                break;
            }
            let name_len = (type_len & 0x7F) as usize;
            let name_bytes = fnt
                .get(pos..pos + name_len)
                .ok_or_else(|| err("nome truncado na FNT"))?;
            pos += name_len;
            let name = String::from_utf8_lossy(name_bytes).into_owned();

            if type_len & 0x80 != 0 {
                // Diretorio: 2 bytes com o id (0xF001..).
                let dir_id = read_u16(fnt, pos)?;
                pos += 2;
                let child = (dir_id & 0x0FFF) as usize;
                if child < dir_paths.len() && child > dir_index {
                    dir_paths[child] = format!("{prefix}{name}/");
                }
            } else {
                let fat_entry = layout.fat_offset + file_id as usize * 8;
                if fat_entry + 8 > layout.fat_offset + layout.fat_size {
                    return Err(err(format!("file id {file_id} fora da FAT")));
                }
                let start = read_u32(data, fat_entry)? as u64;
                let end = read_u32(data, fat_entry + 4)? as u64;
                if end < start || end > data.len() as u64 {
                    return Err(err(format!(
                        "arquivo \"{prefix}{name}\" com range invalido na FAT"
                    )));
                }
                files.push(ResourceDescriptor {
                    path: format!("{prefix}{name}"),
                    offset: start,
                    size: end - start,
                });
                file_id = file_id.wrapping_add(1);
            }
            if files.len() > MAX_FILES {
                return Err(err("filesystem declara arquivos demais"));
            }
        }
    }
    Ok(files)
}

impl GameAdapter for NdsAdapter {
    fn id(&self) -> &'static str {
        "nds.generic"
    }

    fn display_name(&self) -> &'static str {
        "NDS Generic"
    }

    fn platform(&self) -> Platform {
        Platform::Nds
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
        if head.len() < HEADER_LEN {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let mut evidence = Vec::new();
        let mut confidence: f32 = 0.0;

        let header_crc_ok =
            crc16(&head[..HEADER_CRC_OFFSET]) == u16::from_le_bytes([head[0x15E], head[0x15F]]);
        let logo_field_ok = u16::from_le_bytes([head[LOGO_CRC_OFFSET], head[LOGO_CRC_OFFSET + 1]])
            == LOGO_CRC_EXPECTED;

        if header_crc_ok {
            confidence += 0.6;
            evidence.push("CRC-16 do header valido".to_string());
        }
        if logo_field_ok {
            confidence += 0.25;
            evidence.push("campo do logo CRC com o valor esperado (0xCF56)".to_string());
        }
        if confidence == 0.0 {
            return ProbeResult::no_match(self.id(), self.platform());
        }

        let title = &head[0..12];
        if title.iter().all(|&b| b == 0 || (0x20..0x7F).contains(&b)) {
            confidence += 0.05;
            evidence.push("titulo do cartucho em ASCII valido".to_string());
        }
        match fs_layout(head).and_then(|l| {
            if l.fnt_offset + l.fnt_size <= head.len() && l.fat_offset + l.fat_size <= head.len() {
                walk_filesystem(head, &l)
            } else {
                Err(err("filesystem alem da janela de probe"))
            }
        }) {
            Ok(files) => {
                confidence += 0.05;
                evidence.push(format!(
                    "filesystem FNT/FAT valido: {} arquivos",
                    files.len()
                ));
            }
            Err(_) => evidence
                .push("filesystem fora da janela de probe (inspecao completa na extracao)".into()),
        }

        ProbeResult {
            confidence: confidence.min(0.99),
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }

    fn list_resources(&self, data: &[u8]) -> Result<Vec<ResourceDescriptor>> {
        if data.len() < HEADER_LEN {
            return Err(err("arquivo menor que o header NDS"));
        }
        let layout = fs_layout(data)?;
        walk_filesystem(data, &layout)
    }

    /// Scanner ASCII + UTF-16LE por arquivo do filesystem (e no header, onde
    /// vive o titulo), com `resource_path` dizendo de onde cada string veio.
    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        let files = self.list_resources(data)?;
        let mut regions: Vec<(String, u64, u64)> =
            vec![("header".to_string(), 0, HEADER_LEN as u64)];
        regions.extend(
            files
                .iter()
                .filter(|f| f.size > 0)
                .map(|f| (f.path.clone(), f.offset, f.offset + f.size)),
        );

        let mut entries: Vec<TextEntry> = Vec::new();
        'outer: for (path, start, end) in regions {
            for encoding in [ScanEncoding::Ascii, ScanEncoding::Utf16Le] {
                let outcome = scan_bytes(
                    data,
                    &ScanConfig {
                        encoding,
                        region_start: Some(start),
                        region_end: Some(end),
                        max_entries: MAX_ENTRIES_TOTAL - entries.len(),
                        ..ScanConfig::default()
                    },
                )?;
                for mut e in outcome.entries {
                    e.resource_path = Some(path.clone());
                    e.max_bytes = Some(e.original_bytes.len());
                    e.context = Some("in-place: traducao limitada ao espaco original".to_string());
                    entries.push(e);
                }
                if entries.len() >= MAX_ENTRIES_TOTAL {
                    break 'outer;
                }
            }
        }
        Ok(entries)
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        if data.len() < HEADER_LEN {
            return Err(err("arquivo menor que o header NDS"));
        }
        super::inplace::apply_in_place(data, entries, |out| {
            // Traducao do titulo (0x00..0x0C) muda o CRC do header: recalcula.
            let crc = crc16(&out[..HEADER_CRC_OFFSET]);
            out[HEADER_CRC_OFFSET..HEADER_CRC_OFFSET + 2].copy_from_slice(&crc.to_le_bytes());
        })
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        let mut checks = Vec::new();
        let mut problems = Vec::new();

        if data.len() < HEADER_LEN {
            problems.push("arquivo menor que o header NDS".to_string());
        } else {
            if crc16(&data[..HEADER_CRC_OFFSET]) == u16::from_le_bytes([data[0x15E], data[0x15F]]) {
                checks.push("CRC-16 do header valido".to_string());
            } else {
                problems.push("CRC-16 do header invalido".to_string());
            }
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

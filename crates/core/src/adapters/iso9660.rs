//! Parser ISO 9660 compartilhado pelos adapters de PlayStation (PS1/PS2/PSP).
//! Layout confirmado no kernel Linux (include/uapi/linux/iso_fs.h):
//! PVD no setor 16 — "CD001" no offset 1, system_id em 8..40, root directory
//! record em 156..190; directory record — extent u32 LE em +2, size u32 LE em
//! +10, flags em +25 (bit 1 = diretorio), name_len em +32, nome em +33.
//!
//! Suporta imagens 2048/setor (ISO) e raw 2352/setor (BIN de CD: sync 12 +
//! header 4 [+ subheader 8 no Mode 2]; dados form1 = 2048 por setor).

use std::collections::{HashMap, HashSet};

use super::pointers::{
    ensure_pointers_untouched, file_relative_tables, mark_relocatable, relocate, to_file_relative,
    Relocation,
};
use crate::error::{CoreError, Result};
use crate::types::ResourceDescriptor;

pub const SECTOR_RAW: usize = 2352;
pub const SECTOR_DATA: usize = 2048;
const SYNC: [u8; 12] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
];
const PVD_LBA: usize = 16;
const MAX_DIRS: usize = 4096;
const MAX_FILES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorMap {
    Plain2048,
    Raw2352,
}

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("iso9660: {}", msg.into()))
}

/// Raw se o arquivo comeca com o sync pattern de CD; senao 2048/setor.
pub fn detect_map(data: &[u8]) -> SectorMap {
    if data.len() >= SYNC.len() && data[..SYNC.len()] == SYNC {
        SectorMap::Raw2352
    } else {
        SectorMap::Plain2048
    }
}

/// Offset absoluto dos 2048 bytes de dados do setor `lba`.
fn sector_data_offset(data: &[u8], map: SectorMap, lba: usize) -> Result<usize> {
    match map {
        SectorMap::Plain2048 => Ok(lba
            .checked_mul(SECTOR_DATA)
            .ok_or_else(|| err("LBA estourou"))?),
        SectorMap::Raw2352 => {
            let base = lba
                .checked_mul(SECTOR_RAW)
                .ok_or_else(|| err("LBA estourou"))?;
            let mode = *data
                .get(base + 15)
                .ok_or_else(|| err(format!("setor {lba} fora do arquivo")))?;
            // Mode 1: dados em +16; Mode 2 (XA form1): subheader de 8, dados em +24.
            Ok(base + if mode == 1 { 16 } else { 24 })
        }
    }
}

pub fn read_sector(data: &[u8], map: SectorMap, lba: usize) -> Result<&[u8]> {
    let off = sector_data_offset(data, map, lba)?;
    data.get(off..off + SECTOR_DATA)
        .ok_or_else(|| err(format!("setor {lba} truncado")))
}

#[derive(Debug, Clone)]
pub struct IsoInfo {
    pub system_id: String,
    pub volume_id: String,
}

fn ascii_field(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

/// Primary Volume Descriptor (setor 16). Err se nao houver "CD001".
pub fn parse_pvd(data: &[u8], map: SectorMap) -> Result<IsoInfo> {
    let pvd = read_sector(data, map, PVD_LBA)?;
    if &pvd[1..6] != b"CD001" {
        return Err(err("magic CD001 ausente no setor 16"));
    }
    Ok(IsoInfo {
        system_id: ascii_field(&pvd[8..40]),
        volume_id: ascii_field(&pvd[40..72]),
    })
}

fn read_u32_le(buf: &[u8], off: usize) -> Result<u32> {
    let bytes = buf
        .get(off..off + 4)
        .ok_or_else(|| err("record truncado"))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

/// Regioes contiguas (offset absoluto no arquivo) que compoem um arquivo do
/// filesystem: 1 regiao no formato 2048; uma por setor no raw 2352.
pub fn file_regions(
    data: &[u8],
    map: SectorMap,
    extent: u32,
    size: u32,
) -> Result<Vec<(u64, u64)>> {
    let mut regions = Vec::new();
    let sectors = (size as usize).div_ceil(SECTOR_DATA);
    match map {
        SectorMap::Plain2048 => {
            let start = sector_data_offset(data, map, extent as usize)? as u64;
            let end = start + size as u64;
            if end > data.len() as u64 {
                return Err(err("arquivo passa do fim da imagem"));
            }
            regions.push((start, end));
        }
        SectorMap::Raw2352 => {
            for i in 0..sectors {
                let off = sector_data_offset(data, map, extent as usize + i)? as u64;
                let chunk = SECTOR_DATA.min(size as usize - i * SECTOR_DATA) as u64;
                if off + chunk > data.len() as u64 {
                    return Err(err("arquivo passa do fim da imagem"));
                }
                regions.push((off, off + chunk));
            }
        }
    }
    Ok(regions)
}

/// Le um arquivo inteiro do filesystem para memoria (junta as regioes).
pub fn read_file(data: &[u8], map: SectorMap, extent: u32, size: u32) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(size as usize);
    for (start, end) in file_regions(data, map, extent, size)? {
        out.extend_from_slice(&data[start as usize..end as usize]);
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct IsoFile {
    pub path: String,
    pub extent: u32,
    pub size: u32,
    /// Offset absoluto (na imagem) do directory record deste arquivo — o
    /// tamanho mora em +10 (LE) e +14 (BE).
    pub record_offset: usize,
}

/// Percorre o filesystem a partir do root directory record do PVD.
/// Records nao cruzam fronteira de setor (ECMA-119); ciclos sao cortados
/// por extent visitado e por caps de diretorios/arquivos.
pub fn walk(data: &[u8], map: SectorMap) -> Result<Vec<IsoFile>> {
    let pvd = read_sector(data, map, PVD_LBA)?;
    if &pvd[1..6] != b"CD001" {
        return Err(err("magic CD001 ausente no setor 16"));
    }
    let root = &pvd[156..190];
    let root_extent = read_u32_le(root, 2)?;
    let root_size = read_u32_le(root, 10)?;

    let mut files = Vec::new();
    let mut queue: Vec<(u32, u32, String)> = vec![(root_extent, root_size, String::new())];
    let mut visited: HashSet<u32> = HashSet::new();
    let mut dirs = 0usize;

    while let Some((extent, size, prefix)) = queue.pop() {
        if !visited.insert(extent) {
            continue;
        }
        dirs += 1;
        if dirs > MAX_DIRS {
            return Err(err("diretorios demais (filesystem corrompido?)"));
        }
        let sectors = (size as usize).div_ceil(SECTOR_DATA);
        for i in 0..sectors {
            let sector_base = sector_data_offset(data, map, extent as usize + i)?;
            let sector = read_sector(data, map, extent as usize + i)?;
            let limit = SECTOR_DATA.min(size as usize - i * SECTOR_DATA);
            let mut pos = 0usize;
            while pos < limit {
                let len = sector[pos] as usize;
                if len == 0 {
                    break; // padding ate o fim do setor
                }
                let record = sector
                    .get(pos..pos + len)
                    .ok_or_else(|| err("directory record truncado"))?;
                if record.len() < 34 {
                    return Err(err("directory record menor que o minimo"));
                }
                let entry_extent = read_u32_le(record, 2)?;
                let entry_size = read_u32_le(record, 10)?;
                let flags = record[25];
                let name_len = record[32] as usize;
                let name_bytes = record
                    .get(33..33 + name_len)
                    .ok_or_else(|| err("nome do record truncado"))?;
                let record_offset = sector_base + pos;
                pos += len;

                // "\0" = self, "\x01" = parent.
                if name_bytes == [0] || name_bytes == [1] {
                    continue;
                }
                let mut name = String::from_utf8_lossy(name_bytes).into_owned();
                if let Some(stripped) = name.split(';').next() {
                    name = stripped.to_string(); // remove ";1" de versao
                }
                if flags & 0x02 != 0 {
                    queue.push((entry_extent, entry_size, format!("{prefix}{name}/")));
                } else {
                    files.push(IsoFile {
                        path: format!("{prefix}{name}"),
                        extent: entry_extent,
                        size: entry_size,
                        record_offset,
                    });
                    if files.len() > MAX_FILES {
                        return Err(err("arquivos demais (filesystem corrompido?)"));
                    }
                }
            }
        }
    }
    Ok(files)
}

pub fn to_resources(data: &[u8], map: SectorMap, files: &[IsoFile]) -> Vec<ResourceDescriptor> {
    files
        .iter()
        .filter_map(|f| {
            let offset = sector_data_offset(data, map, f.extent as usize).ok()?;
            Some(ResourceDescriptor {
                path: f.path.clone(),
                offset: offset as u64,
                size: f.size as u64,
            })
        })
        .collect()
}

/// Identificacao da familia PlayStation pelo SYSTEM.CNF ("BOOT2" = PS2,
/// "BOOT" = PS1). `None` = sem SYSTEM.CNF legivel na regiao disponivel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsKind {
    One,
    Two,
}

/// Extracao conservadora comum aos adapters de disco optico: scan ASCII por
/// regiao de arquivo (no raw 2352, uma regiao por setor — strings que cruzam
/// setor sao perdidas, documentado), com resource_path e limite in-place.
pub fn extract_ascii_by_file(data: &[u8], map: SectorMap) -> Result<Vec<crate::types::TextEntry>> {
    use crate::scan::{scan_bytes, ScanConfig, ScanEncoding};
    const MAX_TEXT_ENTRIES: usize = 20_000;

    let files = walk(data, map)?;
    let mut entries: Vec<crate::types::TextEntry> = Vec::new();
    'outer: for file in files.iter().filter(|f| f.size > 0) {
        for (start, end) in file_regions(data, map, file.extent, file.size)? {
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
                e.resource_path = Some(file.path.clone());
                e.max_bytes = Some(e.original_bytes.len());
                e.context = Some("in-place: traducao limitada ao espaco original".to_string());
                entries.push(e);
            }
            if entries.len() >= MAX_TEXT_ENTRIES {
                break 'outer;
            }
        }
    }
    Ok(entries)
}

/// Um arquivo setor a setor: onde comecam os 2048 bytes de dados de cada
/// setor dele na imagem (no raw 2352 os setores nao sao contiguos).
struct FileLayout {
    sector_starts: Vec<usize>,
    size: usize,
}

impl FileLayout {
    fn new(data: &[u8], map: SectorMap, file: &IsoFile) -> Result<Self> {
        let size = file.size as usize;
        let sector_starts = (0..size.div_ceil(SECTOR_DATA))
            .map(|k| {
                let start = sector_data_offset(data, map, file.extent as usize + k)?;
                if start + SECTOR_DATA > data.len() {
                    return Err(err(format!("{}: setor {k} fora da imagem", file.path)));
                }
                Ok(start)
            })
            .collect::<Result<_>>()?;
        Ok(FileLayout {
            sector_starts,
            size,
        })
    }

    /// Setores inteiros do arquivo — o que o hardware do PS1 le.
    fn capacity(&self) -> usize {
        self.sector_starts.len() * SECTOR_DATA
    }

    fn rel_to_abs(&self, rel: usize) -> Option<usize> {
        self.sector_starts
            .get(rel / SECTOR_DATA)
            .map(|s| s + rel % SECTOR_DATA)
    }

    fn abs_to_rel(&self, abs: usize) -> Option<usize> {
        self.sector_starts
            .iter()
            .enumerate()
            .find(|(_, &s)| (s..s + SECTOR_DATA).contains(&abs))
            .map(|(k, &s)| k * SECTOR_DATA + abs - s)
    }

    /// A sobra depois do fim logico precisa estar zerada: qualquer byte ali
    /// pode ser dado escondido que o jogo usa.
    fn slack_is_zero(&self, image: &[u8]) -> bool {
        (self.size..self.capacity())
            .all(|rel| self.rel_to_abs(rel).and_then(|a| image.get(a)) == Some(&0))
    }

    /// Maior string (sem o terminador) que cabe na sobra, alinhada em 4.
    fn slack_room(&self) -> usize {
        self.capacity()
            .saturating_sub(self.size.next_multiple_of(4) + 1)
    }
}

/// PS1: string num arquivo de dados com ponteiro numa TABELA de offsets
/// relativos ao arquivo pode crescer pro espaco livre do ULTIMO SETOR dele —
/// o hardware le setores inteiros, entao essa sobra sempre chega na RAM. O
/// EXE fica de fora sozinho: ponteiro dele e endereco de RAM (0x80..), nao
/// offset, e ele divide a RAM com BSS e heap.
pub fn annotate_sector_slack(
    data: &[u8],
    map: SectorMap,
    entries: &mut [crate::types::TextEntry],
) -> Result<()> {
    let files = walk(data, map)?;
    let by_path: HashMap<&str, &IsoFile> = files.iter().map(|f| (f.path.as_str(), f)).collect();
    // `extract_ascii_by_file` devolve as entries de cada arquivo juntas.
    let mut i = 0;
    while i < entries.len() {
        let path = entries[i].resource_path.clone();
        let end = i + entries[i..]
            .iter()
            .take_while(|e| e.resource_path == path)
            .count();
        if let Some(file) = path.as_deref().and_then(|p| by_path.get(p)) {
            let layout = FileLayout::new(data, map, file)?;
            let room = layout.slack_room();
            if room > 0 && layout.slack_is_zero(data) {
                let bytes = read_file(data, map, file.extent, file.size)?;
                let found = file_relative_tables(
                    &bytes,
                    &entries[i..end],
                    |abs| layout.abs_to_rel(abs),
                    |rel| layout.rel_to_abs(rel),
                );
                for (k, pointers) in found {
                    let e = &mut entries[i + k];
                    if mark_relocatable(e, &pointers) {
                        e.max_bytes = Some(e.original_bytes.len().max(room));
                        e.context = Some(format!(
                            "realocavel: {} ponteiro(s) em tabela — cabe ate {room} bytes no \
                             espaco livre do setor final do arquivo (dividido entre as realocadas)",
                            pointers.len()
                        ));
                    }
                }
            }
        }
        i = end;
    }
    Ok(())
}

/// Grava as relocacoes na sobra do setor final de cada arquivo, reaponta a
/// tabela (offset relativo) e atualiza o tamanho no directory record.
fn relocate_into_slack(
    data: &[u8],
    map: SectorMap,
    out: &mut [u8],
    relocations: &[Relocation],
) -> Result<()> {
    let mut consumed = 0;
    for file in walk(data, map)?.into_iter().filter(|f| f.size > 0) {
        let layout = FileLayout::new(data, map, &file)?;
        let in_file = |abs: usize| layout.abs_to_rel(abs).filter(|&r| r < layout.size);
        let mine: Vec<Relocation> = relocations
            .iter()
            .filter(|r| in_file(r.original_offset).is_some())
            .cloned()
            .collect();
        if mine.is_empty() {
            continue;
        }
        consumed += mine.len();
        if !layout.slack_is_zero(out) {
            return Err(err(format!(
                "{}: o espaco livre do setor final nao esta zerado (pode ser dado escondido) \
                 — relocacao recusada",
                file.path
            )));
        }
        let relative = to_file_relative(&mine, |abs| layout.abs_to_rel(abs))?;
        let mut bytes = read_file(out, map, file.extent, file.size)?;
        relocate(&mut bytes, &relative, 0, layout.capacity()).map_err(|e| {
            err(format!(
                "{} (espaco livre no setor final: {} bytes): {e}",
                file.path,
                layout.capacity() - layout.size
            ))
        })?;
        for (k, chunk) in bytes.chunks(SECTOR_DATA).enumerate() {
            let start = layout.sector_starts[k];
            out[start..start + chunk.len()].copy_from_slice(chunk);
        }
        write_file_size(out, file.record_offset, bytes.len())?;
    }
    if consumed != relocations.len() {
        return Err(err(
            "string realocavel fora de qualquer arquivo do filesystem",
        ));
    }
    Ok(())
}

/// ISO 9660 guarda o tamanho nos dois endians: +10 LE, +14 BE.
fn write_file_size(out: &mut [u8], record: usize, size: usize) -> Result<()> {
    let size = u32::try_from(size).map_err(|_| err("arquivo passaria de 4 GiB"))?;
    let field = out
        .get_mut(record + 10..record + 18)
        .ok_or_else(|| err("directory record fora da imagem"))?;
    field[..4].copy_from_slice(&size.to_le_bytes());
    field[4..].copy_from_slice(&size.to_be_bytes());
    Ok(())
}

/// Reinsercao comum: in-place; com `allow_relocation` (PS1), string com
/// ponteiro em tabela que nao cabe vai pra sobra do setor final do arquivo.
/// Em raw 2352 (BIN) cada setor alterado tem o EDC/ECC regenerado
/// (ECMA-130, modulo `cdrom`) depois de todas as escritas.
pub fn apply_iso(
    data: &[u8],
    map: SectorMap,
    entries: &[crate::types::TextEntry],
    allow_relocation: bool,
) -> Result<crate::adapter::AppliedImage> {
    let plan = super::inplace::plan_in_place(data, entries, allow_relocation)?;
    let mut out = data.to_vec();
    for (offset, patch) in &plan.writes {
        out[*offset..offset + patch.len()].copy_from_slice(patch);
    }
    ensure_pointers_untouched(data, &out, entries)?;
    if !plan.relocations.is_empty() {
        relocate_into_slack(data, map, &mut out, &plan.relocations)?;
    }
    if map == SectorMap::Raw2352 {
        let sectors = out.as_chunks_mut::<SECTOR_RAW>().0;
        for (i, sector) in sectors.iter_mut().enumerate() {
            if sector[..] != data[i * SECTOR_RAW..(i + 1) * SECTOR_RAW] {
                super::cdrom::regenerate_sector(sector)?;
            }
        }
    }
    Ok(crate::adapter::AppliedImage {
        bytes: out,
        report: plan.report,
    })
}

/// Confere o EDC de todos os setores de dados de uma imagem raw 2352.
/// Setor sem o sync pattern nao e setor de dados (track de audio num dump
/// single-file, por exemplo) e entra no terceiro contador, nao em "invalido".
/// Retorna (validos, invalidos, sem_sync); setores sem EDC nao contam.
pub fn raw_edc_scan(data: &[u8]) -> (usize, usize, usize) {
    let mut ok = 0;
    let mut bad = 0;
    let mut no_sync = 0;
    for sector in data.as_chunks::<SECTOR_RAW>().0 {
        if sector[..SYNC.len()] != SYNC {
            no_sync += 1;
            continue;
        }
        match super::cdrom::sector_edc_ok(sector) {
            Some(true) => ok += 1,
            Some(false) => bad += 1,
            None => {}
        }
    }
    (ok, bad, no_sync)
}

pub fn identify_playstation(data: &[u8], map: SectorMap) -> Option<(PsKind, String)> {
    let files = walk(data, map).ok()?;
    let cnf = files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case("SYSTEM.CNF"))?;
    let content = read_file(data, map, cnf.extent, cnf.size.min(4096)).ok()?;
    let text = String::from_utf8_lossy(&content);
    let boot_line = text
        .lines()
        .find(|l| l.trim_start().to_ascii_uppercase().starts_with("BOOT"))?
        .trim()
        .to_string();
    let upper = boot_line.to_ascii_uppercase();
    if upper.starts_with("BOOT2") {
        Some((PsKind::Two, boot_line))
    } else {
        Some((PsKind::One, boot_line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::GameAdapter;
    use crate::adapters::ps1::Ps1Adapter;
    use crate::synth;

    #[test]
    fn slack_relocation_updates_size_in_both_endians() {
        let bin = synth::make_ps1_bin_with_message_table();
        let mut entries = Ps1Adapter.extract_structured(&bin).unwrap();
        for e in entries.iter_mut().filter(|e| {
            e.resource_path.as_deref() == Some("MSG.DAT") && e.source_text == "NEW GAME"
        }) {
            e.translated_text = Some("NOVO JOGO".into());
        }
        let out = Ps1Adapter.apply_text(&bin, &entries).unwrap().bytes;
        let msg = walk(&out, detect_map(&out))
            .unwrap()
            .into_iter()
            .find(|f| f.path == "MSG.DAT")
            .unwrap();
        let r = msg.record_offset;
        let le = u32::from_le_bytes(out[r + 10..r + 14].try_into().unwrap());
        let be = u32::from_be_bytes(out[r + 14..r + 18].try_into().unwrap());
        assert_eq!(le, be, "ISO 9660 exige o mesmo tamanho nos dois endians");
        assert!(le > 0x38, "arquivo cresceu pra sobra do setor: {le}");
    }
}

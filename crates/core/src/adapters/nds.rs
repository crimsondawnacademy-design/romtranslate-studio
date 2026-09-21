//! Adapter de Nintendo DS (GBATEK: cartridge header + filesystem FNT/FAT).
//!
//! Deteccao: CRC-16 do header (0x000..0x15E), campo do logo CRC == 0xCF56
//! (conferimos o CAMPO documentado — o bitmap do logo, material Nintendo,
//! nao entra no repo) e estrutura FNT/FAT dentro dos limites.
//!
//! Extracao: lista o filesystem e roda o scanner (ASCII + UTF-16LE, o encoding
//! tipico de texto em DS) POR ARQUIVO, com `resource_path` preenchido.
//! Reinsercao: in-place (mesmo espaco), CRC do header recalculado. Traducao
//! maior passa se a string tiver ponteiros numa TABELA de offsets relativos
//! ao arquivo: o arquivo cresce e vai pro fim do ROM com a FAT reapontada.
//! O binario ARM9 fica de fora (nem e extraido): ele divide a RAM principal
//! com BSS e heap, e texto anexado nele seria zerado no boot.

use super::pointers::{
    ensure_pointers_untouched, file_relative_tables, mark_relocatable, relocate, to_file_relative,
    Relocation,
};
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
/// GBATEK: 014h = capacidade (128 KB << n); 080h = total usado do ROM.
const CAPACITY_OFFSET: usize = 0x14;
const USED_SIZE_OFFSET: usize = 0x80;
/// Unidade da capacidade do chip (GBATEK: 128 KB << n).
const CHIP_UNIT: usize = 128 * 1024;
/// Maior cartao de DS: 4 Gbit.
const MAX_ROM_LEN: usize = 512 * 1024 * 1024;
/// Alinhamento de arquivo que as ferramentas de rebuild usam (bloco de leitura
/// do cartao); o resto e preenchido com 0xFF, como o padding do proprio ROM.
const FILE_ALIGN: usize = 0x200;
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

/// Uma leitura por trecho. Texto ASCII lido como UTF-16LE vira "CJK" falso
/// na MESMA posicao (e o id e so o offset): no banco as duas leituras se
/// fundiam e a traducao de uma era gravada com o encoding da outra —
/// "NOVO JOGO" virava N\0O\0V\0... por cima do ASCII. O inverso tambem
/// existe: kana UTF-16 lido como ASCII vira "B0D0F0". Regra: se as strings
/// ASCII cobrem >= 3/4 do run UTF-16 e ele nao tem cara de kana (byte alto
/// 0x30 em metade das unidades), e texto ASCII; senao, e UTF-16.
/// ponytail: heuristica — texto japones UTF-16 so de kanji com bytes
/// imprimiveis pode ser lido como ASCII; resolver por idioma de origem se
/// isso aparecer em jogo real.
fn resolve_encoding_overlaps(ascii: Vec<TextEntry>, utf16: Vec<TextEntry>) -> Vec<TextEntry> {
    let span = |e: &TextEntry| {
        let start = e.offset.unwrap_or(0) as usize;
        (start, start + e.original_bytes.len())
    };
    let mut drop_ascii = vec![false; ascii.len()];
    let mut kept_utf16 = Vec::new();
    for u in utf16 {
        let (us, ue) = span(&u);
        // Runs de um scan sao ordenados e disjuntos: busca binaria no inicio.
        let first = ascii.partition_point(|a| span(a).1 <= us);
        let hits: Vec<usize> = (first..ascii.len())
            .take_while(|&i| span(&ascii[i]).0 < ue)
            .collect();
        if hits.is_empty() {
            kept_utf16.push(u);
            continue;
        }
        let covered: usize = hits
            .iter()
            .map(|&i| {
                let (s, e) = span(&ascii[i]);
                e.min(ue) - s.max(us)
            })
            .sum();
        let units = u.original_bytes.as_chunks::<2>().0;
        let kana = units.iter().filter(|c| c[1] == 0x30).count() * 2 >= units.len();
        if covered * 4 >= (ue - us) * 3 && !kana {
            continue; // ASCII lido como UTF-16: descarta o run falso
        }
        for i in hits {
            drop_ascii[i] = true;
        }
        kept_utf16.push(u);
    }
    ascii
        .into_iter()
        .zip(drop_ascii)
        .filter(|(_, dropped)| !dropped)
        .map(|(a, _)| a)
        .chain(kept_utf16)
        .collect()
}

/// Traducao que nao cabe cresce o ARQUIVO dela: a copia nova (strings
/// anexadas + tabela reapontada) vai pro fim do ROM e a FAT passa a apontar
/// pra ela — o que as ferramentas de rebuild de DS fazem, ja que o jogo acha
/// arquivo pela FAT. A copia antiga fica no lugar, inofensiva.
fn relocate_files(data: &[u8], out: &mut Vec<u8>, relocations: &[Relocation]) -> Result<()> {
    let layout = fs_layout(data)?;
    let mut files: Vec<(usize, usize, String)> = walk_filesystem(data, &layout)?
        .into_iter()
        .map(|f| (f.offset as usize, f.size as usize, f.path))
        .collect();
    // Arquivos-alias (mesmo range na FAT) sao um so: move uma vez.
    files.sort();
    files.dedup_by_key(|f| (f.0, f.1));

    let mut consumed = 0;
    for (start, len, path) in files {
        let in_file = |abs: usize| abs.checked_sub(start).filter(|&r| r < len);
        let mine: Vec<Relocation> = relocations
            .iter()
            .filter(|r| in_file(r.original_offset).is_some())
            .cloned()
            .collect();
        if mine.is_empty() {
            continue;
        }
        consumed += mine.len();
        let relative = to_file_relative(&mine, in_file)?;
        let mut bytes = out[start..start + len].to_vec();
        let new_start = out.len().next_multiple_of(FILE_ALIGN);
        let room = MAX_ROM_LEN
            .checked_sub(new_start)
            .ok_or_else(|| err("ROM ja no tamanho maximo de um cartao de DS"))?;
        relocate(&mut bytes, &relative, 0, room).map_err(|e| err(format!("{path}: {e}")))?;
        out.resize(new_start, 0xFF);
        out.extend_from_slice(&bytes);
        let new_end = out.len();
        repoint_fat(out, &layout, (start, start + len), (new_start, new_end))?;
    }
    if consumed != relocations.len() {
        return Err(err(
            "string realocavel fora de qualquer arquivo do filesystem",
        ));
    }

    let used = u32::try_from(out.len()).map_err(|_| err("ROM passaria de 4 GiB"))?;
    out[USED_SIZE_OFFSET..USED_SIZE_OFFSET + 4].copy_from_slice(&used.to_le_bytes());
    while out[CAPACITY_OFFSET] < 12 && CHIP_UNIT << out[CAPACITY_OFFSET] < out.len() {
        out[CAPACITY_OFFSET] += 1;
    }
    Ok(())
}

/// Troca (start, end) de toda entrada da FAT que apontava pro arquivo antigo.
fn repoint_fat(
    out: &mut [u8],
    layout: &FsLayout,
    old: (usize, usize),
    new: (usize, usize),
) -> Result<()> {
    let to_u32 = |v: usize| u32::try_from(v).map_err(|_| err("offset passa de 4 GiB"));
    let old = (to_u32(old.0)?, to_u32(old.1)?);
    let (new_start, new_end) = (to_u32(new.0)?, to_u32(new.1)?);
    let mut hits = 0;
    for k in 0..layout.fat_size / 8 {
        let at = layout.fat_offset + k * 8;
        if (read_u32(out, at)?, read_u32(out, at + 4)?) == old {
            out[at..at + 4].copy_from_slice(&new_start.to_le_bytes());
            out[at + 4..at + 8].copy_from_slice(&new_end.to_le_bytes());
            hits += 1;
        }
    }
    if hits == 0 {
        return Err(err("entrada da FAT do arquivo realocado nao encontrada"));
    }
    Ok(())
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
            reinsert: true, // in-place; relocacao via crescimento do arquivo + FAT
            patch: true,
            compression: false,
            pointer_relocation: true,
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
        // (inicio, fim, faixa de entries) de cada ARQUIVO — o header nao e
        // arquivo da FAT e nao reloca.
        let mut spans: Vec<(usize, usize, std::ops::Range<usize>)> = Vec::new();
        for (path, start, end) in regions {
            let first = entries.len();
            let remaining = MAX_ENTRIES_TOTAL - entries.len();
            let scan = |encoding| {
                scan_bytes(
                    data,
                    &ScanConfig {
                        encoding,
                        region_start: Some(start),
                        region_end: Some(end),
                        max_entries: remaining,
                        ..ScanConfig::default()
                    },
                )
                .map(|o| o.entries)
            };
            let mut found =
                resolve_encoding_overlaps(scan(ScanEncoding::Ascii)?, scan(ScanEncoding::Utf16Le)?);
            found.truncate(remaining);
            for mut e in found {
                e.resource_path = Some(path.clone());
                e.max_bytes = Some(e.original_bytes.len());
                e.context = Some("in-place: traducao limitada ao espaco original".to_string());
                entries.push(e);
            }
            if path != "header" {
                spans.push((start as usize, end as usize, first..entries.len()));
            }
            if entries.len() >= MAX_ENTRIES_TOTAL {
                break;
            }
        }

        for (start, end, range) in spans {
            let len = end - start;
            let found = file_relative_tables(
                &data[start..end],
                &entries[range.clone()],
                |abs| abs.checked_sub(start).filter(|&r| r < len),
                |rel| (rel < len).then_some(start + rel),
            );
            for (i, pointers) in found {
                let e = &mut entries[range.start + i];
                if mark_relocatable(e, &pointers) {
                    e.max_bytes = None;
                    e.context = Some(format!(
                        "realocavel: {} ponteiro(s) em tabela no arquivo — se nao couber, o \
                         arquivo cresce",
                        pointers.len()
                    ));
                }
            }
        }
        Ok(entries)
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        if data.len() < HEADER_LEN {
            return Err(err("arquivo menor que o header NDS"));
        }
        let plan = super::inplace::plan_in_place(data, entries, true)?;
        let mut out = data.to_vec();
        for (offset, patch) in &plan.writes {
            out[*offset..offset + patch.len()].copy_from_slice(patch);
        }
        ensure_pointers_untouched(data, &out, entries)?;
        if !plan.relocations.is_empty() {
            relocate_files(data, &mut out, &plan.relocations)?;
        }
        // Titulo (0x00..0x0C) e os campos de tamanho mudam o CRC do header.
        let crc = crc16(&out[..HEADER_CRC_OFFSET]);
        out[HEADER_CRC_OFFSET..HEADER_CRC_OFFSET + 2].copy_from_slice(&crc.to_le_bytes());
        Ok(AppliedImage {
            bytes: out,
            report: plan.report,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TextEncoding;

    /// (leitura e ASCII?, texto) de cada string que sobrevive ao desempate.
    fn resolve(buf: &[u8]) -> Vec<(bool, String)> {
        let scan = |encoding| {
            let config = ScanConfig {
                encoding,
                ..ScanConfig::default()
            };
            scan_bytes(buf, &config).unwrap().entries
        };
        resolve_encoding_overlaps(scan(ScanEncoding::Ascii), scan(ScanEncoding::Utf16Le))
            .into_iter()
            .map(|e| (e.encoding == TextEncoding::Ascii, e.source_text))
            .collect()
    }

    fn utf16(text: &str) -> Vec<u8> {
        let mut bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    #[test]
    fn one_reading_per_byte_range() {
        // ASCII lido como UTF-16 vira CJK falso: fica so a leitura ASCII.
        assert_eq!(
            resolve(b"WELCOME HOME!\0\0\0"),
            vec![(true, "WELCOME HOME!".into())]
        );
        // Kana UTF-16 lido como ASCII vira "B0D0F0": fica so a leitura UTF-16.
        assert_eq!(
            resolve(&utf16("あいうえお")),
            vec![(false, "あいうえお".into())]
        );
        // UTF-16 latino nao colide com ASCII (o 0x00 quebra o run): fica.
        assert_eq!(
            resolve(&utf16("START GAME")),
            vec![(false, "START GAME".into())]
        );
    }
}

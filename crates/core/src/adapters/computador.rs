//! Computadores japoneses: PC-9801/9821, PC-8801 e Sharp X68000. O jogo vem
//! em imagem de disquete ou de disco rigido; uma ficha por maquina
//! (`Machine`), o formato da imagem decide so onde os setores moram:
//!
//! - D88/D77 (PC-98, PC-88): cabecalho de 0x2B0 bytes com a tabela das
//!   trilhas; cada setor tem um cabecalho de 16 bytes na frente dos dados, e
//!   os setores de uma trilha podem estar gravados fora de ordem. Uma imagem
//!   pode trazer varios discos seguidos.
//! - FDI e HDI (Anex86, PC-98): cabecalho de 32 bytes (tamanho do cabecalho,
//!   dos dados e a geometria) e a imagem crua.
//! - DIM (X68000, DIFC.X): cabecalho de 0x100 bytes, tipo no byte 0, uma
//!   marca por trilha em 1..=0xAA e "DIFC HEADER" em 0xAB; trilha vazia pode
//!   ficar fora do arquivo.
//! - XDF/HDM: 2HD cru de 1.232 setores de 1 KiB, sem cabecalho.
//!
//! Dentro do disco, na ordem logica dos setores: com FAT12/FAT16 (MS-DOS do
//! PC-98, Human68k do X68000) a extracao e por arquivo; sem FAT (o PC-88, o
//! jogo de formato proprio) o disco inteiro e um bloco. Texto em ASCII e
//! Shift-JIS, regravado no espaco da original. A posicao de cada frase e a
//! logica (`metadata.logical`, `metadata.disk`); a reinsercao mapeia de volta
//! pro arquivo, e a frase que atravessa o fim de um setor do D88 e gravada
//! nos dois pedacos.
//!
//! Lista de pedidos do Data Crystal (20/10/2026, so contagens): PC-9801/9821
//! 80 jogos, X68000 26, PC-8801 23, quase todos japoneses.

use std::borrow::Cow;
use std::collections::HashSet;

use super::inplace::plan_in_place;
use crate::adapter::{
    AppliedImage, ApplyReport, Extraction, GameAdapter, GameInput, VerificationReport,
};
use crate::error::{CoreError, Result};
use crate::scan::{scan_bytes, ScanConfig, ScanEncoding, MAX_ENTRIES};
use crate::types::{
    AdapterCapabilities, Platform, ProbeResult, ResourceDescriptor, SupportLevel, TextEntry,
};

pub struct Machine {
    id: &'static str,
    name: &'static str,
    platform: Platform,
}

pub static MACHINES: [Machine; 3] = [
    Machine {
        id: "pc98.disk",
        name: "PC-9801 / PC-9821",
        platform: Platform::Pc98,
    },
    Machine {
        id: "pc88.disk",
        name: "PC-8801",
        platform: Platform::Pc88,
    },
    Machine {
        id: "x68k.disk",
        name: "Sharp X68000",
        platform: Platform::X68000,
    },
];

pub struct Computador(pub &'static Machine);

fn le16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        d.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn le32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        d.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

// --- Os discos da imagem ----------------------------------------------------

/// `len` bytes contiguos no arquivo: na ordem logica do disco comecam em
/// `logical`; no arquivo, em `at`.
#[derive(Debug, Clone, Copy)]
struct Piece {
    logical: usize,
    at: usize,
    len: usize,
}

/// Um disco da imagem, setores na ordem logica. Buraco entre pedacos (trilha
/// fora do DIM) le como zero.
#[derive(Debug)]
struct Disk {
    format: &'static str,
    /// Tipo de midia do D88 (0x00 = 2D, 0x10 = 2DD, 0x20 = 2HD).
    media: Option<u8>,
    pieces: Vec<Piece>,
    len: usize,
}

impl Disk {
    fn contiguous(format: &'static str, at: usize, len: usize) -> Disk {
        Disk {
            format,
            media: None,
            pieces: vec![Piece {
                logical: 0,
                at,
                len,
            }],
            len,
        }
    }

    /// Os bytes na ordem logica; sem copia quando ja sao um pedaco so.
    fn logical<'a>(&self, data: &'a [u8]) -> Cow<'a, [u8]> {
        if let [p] = self.pieces[..] {
            if p.logical == 0 && p.len == self.len {
                return Cow::Borrowed(data.get(p.at..p.at + p.len).unwrap_or_default());
            }
        }
        let mut out = vec![0u8; self.len];
        for p in &self.pieces {
            if let (Some(src), Some(dst)) = (
                data.get(p.at..p.at + p.len),
                out.get_mut(p.logical..p.logical + p.len),
            ) {
                dst.copy_from_slice(src);
            }
        }
        Cow::Owned(out)
    }

    /// Onde a posicao logica mora no arquivo.
    fn at(&self, logical: usize) -> Option<usize> {
        let i = self
            .pieces
            .partition_point(|p| p.logical <= logical)
            .checked_sub(1)?;
        let p = self.pieces[i];
        (logical < p.logical + p.len).then(|| p.at + logical - p.logical)
    }

    /// Grava `bytes` na posicao logica `at`, pedaco por pedaco.
    fn write(&self, out: &mut [u8], at: usize, bytes: &[u8]) {
        let end = at + bytes.len();
        for p in &self.pieces {
            let (from, to) = (at.max(p.logical), end.min(p.logical + p.len));
            if from < to {
                let dst = p.at + from - p.logical;
                if let Some(dst) = out.get_mut(dst..dst + to - from) {
                    dst.copy_from_slice(&bytes[from - at..to - at]);
                }
            }
        }
    }
}

const D88_HEADER: usize = 0x2B0;
const D88_TRACKS: usize = 164;
/// 2D, 2DD, 2HD, 1D, 1DD.
const D88_MEDIA: [u8; 5] = [0x00, 0x10, 0x20, 0x30, 0x40];
/// Mais setores numa trilha que isto e lixo, nao disquete.
const MAX_SECTORS: usize = 64;

/// Um disco D88 em `base`, e o tamanho dele.
fn d88_disk(data: &[u8], base: usize) -> Option<(Disk, usize)> {
    let h = data.get(base..base.checked_add(D88_HEADER)?)?;
    let size = le32(h, 0x1C)? as usize;
    let media = h[0x1B];
    if !D88_MEDIA.contains(&media) || size < D88_HEADER || size > data.len() - base {
        return None;
    }
    let disk = &data[base..base + size];
    let offsets: Vec<usize> = (0..D88_TRACKS)
        .map(|i| le32(h, 0x20 + 4 * i).unwrap_or(0) as usize)
        .collect();
    // Imagem antiga tem cabecalho menor: a tabela acaba onde a primeira
    // trilha comeca.
    let first = offsets.iter().copied().filter(|&o| o != 0).min()?;
    if first < 0x20 + 4 {
        return None;
    }
    let tracks = ((first - 0x20) / 4).min(D88_TRACKS);
    let mut pieces = Vec::new();
    let mut logical = 0;
    for &start in offsets[..tracks].iter().filter(|&&o| o != 0) {
        // (setor R, posicao dos dados, tamanho)
        let mut sectors: Vec<(u8, usize, usize)> = Vec::new();
        let (mut pos, mut count) = (start, 1);
        while sectors.len() < count {
            let s = disk.get(pos..pos + 16)?;
            if sectors.is_empty() {
                count = le16(s, 4)? as usize;
                if count == 0 || count > MAX_SECTORS {
                    return None;
                }
            }
            let len = le16(s, 14)? as usize;
            if pos + 16 + len > disk.len() {
                return None;
            }
            sectors.push((s[2], base + pos + 16, len));
            pos += 16 + len;
        }
        // Na trilha os setores podem estar intercalados; a ordem logica e a
        // do numero R.
        sectors.sort_by_key(|s| s.0);
        for (_, at, len) in sectors.into_iter().filter(|s| s.2 > 0) {
            pieces.push(Piece { logical, at, len });
            logical += len;
        }
    }
    (!pieces.is_empty()).then_some((
        Disk {
            format: "D88",
            media: Some(media),
            pieces,
            len: logical,
        },
        size,
    ))
}

/// Os discos de uma imagem D88; ela tem que acabar no fim do ultimo.
fn d88(data: &[u8]) -> Option<Vec<Disk>> {
    let mut disks = Vec::new();
    let mut base = 0;
    while base < data.len() {
        let (disk, size) = d88_disk(data, base)?;
        disks.push(disk);
        base += size;
    }
    (!disks.is_empty()).then_some(disks)
}

/// FDI ou HDI do Anex86: o cabecalho diz o tamanho de tudo.
fn anex86(data: &[u8]) -> Option<Disk> {
    let field = |i: usize| le32(data, i * 4).map(|v| v as usize);
    let (header, size, sector) = (field(2)?, field(3)?, field(4)?);
    let geometry = field(5)?
        .checked_mul(field(6)?)?
        .checked_mul(field(7)?)?
        .checked_mul(sector)?;
    let valid = field(0)? == 0
        && (32..=0x10000).contains(&header)
        && size > 0
        && header.checked_add(size)? == data.len()
        && sector.is_power_of_two()
        && (128..=2048).contains(&sector)
        && geometry == size;
    let format = if size > 4 << 20 { "HDI" } else { "FDI" };
    valid.then(|| Disk::contiguous(format, header, size))
}

const DIM_HEADER: usize = 0x100;
const DIM_TRACKS: usize = 154;

/// DIM do X68000. Com todas as trilhas no arquivo, e cru; senao, so as
/// marcadas no cabecalho estao la.
fn dim(data: &[u8]) -> Option<Disk> {
    if data.get(0xAB..0xAB + 11)? != b"DIFC HEADER" {
        return None;
    }
    // Setores por trilha e tamanho, pelo tipo (o mesmo quadro do MAME).
    let (spt, bps) = match data[0] {
        1 | 3 => (9, 1024),
        2 => (15, 512),
        9 => (18, 512),
        17 => (26, 256),
        _ => (8, 1024),
    };
    let track = spt * bps;
    let body = data.len().checked_sub(DIM_HEADER)?;
    if body == DIM_TRACKS * track {
        return Some(Disk::contiguous("DIM", DIM_HEADER, body));
    }
    let mut pieces = Vec::new();
    let mut at = DIM_HEADER;
    for t in (0..DIM_TRACKS).filter(|t| data[1 + t] != 0) {
        pieces.push(Piece {
            logical: t * track,
            at,
            len: track,
        });
        at += track;
    }
    (at == data.len() && !pieces.is_empty()).then_some(Disk {
        format: "DIM",
        media: None,
        pieces,
        len: DIM_TRACKS * track,
    })
}

/// 2HD de 1.232 setores de 1 KiB: o do PC-98 (MS-DOS) e o do X68000.
const RAW_2HD: usize = 1024 * 8 * 77 * 2;

/// Os discos da imagem, no primeiro formato que ela fecha.
fn disks(data: &[u8]) -> Option<Vec<Disk>> {
    d88(data)
        .or_else(|| anex86(data).map(|d| vec![d]))
        .or_else(|| dim(data).map(|d| vec![d]))
        .or_else(|| (data.len() == RAW_2HD).then(|| vec![Disk::contiguous("XDF", 0, RAW_2HD)]))
}

// --- FAT12/FAT16 --------------------------------------------------------------

#[derive(Debug)]
struct Fat {
    fat_at: usize,
    root_at: usize,
    root_entries: usize,
    data_at: usize,
    cluster: usize,
    clusters: usize,
    fat16: bool,
}

struct FatFile {
    path: String,
    /// Faixas logicas do conteudo, clusters seguidos juntos.
    runs: Vec<(usize, usize)>,
    size: usize,
}

const MAX_FILES: usize = 65_536;
const MAX_DIRS: usize = 4096;
/// Num HD do PC-98 o volume comeca depois do IPL e da tabela de particoes:
/// procura-se o setor de boot nos primeiros 2 MiB.
const HD_SEARCH: usize = 2 << 20;

impl Fat {
    /// Volume FAT no disco: setor de boot do MS-DOS (no comeco ou, num HD, na
    /// primeira particao) ou o 2HD padrao.
    fn find(d: &[u8]) -> Option<Fat> {
        let window = if d.len() > 4 << 20 { HD_SEARCH } else { 1 };
        // ponytail: so o primeiro volume de um HD; ler a tabela de
        // particoes do PC-98 se um jogo vier em outra.
        (0..window.min(d.len()))
            .step_by(256)
            .find_map(|at| Fat::bpb(d, at))
            .or_else(|| Fat::standard_2hd(d))
    }

    fn bpb(d: &[u8], at: usize) -> Option<Fat> {
        let b = d.get(at..at + 0x24)?;
        if !matches!(b[0], 0xEB | 0xE9) {
            return None;
        }
        let bps = le16(b, 0x0B)? as usize;
        let spc = b[0x0D] as usize;
        let reserved = le16(b, 0x0E)? as usize;
        let fats = b[0x10] as usize;
        let root = le16(b, 0x11)? as usize;
        let total = match le16(b, 0x13)? {
            0 => le32(b, 0x20)? as usize,
            n => n as usize,
        };
        let media = b[0x15];
        let spf = le16(b, 0x16)? as usize;
        let valid = [256, 512, 1024, 2048].contains(&bps)
            && spc.is_power_of_two()
            && reserved > 0
            && (1..=2).contains(&fats)
            && root > 0
            && spf > 0
            && total > 0
            && media >= 0xF0
            // O primeiro byte da FAT repete o tipo de midia.
            && d.get(at + reserved * bps) == Some(&media);
        if !valid {
            return None;
        }
        Fat::layout(d, at, bps, spc, reserved, fats, root, spf, total)
    }

    /// Sem setor de boot do MS-DOS (Human68k do X68000, disco de sistema
    /// antigo do PC-98): o 2HD de 1.232 setores de 1 KiB tem sempre o mesmo
    /// desenho, conferido pelo comeco da FAT no setor 1.
    fn standard_2hd(d: &[u8]) -> Option<Fat> {
        let marked = d.len() >= RAW_2HD
            && d.get(1024).is_some_and(|m| *m >= 0xF8)
            && d.get(1025..1027) == Some(&[0xFF, 0xFF][..]);
        if !marked {
            return None;
        }
        Fat::layout(d, 0, 1024, 1, 1, 2, 192, 2, 1232)
    }

    #[allow(clippy::too_many_arguments)]
    fn layout(
        d: &[u8],
        start: usize,
        bps: usize,
        spc: usize,
        reserved: usize,
        fats: usize,
        root: usize,
        spf: usize,
        total: usize,
    ) -> Option<Fat> {
        let fat_at = start + reserved * bps;
        let root_at = fat_at + fats * spf * bps;
        let data_at = root_at + (root * 32).div_ceil(bps) * bps;
        let cluster = spc * bps;
        let clusters = (start + total * bps).saturating_sub(data_at) / cluster;
        let fat16 = clusters >= 4085;
        // FAT32 fica de fora: nao existe nessas maquinas.
        (root_at + root * 32 <= d.len() && clusters > 0 && clusters < 65_525).then_some(Fat {
            fat_at,
            root_at,
            root_entries: root,
            data_at,
            cluster,
            clusters,
            fat16,
        })
    }

    fn next(&self, d: &[u8], c: usize) -> usize {
        let v = if self.fat16 {
            le16(d, self.fat_at + 2 * c)
        } else {
            le16(d, self.fat_at + c + c / 2).map(|v| if c & 1 == 1 { v >> 4 } else { v & 0xFFF })
        };
        v.unwrap_or(0) as usize
    }

    /// A corrente de clusters a partir de `first`; corta em laco.
    fn chain(&self, d: &[u8], first: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut c = first;
        while (2..self.clusters + 2).contains(&c) && out.len() < self.clusters {
            out.push(c);
            c = self.next(d, c);
        }
        out
    }

    /// Faixas logicas dos clusters, seguidos juntos, cortadas em `size`.
    fn runs(&self, chain: &[usize], size: usize) -> Vec<(usize, usize)> {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        let mut left = size;
        for &c in chain {
            if left == 0 {
                break;
            }
            let start = self.data_at + (c - 2) * self.cluster;
            let len = self.cluster.min(left);
            left -= len;
            match runs.last_mut() {
                Some(run) if run.1 == start => run.1 += len,
                _ => runs.push((start, start + len)),
            }
        }
        runs
    }

    fn files(&self, d: &[u8]) -> Vec<FatFile> {
        let mut out = Vec::new();
        let root = vec![(self.root_at, self.root_at + self.root_entries * 32)];
        let mut dirs = vec![(root, String::new())];
        let mut seen = HashSet::new();
        while let Some((runs, prefix)) = dirs.pop() {
            let bytes: Vec<u8> = runs
                .iter()
                .flat_map(|&(a, b)| d.get(a..b.min(d.len())).unwrap_or_default())
                .copied()
                .collect();
            for e in bytes.as_chunks::<32>().0 {
                match e[0] {
                    0 => break,
                    0xE5 | b'.' => continue,
                    _ => {}
                }
                let attr = e[11];
                // Rotulo do volume e nome longo.
                if attr & 0x08 != 0 {
                    continue;
                }
                let first = le16(e, 0x1A).unwrap_or(0) as usize;
                let chain = self.chain(d, first);
                let name = dos_name(e);
                if attr & 0x10 != 0 {
                    if seen.insert(first) && seen.len() <= MAX_DIRS {
                        let size = chain.len() * self.cluster;
                        dirs.push((self.runs(&chain, size), format!("{prefix}{name}/")));
                    }
                    continue;
                }
                let size = le32(e, 0x1C).unwrap_or(0) as usize;
                out.push(FatFile {
                    path: format!("{prefix}{name}"),
                    runs: self.runs(&chain, size),
                    size,
                });
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
        out
    }
}

/// Nome 8.3 em Shift-JIS (0x05 no primeiro byte e o 0xE5 de um kanji).
// ponytail: o nome longo do Human68k (mais 10 bytes em 0x0C) fica de fora;
// o caminho so aparece na tela.
fn dos_name(e: &[u8]) -> String {
    let mut name = e[..8].to_vec();
    if name[0] == 0x05 {
        name[0] = 0xE5;
    }
    let decode = |bytes: &[u8]| {
        let mut s = String::new();
        let mut i = 0;
        while i < bytes.len() {
            match crate::sjis::decode_char(bytes, i) {
                Some((c, n)) => {
                    s.push(c);
                    i += n;
                }
                None => {
                    s.push('?');
                    i += 1;
                }
            }
        }
        s.trim_end().to_string()
    };
    let (base, ext) = (decode(&name), decode(&e[8..11]));
    if ext.is_empty() {
        base
    } else {
        format!("{base}.{ext}")
    }
}

// --- Qual maquina ---------------------------------------------------------------

/// A maquina do disco e por que. Extensao propria da maquina decide; depois o
/// setor de boot (68000 no X68000, x86 no PC-98) e o tipo de midia (o PC-88 e
/// o do disquete 2D).
fn machine(disk: &Disk, logical: &[u8], extension: &str) -> (Platform, &'static str) {
    match extension {
        "88d" => return (Platform::Pc88, "extensao .88d"),
        "98d" | "hdm" | "fdi" | "hdi" => return (Platform::Pc98, "extensao do PC-98"),
        "xdf" | "dim" => return (Platform::X68000, "extensao do X68000"),
        _ => {}
    }
    if matches!(disk.format, "FDI" | "HDI") {
        return (Platform::Pc98, "imagem do Anex86, emulador de PC-98");
    }
    let sector = disk.pieces.first().map_or(0, |p| p.len);
    let boot = logical.first().copied();
    if boot == Some(0x60) && (sector == 1024 || disk.format != "D88") {
        return (Platform::X68000, "setor de boot de 68000 (BRA no byte 0)");
    }
    if matches!(boot, Some(0xEB | 0xE9)) && disk.media != Some(0x00) {
        return (Platform::Pc98, "setor de boot do MS-DOS (x86)");
    }
    match disk.format {
        "DIM" | "XDF" => (Platform::X68000, "formato de imagem do X68000"),
        _ if disk.media == Some(0x00) => (Platform::Pc88, "disquete 2D, o do PC-8801"),
        _ => (Platform::Pc98, "disquete 2DD/2HD sem marca do X68000"),
    }
}

fn extension(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Disco de onde a entry veio e a posicao logica dela nele.
fn place(entry: &TextEntry) -> Option<(usize, usize)> {
    let get = |key: &str| entry.metadata.get(key)?.as_u64().map(|v| v as usize);
    Some((get("disk")?, get("logical")?))
}

impl Computador {
    fn check(&self, data: &[u8]) -> Result<Vec<Disk>> {
        disks(data).ok_or_else(|| {
            CoreError::Project(format!(
                "{}: a imagem nao fecha como D88, FDI, HDI, DIM nem XDF",
                self.0.name
            ))
        })
    }
}

impl GameAdapter for Computador {
    fn id(&self) -> &'static str {
        self.0.id
    }

    fn display_name(&self) -> &'static str {
        self.0.name
    }

    fn platform(&self) -> Platform {
        self.0.platform
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            detect: true,
            extract: true,
            reinsert: true,
            patch: true,
            compression: false,
            pointer_relocation: false,
            font_table: false,
            experimental: true,
            support_level: SupportLevel::Experimental,
        }
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        let no = ProbeResult::no_match(self.id(), self.platform());
        let whole = input.whole();
        let data: &[u8] = whole.as_deref().unwrap_or(&input.head);
        let Some(disks) = disks(data) else {
            return no;
        };
        let disk = &disks[0];
        let logical = disk.logical(data);
        let fat = Fat::find(&logical);
        // Imagem crua so pelo tamanho nao basta: precisa de FAT ou de boot de
        // 68000.
        if disk.format == "XDF" && fat.is_none() && logical.first() != Some(&0x60) {
            return no;
        }
        let (platform, why) = machine(disk, &logical, &extension(&input.path));
        if platform != self.platform() {
            return no;
        }
        let mut evidence = vec![match disks.len() {
            1 => format!("imagem {} de {} KiB", disk.format, disk.len / 1024),
            n => format!("imagem {} com {n} discos", disk.format),
        }];
        evidence.push(match &fat {
            Some(fat) => format!(
                "sistema de arquivos {} com {} arquivos",
                if fat.fat16 { "FAT16" } else { "FAT12" },
                fat.files(&logical).len()
            ),
            None => "sem FAT: o disco e lido como um bloco".to_string(),
        });
        evidence.push(why.to_string());
        ProbeResult {
            confidence: if disk.format == "XDF" { 0.7 } else { 0.9 },
            evidence,
            ..no
        }
    }

    fn list_resources(&self, data: &[u8]) -> Result<Vec<ResourceDescriptor>> {
        let disks = self.check(data)?;
        let many = disks.len() > 1;
        let mut out = Vec::new();
        for (k, disk) in disks.iter().enumerate() {
            let logical = disk.logical(data);
            let Some(fat) = Fat::find(&logical) else {
                continue;
            };
            for file in fat.files(&logical) {
                out.push(ResourceDescriptor {
                    path: disk_path(many, k, &file.path),
                    offset: file.runs.first().and_then(|r| disk.at(r.0)).unwrap_or(0) as u64,
                    size: file.size as u64,
                });
            }
        }
        Ok(out)
    }

    /// Por arquivo quando o disco tem FAT; senao o disco inteiro. A varredura
    /// e na ordem logica (a frase pode atravessar o fim de um setor); `offset`
    /// e onde o primeiro byte mora no arquivo.
    // ponytail: arquivo fragmentado e varrido trecho a trecho; frase que
    // atravessa dois clusters fora de ordem nao sai. Disco gravado de uma vez
    // nao tem isso.
    fn extract(&self, data: &[u8]) -> Result<Extraction> {
        let disks = self.check(data)?;
        let many = disks.len() > 1;
        let mut out = Extraction::default();
        for (k, disk) in disks.iter().enumerate() {
            let logical = disk.logical(data);
            let fat = Fat::find(&logical);
            let regions: Vec<(Option<String>, (usize, usize))> = match &fat {
                Some(fat) => fat
                    .files(&logical)
                    .into_iter()
                    .flat_map(|f| {
                        let path = disk_path(many, k, &f.path);
                        f.runs.into_iter().map(move |r| (Some(path.clone()), r))
                    })
                    .collect(),
                None => vec![(None, (0, logical.len()))],
            };
            for (path, (start, end)) in regions {
                let outcome = scan_bytes(
                    &logical,
                    &ScanConfig {
                        encoding: ScanEncoding::Ascii,
                        japanese: true,
                        region_start: Some(start as u64),
                        region_end: Some(end as u64),
                        max_entries: MAX_ENTRIES - out.entries.len(),
                        only_text: true,
                        // Arquivo a arquivo, como nos discos de console; o
                        // disco sem FAT fica como cartucho (`stray_ids`).
                        skip_noise: fat.is_some(),
                        ..ScanConfig::default()
                    },
                )?;
                out.discarded += outcome.discarded;
                out.truncated |= outcome.truncated;
                for mut e in outcome.entries {
                    let Some(at) = e.offset.and_then(|l| disk.at(l as usize)) else {
                        continue;
                    };
                    let logical_at = e.offset.unwrap_or_default();
                    e.id = format!("scan-{at:08x}");
                    e.offset = Some(at as u64);
                    e.resource_path = path.clone();
                    e.max_bytes = Some(e.original_bytes.len());
                    e.context = Some("in-place: traducao limitada ao espaco original".to_string());
                    e.metadata["disk"] = serde_json::json!(k);
                    e.metadata["logical"] = serde_json::json!(logical_at);
                    out.entries.push(e);
                }
                if out.entries.len() >= MAX_ENTRIES {
                    out.truncated = true;
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }

    /// Cada disco na ordem logica, com o plano de sempre (`plan_in_place`),
    /// e de volta pro arquivo pedaco por pedaco. Entry sem posicao logica (a
    /// varredura com tabela do projeto, que le o arquivo cru) vai direto no
    /// arquivo.
    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        let disks = self.check(data)?;
        let mut out = data.to_vec();
        let mut report = ApplyReport::default();
        let mut add = |r: ApplyReport| {
            report.applied += r.applied;
            report.kept_original += r.kept_original;
            report.ignored_generic += r.ignored_generic;
        };
        for (k, disk) in disks.iter().enumerate() {
            let mine: Vec<TextEntry> = entries
                .iter()
                .filter_map(|e| {
                    let (disk, at) = place(e)?;
                    (disk == k).then(|| TextEntry {
                        offset: Some(at as u64),
                        ..e.clone()
                    })
                })
                .collect();
            if mine.is_empty() {
                continue;
            }
            let plan = plan_in_place(&disk.logical(data), &mine, false)?;
            for (at, bytes) in &plan.writes {
                disk.write(&mut out, *at, bytes);
            }
            add(plan.report);
        }
        let raw: Vec<TextEntry> = entries
            .iter()
            .filter(|e| place(e).is_none_or(|(k, _)| k >= disks.len()))
            .cloned()
            .collect();
        let plan = plan_in_place(data, &raw, false)?;
        for (at, bytes) in &plan.writes {
            out[*at..at + bytes.len()].copy_from_slice(bytes);
        }
        add(plan.report);
        Ok(AppliedImage { bytes: out, report })
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        let mut checks = Vec::new();
        let mut problems = Vec::new();
        match self.check(data) {
            Ok(disks) => {
                checks.push(format!(
                    "imagem {} com {} disco(s) lida de novo",
                    disks[0].format,
                    disks.len()
                ));
                for disk in &disks {
                    if let Some(fat) = Fat::find(&disk.logical(data)) {
                        checks.push(format!(
                            "FAT lida de novo: {} arquivos",
                            fat.files(&disk.logical(data)).len()
                        ));
                    }
                }
            }
            Err(e) => problems.push(e.to_string()),
        }
        Ok(VerificationReport {
            ok: problems.is_empty(),
            checks,
            problems,
        })
    }
}

fn disk_path(many: bool, k: usize, path: &str) -> String {
    if many {
        format!("disco {}/{path}", k + 1)
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    fn adapter(platform: Platform) -> Computador {
        Computador(MACHINES.iter().find(|m| m.platform == platform).unwrap())
    }

    /// A imagem num arquivo de verdade, apagado no fim: o probe le alem dos
    /// 128 KiB do comeco (`GameInput::whole`).
    struct OnDisk(GameInput);

    impl Drop for OnDisk {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0.path);
        }
    }

    fn on_disk(name: &str, image: &[u8]) -> OnDisk {
        let dir = std::env::temp_dir().join(format!("rts-computador-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = dir.join(format!("{unique}-{name}"));
        std::fs::write(&path, image).unwrap();
        OnDisk(GameInput::load(&path).unwrap())
    }

    fn probe(platform: Platform, name: &str, image: &[u8]) -> f32 {
        adapter(platform).probe(&on_disk(name, image).0).confidence
    }

    fn images() -> Vec<(&'static str, Vec<u8>, Platform)> {
        vec![
            ("jogo.d88", synth::make_pc98_d88(), Platform::Pc98),
            ("jogo.fdi", synth::make_pc98_fdi(), Platform::Pc98),
            ("jogo.hdi", synth::make_pc98_hdi(), Platform::Pc98),
            ("jogo.d88", synth::make_pc88_d88(), Platform::Pc88),
            ("jogo.dim", synth::make_x68k_dim(), Platform::X68000),
            ("jogo.xdf", synth::make_x68k_xdf(), Platform::X68000),
        ]
    }

    #[test]
    fn each_machine_knows_its_own_disk_and_no_other_adapter_takes_it() {
        for (name, image, platform) in images() {
            let file = on_disk(name, &image);
            let input = &file.0;
            let mut results: Vec<(String, f32)> = crate::adapters::all()
                .iter()
                .map(|a| (a.id().to_string(), a.probe(input).confidence))
                .filter(|r| r.1 > 0.0)
                .collect();
            results.sort_by(|a, b| b.1.total_cmp(&a.1));
            assert_eq!(
                results.first().map(|r| r.0.as_str()),
                Some(adapter(platform).id()),
                "{name}: {results:?}"
            );
            assert_eq!(results.len(), 1, "{name}: {results:?}");
            // Sem a extensao, a maquina sai do conteudo.
            assert!(probe(platform, "sem-extensao", &image) >= 0.7, "{name}");
        }
    }

    #[test]
    fn other_formats_noise_and_truncation_are_refused_without_panic() {
        let others = [
            synth::make_saturn_bin(),
            synth::make_ps1_bin(),
            synth::make_gba_rom("NOPE"),
            synth::make_nds_rom(),
            synth::make_random(RAW_2HD, 5),
            synth::make_random(1 << 16, 7),
            vec![0u8; RAW_2HD],
            Vec::new(),
        ];
        for m in &MACHINES {
            for image in &others {
                assert_eq!(probe(m.platform, "a.d88", image), 0.0, "{}", m.id);
            }
        }
        for (name, image, platform) in images() {
            for len in (0..image.len()).step_by(image.len() / 97 + 1) {
                let cut = &image[..len];
                probe(platform, name, cut);
                let a = adapter(platform);
                if let Ok(x) = a.extract(cut) {
                    a.apply_text(cut, &x.entries).unwrap();
                }
            }
            // Bytes trocados no meio nao derrubam nada.
            let mut noisy = image.clone();
            for (i, b) in synth::make_random(noisy.len(), 3).iter().enumerate() {
                if i % 7 == 0 {
                    noisy[i] = *b;
                }
            }
            probe(platform, name, &noisy);
            let _ = adapter(platform).extract(&noisy);
            let _ = adapter(platform).list_resources(&noisy);
        }
    }

    #[test]
    fn japanese_comes_out_per_file_and_the_translation_goes_back_in_place() {
        for (name, image, platform) in images() {
            let a = adapter(platform);
            let mut entries = a.extract(&image).unwrap().entries;
            let find = |entries: &[TextEntry], text: &str| {
                entries
                    .iter()
                    .position(|e| e.source_text.starts_with(text))
                    .unwrap_or_else(|| {
                        panic!(
                            "{name}: {text:?} nao saiu de {:?}",
                            entries.iter().map(|e| &e.source_text).collect::<Vec<_>>()
                        )
                    })
            };
            let japanese = find(&entries, synth::DISK_JAPANESE);
            let english = find(&entries, synth::DISK_ASCII);
            assert_eq!(
                entries[japanese].encoding,
                crate::types::TextEncoding::ShiftJis
            );
            if platform != Platform::Pc88 {
                assert_eq!(
                    entries[english].resource_path.as_deref(),
                    Some("GAME.DAT"),
                    "{name}"
                );
                find(&entries, synth::DISK_SUBDIR_LINE);
                let files = a.list_resources(&image).unwrap();
                assert!(files.iter().any(|f| f.path == "MSG/TEXT.DAT"), "{name}");
            }
            entries[english].translated_text = Some("APERTE RETURN".into());
            entries[japanese].translated_text = Some("Vamos a aventura!".into());
            let out = a.apply_text(&image, &entries).unwrap();
            assert_eq!(out.report.applied, 2, "{name}");
            assert_eq!(out.bytes.len(), image.len());
            assert!(a.verify(&out.bytes).unwrap().ok, "{name}");
            let again = a.extract(&out.bytes).unwrap().entries;
            find(&again, "APERTE RETURN");
            find(&again, "Vamos a aventura!");
            assert!(
                !again
                    .iter()
                    .any(|e| e.source_text.starts_with(synth::DISK_JAPANESE)),
                "{name}"
            );
            // So os bytes das duas frases mudaram (e nenhum cabecalho de
            // setor do D88).
            let changed = image.iter().zip(&out.bytes).filter(|(a, b)| a != b).count();
            let room =
                entries[english].original_bytes.len() + entries[japanese].original_bytes.len();
            assert!(changed <= room, "{name}: {changed} bytes mudaram");
        }
    }

    #[test]
    fn a_line_across_two_d88_sectors_is_read_and_written_in_both_pieces() {
        for image in [synth::make_pc98_d88(), synth::make_pc88_d88()] {
            let disks = disks(&image).unwrap();
            let a = adapter(Platform::Pc98);
            let entries = a.extract(&image).unwrap().entries;
            let e = entries
                .iter()
                .find(|e| e.source_text.starts_with(synth::DISK_JAPANESE))
                .unwrap();
            let (k, logical) = place(e).unwrap();
            let end = logical + e.original_bytes.len() - 1;
            // A frase atravessa o fim de um setor, e o setor seguinte nao esta
            // logo depois no arquivo.
            assert_ne!(
                disks[k].at(end),
                Some(disks[k].at(logical).unwrap() + e.original_bytes.len() - 1)
            );
            assert_eq!(e.offset, disks[k].at(logical).map(|a| a as u64));
        }
    }
}

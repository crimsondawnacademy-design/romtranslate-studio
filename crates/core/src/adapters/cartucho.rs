//! Cartuchos sem sistema de arquivos: o jogo e um bloco de bytes, como no NES
//! e no SNES. Uma ficha por console (`Console`): como reconhecer pelo
//! cabecalho e como refazer o checksum que o cartucho guarda. O resto e o
//! motor de sempre — varredura ASCII, tabela `.tbl`, perfil do jogo
//! (`perfil.toml`), reinsercao no espaco da original.
//!
//! Consoles da lista de pedidos de traducao do Data Crystal (20/10/2026, so
//! contagens): Game Boy/Color 104 jogos, Mega Drive 67 (+24 do Pico),
//! WonderSwan/Color 56, PC Engine 29, N64 26, MSX 21, Game Gear/Master System
//! 15, Famicom Disk System 8, Neo Geo Pocket 3.
//!
//! Checksum: so e refeito se o do original conferia. Cartucho com checksum
//! errado de fabrica (comum no Game Gear japones) fica como veio.
//!
//! Detecao sem bytes de material da Nintendo ou da Sega: logo nenhum, so
//! campos do cabecalho (checksum, tamanho, nome do sistema).

use crate::adapter::{AppliedImage, Extraction, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::scan::{scan_bytes, ScanConfig, ScanEncoding};
use crate::types::{AdapterCapabilities, Platform, ProbeResult, SupportLevel, TextEntry};

/// O que um console tem de proprio.
pub struct Console {
    id: &'static str,
    name: &'static str,
    platform: Platform,
    /// (confianca, evidencias) pelo comeco e pelo fim do arquivo; 0 = nao e.
    probe: fn(&Bytes) -> (f32, Vec<String>),
    /// Refaz em `out` o checksum que o original tinha certo. `false` = o
    /// cartucho nao tem checksum, ou o do original nao conferia.
    fix: fn(original: &[u8], out: &mut [u8]) -> bool,
}

/// O que o probe enxerga: o comeco do arquivo, o fim, o tamanho e o nome.
pub struct Bytes<'a> {
    head: &'a [u8],
    tail: &'a [u8],
    size: usize,
    extension: String,
}

pub struct Cartucho(pub &'static Console);

pub static CONSOLES: [Console; 9] = [
    Console {
        id: "gb.cart",
        name: "Game Boy / Game Boy Color",
        platform: Platform::GameBoy,
        probe: gb_probe,
        fix: gb_fix,
    },
    Console {
        id: "md.cart",
        name: "Mega Drive / Genesis",
        platform: Platform::MegaDrive,
        probe: md_probe,
        fix: md_fix,
    },
    Console {
        id: "sms.cart",
        name: "Master System / Game Gear",
        platform: Platform::MasterSystem,
        probe: sega8_probe,
        fix: sega8_fix,
    },
    Console {
        id: "n64.z64",
        name: "Nintendo 64 (.z64)",
        platform: Platform::N64,
        probe: n64_probe,
        fix: n64_fix,
    },
    Console {
        id: "ws.cart",
        name: "WonderSwan / WonderSwan Color",
        platform: Platform::WonderSwan,
        probe: ws_probe,
        fix: ws_fix,
    },
    Console {
        id: "ngp.cart",
        name: "Neo Geo Pocket / Color",
        platform: Platform::NeoGeoPocket,
        probe: ngp_probe,
        fix: no_checksum,
    },
    Console {
        id: "fds.disk",
        name: "Famicom Disk System",
        platform: Platform::Fds,
        probe: fds_probe,
        fix: no_checksum,
    },
    Console {
        id: "msx.cart",
        name: "MSX (cartucho)",
        platform: Platform::Msx,
        probe: msx_probe,
        fix: no_checksum,
    },
    Console {
        id: "pce.hucard",
        name: "PC Engine (HuCard)",
        platform: Platform::PcEngine,
        probe: pce_probe,
        fix: no_checksum,
    },
];

fn no_checksum(_: &[u8], _: &mut [u8]) -> bool {
    false
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn le16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn printable(d: &[u8]) -> bool {
    d.iter().all(|&b| b == 0 || (0x20..0x7F).contains(&b))
}

// --- Game Boy / Color: cabecalho em 0x100..0x150 -------------------------

fn gb_header_sum(d: &[u8]) -> u8 {
    d[0x134..=0x14C]
        .iter()
        .fold(0u8, |x, b| x.wrapping_sub(*b).wrapping_sub(1))
}

fn gb_global_sum(d: &[u8]) -> u16 {
    let all = d.iter().fold(0u16, |s, b| s.wrapping_add(*b as u16));
    all.wrapping_sub(d[0x14E] as u16)
        .wrapping_sub(d[0x14F] as u16)
}

fn gb_probe(b: &Bytes) -> (f32, Vec<String>) {
    let d = b.head;
    if d.len() < 0x150 || b.size < 0x8000 || d[0x14D] != gb_header_sum(d) {
        return (0.0, Vec::new());
    }
    let mut confidence = 0.6;
    let mut evidence = vec!["checksum do cabecalho (0x14D) confere".to_string()];
    if d[0x148] <= 8 && 0x8000usize << d[0x148] == b.size {
        confidence += 0.25;
        evidence.push(format!("tamanho declarado bate: {} KiB", b.size / 1024));
    }
    if printable(&d[0x134..0x143]) {
        confidence += 0.1;
    }
    if d[0x143] & 0x80 != 0 {
        evidence.push("cartucho com modo Game Boy Color".to_string());
    }
    (confidence, evidence)
}

fn gb_fix(original: &[u8], out: &mut [u8]) -> bool {
    if original.len() < 0x150 {
        return false;
    }
    let header = original[0x14D] == gb_header_sum(original);
    let global = be16(original, 0x14E) == Some(gb_global_sum(original));
    if header {
        out[0x14D] = gb_header_sum(out);
    }
    if global {
        let sum = gb_global_sum(out);
        out[0x14E..0x150].copy_from_slice(&sum.to_be_bytes());
    }
    header || global
}

// --- Mega Drive / Genesis (Pico, 32X): "SEGA" em 0x100 --------------------

fn md_sum(d: &[u8]) -> u16 {
    d.get(0x200..)
        .unwrap_or_default()
        .chunks(2)
        .fold(0u16, |s, w| {
            s.wrapping_add(u16::from_be_bytes([w[0], *w.get(1).unwrap_or(&0)]))
        })
}

fn md_probe(b: &Bytes) -> (f32, Vec<String>) {
    let d = b.head;
    let Some(system) = d.get(0x100..0x110) else {
        return (0.0, Vec::new());
    };
    if !system.starts_with(b"SEGA") && !system.starts_with(b" SEGA") {
        return (0.0, Vec::new());
    }
    let mut confidence = 0.75;
    let name = String::from_utf8_lossy(system).trim().to_string();
    let mut evidence = vec![format!("nome do sistema em 0x100: {name:?}")];
    if b.size == d.len() && be16(d, 0x18E) == Some(md_sum(d)) {
        confidence += 0.2;
        evidence.push("checksum do cabecalho (0x18E) confere".to_string());
    }
    if be32(d, 0x1A4).map(|end| end as usize + 1) == Some(b.size) {
        confidence += 0.04;
        evidence.push("fim da ROM declarado bate com o tamanho".to_string());
    }
    (confidence, evidence)
}

fn md_fix(original: &[u8], out: &mut [u8]) -> bool {
    if original.len() <= 0x200 || be16(original, 0x18E) != Some(md_sum(original)) {
        return false;
    }
    let sum = md_sum(out);
    out[0x18E..0x190].copy_from_slice(&sum.to_be_bytes());
    true
}

// --- Master System / Game Gear: "TMR SEGA" -------------------------------

/// Codigo de tamanho do cabecalho -> fim da faixa somada.
const SEGA8_SIZES: [(u8, usize); 9] = [
    (0xA, 0x2000),
    (0xB, 0x4000),
    (0xC, 0x8000),
    (0xD, 0xC000),
    (0xE, 0x10000),
    (0xF, 0x20000),
    (0x0, 0x40000),
    (0x1, 0x80000),
    (0x2, 0x100000),
];

fn sega8_header(d: &[u8]) -> Option<usize> {
    [0x7FF0, 0x3FF0, 0x1FF0]
        .into_iter()
        .find(|&at| d.get(at..at + 8) == Some(b"TMR SEGA"))
}

/// A soma do cartucho: 0..fim (sem os 16 bytes do cabecalho em 0x7FF0) e,
/// acima de 32 KiB, 0x8000..fim.
fn sega8_sum(d: &[u8], header: usize) -> Option<u16> {
    let code = d[header + 15] & 0x0F;
    let end = SEGA8_SIZES.iter().find(|(c, _)| *c == code)?.1;
    let first = end.min(0x8000) - 0x10;
    let low = d.get(..first)?;
    let high = if end > 0x8000 {
        d.get(0x8000..end)?
    } else {
        &[]
    };
    Some(
        low.iter()
            .chain(high)
            .fold(0u16, |s, b| s.wrapping_add(*b as u16)),
    )
}

fn sega8_probe(b: &Bytes) -> (f32, Vec<String>) {
    let d = b.head;
    match sega8_header(d) {
        Some(at) => {
            let region = d[at + 15] >> 4;
            let system = match region {
                3 | 4 => "Master System",
                5..=7 => "Game Gear",
                _ => "Master System/Game Gear",
            };
            let mut evidence = vec![format!("cabecalho TMR SEGA em 0x{at:X} ({system})")];
            let mut confidence = 0.8;
            if b.size == d.len() && le16(d, at + 10) == sega8_sum(d, at) {
                confidence += 0.15;
                evidence.push("checksum confere".to_string());
            }
            (confidence, evidence)
        }
        // Jogo japones de Master System nao precisa do cabecalho.
        None if matches!(b.extension.as_str(), "sms" | "gg") && b.size.is_multiple_of(0x2000) => (
            0.5,
            vec!["sem cabecalho TMR SEGA: reconhecido pela extensao".to_string()],
        ),
        None => (0.0, Vec::new()),
    }
}

fn sega8_fix(original: &[u8], out: &mut [u8]) -> bool {
    let Some(at) = sega8_header(original) else {
        return false;
    };
    if le16(original, at + 10) != sega8_sum(original, at) {
        return false;
    }
    match sega8_sum(out, at) {
        Some(sum) => {
            out[at + 10..at + 12].copy_from_slice(&sum.to_le_bytes());
            true
        }
        None => false,
    }
}

// --- Nintendo 64 (.z64, big-endian) --------------------------------------

const N64_MAGIC: u32 = 0x8037_1240;
const N64_START: usize = 0x1000;
const N64_LENGTH: usize = 0x10_0000;

/// As variantes do CRC do cabecalho (uma por chip de protecao). A variante
/// do jogo e a que reproduz o CRC guardado no original: nenhuma constante do
/// codigo de boot e necessaria.
#[derive(Clone, Copy, Debug, PartialEq)]
enum N64Crc {
    Plain(u32),
    Sum,
    Product,
    Table,
}

const N64_VARIANTS: [N64Crc; 4] = [
    N64Crc::Plain(0xF8CA_4DDC),
    N64Crc::Sum,
    N64Crc::Product,
    N64Crc::Table,
];

fn n64_crc(d: &[u8], variant: N64Crc) -> Option<(u32, u32)> {
    let seed = match variant {
        N64Crc::Plain(seed) => seed,
        N64Crc::Sum => 0xA388_6759,
        N64Crc::Product => 0x1FEA_617A,
        N64Crc::Table => 0xDF26_F436,
    };
    let body = d.get(N64_START..N64_START + N64_LENGTH)?;
    let (mut t1, mut t2, mut t3, mut t4, mut t5, mut t6) = (seed, seed, seed, seed, seed, seed);
    for (k, word) in body.as_chunks::<4>().0.iter().enumerate() {
        let i = N64_START + k * 4;
        let w = u32::from_be_bytes(*word);
        if t6.wrapping_add(w) < t6 {
            t4 = t4.wrapping_add(1);
        }
        t6 = t6.wrapping_add(w);
        t3 ^= w;
        let r = w.rotate_left(w & 0x1F);
        t5 = t5.wrapping_add(r);
        t2 ^= if t2 > w { r } else { t6 ^ w };
        t1 = t1.wrapping_add(match variant {
            N64Crc::Table => be32(d, 0x40 + 0x0710 + (i & 0xFF))? ^ w,
            _ => t5 ^ w,
        });
    }
    Some(match variant {
        N64Crc::Sum => ((t6 ^ t4).wrapping_add(t3), (t5 ^ t2).wrapping_add(t1)),
        N64Crc::Product => (
            t6.wrapping_mul(t4).wrapping_add(t3),
            t5.wrapping_mul(t2).wrapping_add(t1),
        ),
        _ => (t6 ^ t4 ^ t3, t5 ^ t2 ^ t1),
    })
}

fn n64_variant(d: &[u8]) -> Option<N64Crc> {
    let stored = (be32(d, 0x10)?, be32(d, 0x14)?);
    N64_VARIANTS
        .into_iter()
        .find(|&v| n64_crc(d, v) == Some(stored))
}

fn n64_probe(b: &Bytes) -> (f32, Vec<String>) {
    match be32(b.head, 0) {
        Some(N64_MAGIC) => {
            let title = String::from_utf8_lossy(b.head.get(0x20..0x34).unwrap_or_default());
            (
                0.95,
                vec![format!(
                    "cabecalho de N64 big-endian (.z64): {:?}",
                    title.trim()
                )],
            )
        }
        Some(0x3780_4012) | Some(0x4012_3780) => (
            0.3,
            vec![
                "N64 com os bytes trocados (.v64/.n64): converta para .z64 antes de traduzir"
                    .to_string(),
            ],
        ),
        _ => (0.0, Vec::new()),
    }
}

fn n64_fix(original: &[u8], out: &mut [u8]) -> bool {
    let Some(variant) = n64_variant(original) else {
        return false;
    };
    match n64_crc(out, variant) {
        Some((a, b)) => {
            out[0x10..0x14].copy_from_slice(&a.to_be_bytes());
            out[0x14..0x18].copy_from_slice(&b.to_be_bytes());
            true
        }
        None => false,
    }
}

// --- WonderSwan / Color: rodape nos ultimos 16 bytes ----------------------

fn ws_sum(d: &[u8]) -> u16 {
    d[..d.len() - 2]
        .iter()
        .fold(0u16, |s, b| s.wrapping_add(*b as u16))
}

fn ws_probe(b: &Bytes) -> (f32, Vec<String>) {
    let t = b.tail;
    if t.len() < 16 || b.size < 0x2_0000 || !b.size.is_power_of_two() || t[t.len() - 16] != 0xEA {
        return (0.0, Vec::new());
    }
    let color = t[t.len() - 9] & 1 == 1;
    let mut evidence = vec![format!(
        "rodape de WonderSwan{} (salto 0xEA nos ultimos 16 bytes)",
        if color { " Color" } else { "" }
    )];
    let mut confidence = 0.5;
    if b.head.len() == b.size && le16(t, t.len() - 2) == Some(ws_sum(b.head)) {
        confidence += 0.45;
        evidence.push("checksum do rodape confere".to_string());
    }
    (confidence, evidence)
}

fn ws_fix(original: &[u8], out: &mut [u8]) -> bool {
    let n = original.len();
    if n < 16 || le16(original, n - 2) != Some(ws_sum(original)) {
        return false;
    }
    let sum = ws_sum(out);
    out[n - 2..].copy_from_slice(&sum.to_le_bytes());
    true
}

// --- Sem checksum ----------------------------------------------------------

fn ngp_probe(b: &Bytes) -> (f32, Vec<String>) {
    let tag = b.head.get(..28).unwrap_or_default();
    if tag == b"COPYRIGHT BY SNK CORPORATION" || tag == b" LICENSED BY SNK CORPORATION" {
        (0.95, vec!["cabecalho de licenca da SNK".to_string()])
    } else {
        (0.0, Vec::new())
    }
}

fn fds_probe(b: &Bytes) -> (f32, Vec<String>) {
    let d = b.head;
    if d.starts_with(b"FDS\x1a") {
        (0.95, vec!["cabecalho fwNES (FDS\\x1A)".to_string()])
    } else if d.first() == Some(&1) && d.get(1..15) == Some(b"*NINTENDO-HVC*") {
        (
            0.9,
            vec!["bloco de disco do Famicom Disk System".to_string()],
        )
    } else {
        (0.0, Vec::new())
    }
}

fn msx_probe(b: &Bytes) -> (f32, Vec<String>) {
    let d = b.head;
    let init = le16(d, 2).unwrap_or(0);
    if !d.starts_with(b"AB") || !(0x4000..0xC000).contains(&init) {
        return (0.0, Vec::new());
    }
    let mut evidence = vec![format!(
        "cabecalho de cartucho MSX (\"AB\", inicio 0x{init:04X})"
    )];
    let mut confidence = 0.55;
    if matches!(b.extension.as_str(), "rom" | "mx1" | "mx2") {
        confidence += 0.3;
        evidence.push("extensao de ROM de MSX".to_string());
    }
    (confidence, evidence)
}

/// HuCard nao tem cabecalho: so a extensao e o tamanho (blocos de 8 KiB, com
/// ou sem os 512 bytes do copiador).
fn pce_probe(b: &Bytes) -> (f32, Vec<String>) {
    if b.extension == "pce" && matches!(b.size % 0x2000, 0 | 512) && b.size >= 0x2000 {
        (
            0.6,
            vec!["sem cabecalho: reconhecido pela extensao .pce e pelo tamanho".to_string()],
        )
    } else {
        (0.0, Vec::new())
    }
}

// --- O adapter --------------------------------------------------------------

fn extension(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

impl Cartucho {
    fn bytes<'a>(&self, data: &'a [u8]) -> Bytes<'a> {
        Bytes {
            head: data,
            tail: data,
            size: data.len(),
            extension: String::new(),
        }
    }

    fn check(&self, data: &[u8]) -> Result<()> {
        let (confidence, _) = (self.0.probe)(&self.bytes(data));
        // Sem nome de arquivo aqui: o que so a extensao reconhece passa.
        if confidence > 0.0
            || matches!(self.0.platform, Platform::PcEngine | Platform::MasterSystem)
        {
            Ok(())
        } else {
            Err(CoreError::Project(format!(
                "{}: o arquivo nao tem o cabecalho deste console",
                self.0.name
            )))
        }
    }
}

impl GameAdapter for Cartucho {
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
        // Cartucho maior que a janela do probe: o arquivo inteiro, mapeado
        // (o checksum soma tudo, e o WonderSwan tem o cabecalho no fim).
        let whole = input.whole();
        let head: &[u8] = whole.as_deref().unwrap_or(&input.head);
        let (confidence, evidence) = (self.0.probe)(&Bytes {
            head,
            tail: head,
            size: input.size as usize,
            extension: extension(&input.path),
        });
        if confidence <= 0.0 {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        ProbeResult {
            confidence: confidence.min(0.99),
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }

    /// Varredura do cartucho inteiro, em ASCII e em Shift-JIS (o japones de
    /// boa parte dos jogos de N64, MSX e PC Engine), cada frase no espaco da
    /// original. Jogo com tabela propria (kana de 8 bits, o comum em Game Boy
    /// e Mega Drive): `scan --tbl` ou perfil do jogo.
    fn extract(&self, data: &[u8]) -> Result<Extraction> {
        self.check(data)?;
        let outcome = scan_bytes(
            data,
            &ScanConfig {
                encoding: ScanEncoding::Ascii,
                japanese: true,
                only_text: true,
                ..ScanConfig::default()
            },
        )?;
        let mut entries = outcome.entries;
        for e in entries.iter_mut() {
            e.max_bytes = Some(e.original_bytes.len());
            e.context = Some("in-place: traducao limitada ao espaco original".to_string());
        }
        Ok(Extraction {
            entries,
            discarded: outcome.discarded,
            truncated: outcome.truncated,
            ..Extraction::default()
        })
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        self.check(data)?;
        let fix = self.0.fix;
        super::inplace::apply_in_place(data, entries, |out| {
            fix(data, out);
        })
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        let mut checks = Vec::new();
        let mut problems = Vec::new();
        match self.check(data) {
            Ok(()) => checks.push(format!("cabecalho de {} preservado", self.0.name)),
            Err(e) => problems.push(e.to_string()),
        }
        // O checksum da copia confere consigo mesmo: refazer nao muda nada.
        let mut again = data.to_vec();
        if (self.0.fix)(data, &mut again) {
            if again == data {
                checks.push("checksum do cartucho confere".to_string());
            } else {
                problems.push("checksum do cartucho nao confere".to_string());
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

    const TEXT: &[u8] = b"WELCOME TO THE CASTLE OF DREAMS";

    fn console(id: &str) -> Cartucho {
        Cartucho(CONSOLES.iter().find(|c| c.id == id).unwrap())
    }

    fn probe(id: &str, name: &str, rom: &[u8]) -> f32 {
        console(id)
            .probe(&GameInput::from_bytes(name, rom))
            .confidence
    }

    /// Uma frase em 0x400, como a varredura acharia.
    fn with_text(mut rom: Vec<u8>) -> Vec<u8> {
        rom[0x400..0x400 + TEXT.len()].copy_from_slice(TEXT);
        rom
    }

    fn translated(id: &str, rom: &[u8]) -> Vec<u8> {
        let entry = TextEntry {
            id: "t".into(),
            resource_path: None,
            offset: rom
                .windows(TEXT.len())
                .position(|w| w == TEXT)
                .map(|at| at as u64),
            original_bytes: TEXT.to_vec(),
            source_text: String::from_utf8(TEXT.to_vec()).unwrap(),
            translated_text: Some("BEM-VINDO AO CASTELO DOS SONHOS".into()),
            context: None,
            max_bytes: Some(TEXT.len()),
            encoding: TextEncoding::Ascii,
            status: Default::default(),
            metadata: serde_json::json!({}),
        };
        console(id).apply_text(rom, &[entry]).unwrap().bytes
    }

    fn gb_rom() -> Vec<u8> {
        let mut rom = with_text(vec![0u8; 0x8000]);
        rom[0x134..0x13C].copy_from_slice(b"SYNTHGB ");
        rom[0x143] = 0x80;
        rom[0x14D] = gb_header_sum(&rom);
        let sum = gb_global_sum(&rom);
        rom[0x14E..0x150].copy_from_slice(&sum.to_be_bytes());
        rom
    }

    fn md_rom() -> Vec<u8> {
        let mut rom = with_text(vec![0u8; 0x1000]);
        rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        rom[0x1A4..0x1A8].copy_from_slice(&0xFFFu32.to_be_bytes());
        let sum = md_sum(&rom);
        rom[0x18E..0x190].copy_from_slice(&sum.to_be_bytes());
        rom
    }

    fn sms_rom() -> Vec<u8> {
        let mut rom = with_text(vec![0u8; 0x8000]);
        rom[0x7FF0..0x7FF8].copy_from_slice(b"TMR SEGA");
        rom[0x7FFF] = 0x6C; // Game Gear, 32 KiB
        let sum = sega8_sum(&rom, 0x7FF0).unwrap();
        rom[0x7FFA..0x7FFC].copy_from_slice(&sum.to_le_bytes());
        rom
    }

    fn n64_rom() -> Vec<u8> {
        let mut rom = vec![0u8; N64_START + N64_LENGTH];
        rom[..4].copy_from_slice(&N64_MAGIC.to_be_bytes());
        for (i, b) in rom[N64_START..].iter_mut().enumerate() {
            *b = (i * 7 % 251) as u8;
        }
        rom[0x2000..0x2000 + TEXT.len()].copy_from_slice(TEXT);
        let (a, b) = n64_crc(&rom, N64_VARIANTS[0]).unwrap();
        rom[0x10..0x14].copy_from_slice(&a.to_be_bytes());
        rom[0x14..0x18].copy_from_slice(&b.to_be_bytes());
        rom
    }

    fn ws_rom() -> Vec<u8> {
        let mut rom = with_text(vec![0u8; 0x2_0000]);
        let n = rom.len();
        rom[n - 16] = 0xEA;
        rom[n - 9] = 1;
        let sum = ws_sum(&rom);
        rom[n - 2..].copy_from_slice(&sum.to_le_bytes());
        rom
    }

    #[test]
    fn each_console_knows_its_own_cartridge_and_refuses_noise() {
        let mut fds = with_text(vec![0u8; 0x1000]);
        fds[..4].copy_from_slice(b"FDS\x1a");
        let mut ngp = with_text(vec![0u8; 0x1000]);
        ngp[..28].copy_from_slice(b"COPYRIGHT BY SNK CORPORATION");
        let mut msx = with_text(vec![0u8; 0x4000]);
        msx[..4].copy_from_slice(&[b'A', b'B', 0x10, 0x40]);
        let roms = [
            ("gb.cart", "a.gbc", gb_rom()),
            ("md.cart", "a.md", md_rom()),
            ("sms.cart", "a.gg", sms_rom()),
            ("n64.z64", "a.z64", n64_rom()),
            ("ws.cart", "a.wsc", ws_rom()),
            ("fds.disk", "a.fds", fds),
            ("ngp.cart", "a.ngc", ngp),
            ("msx.cart", "a.rom", msx),
            ("pce.hucard", "a.pce", with_text(vec![0u8; 0x4000])),
        ];
        for (id, name, rom) in &roms {
            assert!(
                probe(id, name, rom) >= 0.6,
                "{id}: {}",
                probe(id, name, rom)
            );
            // Nenhum outro console reclama o cartucho com a mesma confianca.
            for other in CONSOLES.iter().filter(|c| c.id != *id) {
                assert!(
                    probe(other.id, name, rom) < probe(id, name, rom),
                    "{} pegou {id}",
                    other.id
                );
            }
        }
        for c in &CONSOLES {
            assert_eq!(
                probe(c.id, "x.bin", &crate::synth::make_random(0x2_0000, 3)),
                0.0,
                "{}",
                c.id
            );
            assert_eq!(probe(c.id, "x.bin", &[]), 0.0, "{}", c.id);
            assert_eq!(probe(c.id, "x.bin", &[0x41, 0x42]), 0.0, "{}", c.id);
        }
    }

    #[test]
    fn the_checksum_is_redone_after_the_translation() {
        for (id, rom) in [
            ("gb.cart", gb_rom()),
            ("md.cart", md_rom()),
            ("sms.cart", sms_rom()),
            ("n64.z64", n64_rom()),
            ("ws.cart", ws_rom()),
        ] {
            let out = translated(id, &rom);
            assert_ne!(out, rom, "{id}: nada foi gravado");
            assert!(
                out.windows(8).any(|w| w == b"BEM-VIND"),
                "{id}: traducao ausente"
            );
            let report = console(id).verify(&out).unwrap();
            assert!(report.ok, "{id}: {:?}", report.problems);
            assert!(report.checks.iter().any(|c| c.contains("checksum")), "{id}");
        }
    }

    #[test]
    fn japanese_in_shift_jis_comes_out_of_the_cartridge() {
        let mut rom = gb_rom();
        let sjis = crate::sjis::encode("ようこそ、ゆめの しろへ。きみの なまえは？").unwrap();
        rom[0x600..0x600 + sjis.len()].copy_from_slice(&sjis);
        let entries = console("gb.cart").extract(&rom).unwrap().entries;
        assert!(
            entries.iter().any(|e| e.source_text.contains("ゆめの")),
            "{:?}",
            entries.iter().map(|e| &e.source_text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_wrong_factory_checksum_stays_wrong() {
        let mut rom = md_rom();
        rom[0x18E] ^= 0xFF;
        let out = translated("md.cart", &rom);
        assert_eq!(out[0x18E..0x190], rom[0x18E..0x190]);
        // N64 cujo CRC nenhuma variante reproduz: fica como veio.
        let mut rom = n64_rom();
        rom[0x10] ^= 0xFF;
        assert_eq!(n64_variant(&rom), None);
        let out = translated("n64.z64", &rom);
        assert_eq!(out[0x10..0x18], rom[0x10..0x18]);
    }
}

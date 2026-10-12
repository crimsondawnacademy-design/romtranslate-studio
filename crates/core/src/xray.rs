//! Raio-X do arquivo de origem: o que o app NAO consegue ler, dito em numeros.
//!
//! Extracao vazia ou curta nao prova que o jogo tem pouco texto — ele pode
//! estar comprimido, cifrado ou numa tabela de caracteres propria. O relatorio
//! separa o que foi OBSERVADO (bloco LZ77 que descomprime inteiro, string com
//! ponteiro em tabela) do que e INFERENCIA (bloco denso, "parece texto").

use std::path::Path;

use serde::Serialize;

use crate::db::ProjectDb;
use crate::error::Result;
use crate::fileio::read_view;
use crate::lz77;
use crate::project::load_project;
use crate::scan::ascii_texts;
use crate::types::TextEntry;

const BLOCK: usize = 4096;
/// Bits por byte a partir dos quais um bloco conta como denso.
// ponytail: limiar fixo. Texto fica perto de 4.5 bits, codigo ARM/Thumb entre
// 6 e 7, dado comprimido ou cifrado acima de 7.5. 7.2 separa bem em fixture;
// e o botao a girar se um jogo real cair na fronteira.
pub const DENSE_BITS: f64 = 7.2;
/// Abaixo disto um "bloco" valido e coincidencia barata demais.
const MIN_UNPACKED: usize = 32;
const MIN_MATCHES: usize = 2;
/// Teto de bytes descomprimidos numa varredura inteira, tentativas falhas
/// inclusas: arquivo com milhares de headers falsos nao trava o app.
const UNPACK_BUDGET: usize = 256 * 1024 * 1024;
const MAX_LISTED: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lz77Block {
    pub offset: u64,
    pub packed_len: u64,
    pub unpacked_len: u64,
    /// Strings com cara de texto dentro do bloco descomprimido.
    pub strings: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lz77Summary {
    pub blocks: usize,
    pub packed_bytes: u64,
    pub unpacked_bytes: u64,
    pub blocks_with_text: usize,
    /// Total de strings provaveis dentro dos blocos — texto que nenhum
    /// scanner do app enxerga hoje.
    pub strings: usize,
    /// A varredura parou no teto de trabalho: ha arquivo alem do que foi olhado.
    pub truncated: bool,
    /// Os primeiros blocos, em ordem de posicao.
    pub first: Vec<Lz77Block>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XrayReport {
    pub size: u64,
    /// Bytes em blocos de entropia alta: comprimido, cifrado ou audio/video.
    pub dense_bytes: u64,
    /// Bytes em blocos de um valor so (padding).
    pub blank_bytes: u64,
    pub lz77: Lz77Summary,
    /// Strings ja extraidas no projeto.
    pub entries: usize,
    /// Dessas, quantas tem ponteiro em tabela — a traducao pode passar do
    /// tamanho original. As outras precisam caber no espaco que ja ocupam.
    pub relocatable: usize,
}

pub fn xray(data: &[u8], entries: &[TextEntry]) -> XrayReport {
    let (dense_bytes, blank_bytes) = density(data);
    XrayReport {
        size: data.len() as u64,
        dense_bytes,
        blank_bytes,
        lz77: scan_lz77(data, UNPACK_BUDGET),
        entries: entries.len(),
        relocatable: entries.iter().filter(|e| !e.pointers().is_empty()).count(),
    }
}

/// Raio-X do arquivo de origem de um projeto, cruzado com as strings que ele
/// ja extraiu.
pub fn xray_project(project_dir: &Path) -> Result<XrayReport> {
    let project = load_project(project_dir)?;
    let data = read_view(&project.source_path)?;
    let entries = ProjectDb::open(project_dir)?.load_entries()?;
    Ok(xray(&data, &entries))
}

/// (bytes em blocos densos, bytes em blocos de um valor so).
fn density(data: &[u8]) -> (u64, u64) {
    let (mut dense, mut blank) = (0, 0);
    for block in data.chunks(BLOCK) {
        let mut counts = [0u32; 256];
        for &b in block {
            counts[b as usize] += 1;
        }
        let len = block.len() as f64;
        let bits: f64 = counts
            .iter()
            .filter(|&&c| c > 0)
            .map(|&c| {
                let p = c as f64 / len;
                -p * p.log2()
            })
            .sum();
        if bits >= DENSE_BITS {
            dense += block.len() as u64;
        } else if counts.iter().any(|&c| c as usize == block.len()) {
            blank += block.len() as u64;
        }
    }
    (dense, blank)
}

/// Texto de verdade, nao tile 4bpp que por acaso e imprimivel ("3333DDDD").
fn likely_text(text: &str) -> bool {
    let bytes = text.as_bytes();
    let wordy = bytes
        .iter()
        .filter(|b| b.is_ascii_alphabetic() || **b == b' ')
        .count();
    let mut seen = [false; 128];
    for &b in bytes {
        seen[(b & 0x7F) as usize] = true;
    }
    bytes.len() >= 5 && wordy * 10 >= bytes.len() * 7 && seen.iter().filter(|&&s| s).count() >= 4
}

// ponytail: so offsets multiplos de 4 — o BIOS exige a origem alinhada
// (GBATEK), e e o que derruba falso positivo. Bloco guardado desalinhado
// dentro de um arquivo proprio do jogo passa batido. So o tipo 0x10; o 0x11
// (LZ11, DS mais novo) entra quando houver fonte primaria e fixture.
fn scan_lz77(data: &[u8], mut budget: usize) -> Lz77Summary {
    let mut summary = Lz77Summary::default();
    let mut at = 0;
    while at + 4 <= data.len() {
        if budget == 0 {
            summary.truncated = true;
            break;
        }
        let block = (data[at] == 0x10)
            .then(|| lz77::unpack_within(&data[at..], &mut budget))
            .flatten()
            .filter(|b| b.data.len() >= MIN_UNPACKED && b.matches >= MIN_MATCHES);
        let Some(block) = block else {
            at += 4;
            continue;
        };
        let strings = ascii_texts(&block.data)
            .iter()
            .filter(|t| likely_text(t))
            .count();
        summary.blocks += 1;
        summary.packed_bytes += block.packed_len as u64;
        summary.unpacked_bytes += block.data.len() as u64;
        summary.blocks_with_text += usize::from(strings > 0);
        summary.strings += strings;
        if summary.first.len() < MAX_LISTED {
            summary.first.push(Lz77Block {
                offset: at as u64,
                packed_len: block.packed_len as u64,
                unpacked_len: block.data.len() as u64,
                strings,
            });
        }
        at += block.packed_len.next_multiple_of(4);
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_like_runs_do_not_count_as_text() {
        assert!(likely_text("Welcome to the village"));
        assert!(likely_text("GAME OVER"));
        assert!(!likely_text("3333DDDD3333"));
        assert!(!likely_text("\"\"\"\"\"\"\"\""));
        assert!(!likely_text("Hi!"));
    }

    #[test]
    fn budget_exhaustion_is_reported_not_hidden() {
        let script = b"The quick brown fox jumps over the lazy dog. ".repeat(40);
        let mut data = lz77::pack(&script).unwrap();
        data.extend(lz77::pack(&script).unwrap());
        let whole = scan_lz77(&data, usize::MAX);
        assert_eq!((whole.blocks, whole.truncated), (2, false));
        let cut = scan_lz77(&data, script.len() + 10);
        assert_eq!((cut.blocks, cut.truncated), (1, true));
    }
}

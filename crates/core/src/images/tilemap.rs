//! Imagem montada por mapa de quadros. Medido em 17/10/2026 no mesmo RPG
//! japones de GBA: o painel de cores da tela de ajustes guarda as palavras em
//! tiras de 2 linhas de quadros, lado a lado, e a tela as empilha a 13 pixels
//! uma da outra — na ordem guardada, a metade de cima e a de baixo de uma
//! palavra ficam em tiras diferentes (バナナ, ナッツ, グレープ saiam partidas).
//! Quem monta a tela e um mapa cru de entradas de 16 bits, 32 por linha (o
//! fundo de texto do GBA), que conta os quadros a partir do comeco do bloco que
//! o jogo carrega — no jogo medido, 108 quadros depois do comeco da imagem
//! achada.
//!
//! Achar o mapa: janela de 2 KiB com cara de mapa (fundo repetido, filas de
//! quadros seguidos e o quadro de baixo 32 adiante, como numa folha de 32 por
//! linha); pra cada imagem e cada comeco possivel do bloco nela, a entrada
//! repetida lado a lado tem que emendar nela mesma (fundo, borda), e as
//! emendas entre quadros diferentes tem que continuar o desenho bem mais que
//! nos comecos vizinhos. Escrever: celula redesenhada ganha um quadro so dela;
//! quadro repetido em celula que fica, ou que outro mapa mostra, nao muda.
// ponytail: so mapa cru de 32 entradas por linha (o fundo de texto do GBA)
// sobre quadros 4bpp. Mapa comprimido, tela montada por codigo (no jogo
// medido, o caixa eletronico) e sprite fora de grade ficam de fora. Quadro
// novo so sai de quadro que o proprio trecho redesenhado deixa de usar: um
// quadro "vazio" do bloco pode ser o que o jogo escreve na hora.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{lettered, pixels, tiles_of, Picture, TILE, TILE_BYTES};

/// Onde fica o mapa que monta uma imagem: entradas de 16 bits (quadro nos bits
/// 0-9, espelho nos 10-11, paleta nos 12-15), `COLS` por linha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileMap {
    /// Posicao da primeira linha no arquivo.
    pub offset: usize,
    pub rows: usize,
    /// O quadro 0 do mapa e o quadro `base` da imagem.
    pub base: usize,
}

/// Entradas por linha: o mapa de tela do GBA tem 32.
const COLS: usize = 32;
/// Entradas de uma janela da busca: 32 linhas, um mapa de tela (2 KiB).
const WINDOW: usize = COLS * COLS;
/// Passo entre janelas, em entradas (512 bytes): um mapa de 2 KiB fica pelo
/// menos 7/8 dentro de alguma.
const STEP: usize = 256;
/// Entrada igual a de cima pra janela valer: mapa de tela tem fundo.
const MIN_REPEATED: usize = 200;
/// Entrada seguida do quadro seguinte, e entrada com o quadro 32 adiante
/// embaixo: desenho guardado numa folha de 32 quadros por linha.
const MIN_RUNS: usize = 8;
/// Entradas espelhadas, no maximo: um quarto da janela.
const MAX_FLIPPED: usize = WINDOW / 4;
/// Emendas com desenho (nao so fundo dos dois lados) pra medir.
const MIN_SEAMS: u32 = 8;
/// Pixels iguais nas emendas, em decimos: o desenho continua de um quadro pro
/// outro.
const MIN_SEAM: u32 = 8;
/// Quanto as emendas no comeco escolhido passam da media nos comecos vizinhos
/// (`AROUND` pra cada lado), em fracao dos pixels: comeco de bloco qualquer emenda
/// fundo com fundo e listra com listra.
const MIN_MARGIN: f64 = 0.4;
const AROUND: usize = 64;
/// Quadros que parecem letra no mapa pra ele valer: o que interessa e texto.
const MIN_LETTERED: usize = 16;

const LEFT: usize = 0;
const RIGHT: usize = 1;
const TOP: usize = 2;
const BOTTOM: usize = 3;

/// As entradas do mapa, lidas de `data`.
pub fn entries(data: &[u8], map: &TileMap) -> Option<Vec<u16>> {
    let len = map.rows.checked_mul(COLS * 2)?;
    let bytes = data.get(map.offset..map.offset.checked_add(len)?)?;
    Some(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_le_bytes(pair))
            .collect(),
    )
}

fn tile_of(entry: u16, base: usize) -> usize {
    (entry & 0x03FF) as usize + base
}

/// O quadro espelhado: bit 0 na horizontal, bit 1 na vertical. Espelhar duas
/// vezes devolve o quadro.
fn flip(tile: &[u8], flips: u16) -> [u8; TILE_BYTES] {
    let mut out = [0u8; TILE_BYTES];
    if tile.len() < TILE_BYTES {
        return out;
    }
    for y in 0..TILE {
        let sy = if flips & 2 != 0 { TILE - 1 - y } else { y };
        for x in 0..TILE {
            let sx = if flips & 1 != 0 { TILE - 1 - x } else { x };
            let byte = tile[sy * TILE / 2 + sx / 2];
            let p = if sx % 2 == 0 { byte & 0x0F } else { byte >> 4 };
            out[y * TILE / 2 + x / 2] |= if x % 2 == 0 { p } else { p << 4 };
        }
    }
    out
}

/// As celulas do mapa, quadro a quadro, com o espelho de cada entrada: o que
/// `Sheet::new` desenha 32 por linha. Quadro fora da imagem sai vazio.
pub fn cells(tiles: &[u8], entries: &[u16], base: usize) -> Vec<u8> {
    let mut out = vec![0u8; entries.len() * TILE_BYTES];
    for (cell, &entry) in entries.iter().enumerate() {
        let t = tile_of(entry, base);
        if let Some(tile) = tiles.get(t * TILE_BYTES..(t + 1) * TILE_BYTES) {
            out[cell * TILE_BYTES..(cell + 1) * TILE_BYTES]
                .copy_from_slice(&flip(tile, (entry >> 10) & 3));
        }
    }
    out
}

/// Os quadros de uma imagem, medidos pra busca: as quatro bordas em cada
/// espelho (8 pixels, 4 bits cada), se o quadro emenda nele mesmo de lado e
/// de cima pra baixo, e se parece letra.
struct Measured {
    count: usize,
    edges: Vec<[[u32; 4]; 4]>,
    beside: Vec<u64>,
    above: Vec<u64>,
    lettered: Vec<bool>,
}

fn edge(px: &[u8; 64], flips: u16, side: usize) -> u32 {
    let at = |x: usize, y: usize| {
        let sx = if flips & 1 != 0 { TILE - 1 - x } else { x };
        let sy = if flips & 2 != 0 { TILE - 1 - y } else { y };
        u32::from(px[sy * TILE + sx])
    };
    (0..TILE).fold(0, |edge, k| {
        let p = match side {
            LEFT => at(0, k),
            RIGHT => at(TILE - 1, k),
            TOP => at(k, 0),
            _ => at(k, TILE - 1),
        };
        edge | p << (4 * k)
    })
}

fn measure(tiles: &[u8]) -> Measured {
    let count = tiles.len() / TILE_BYTES;
    let mut measured = Measured {
        count,
        edges: Vec::with_capacity(count),
        beside: vec![0; count.div_ceil(64)],
        above: vec![0; count.div_ceil(64)],
        lettered: Vec::with_capacity(count),
    };
    for (t, tile) in tiles.as_chunks::<TILE_BYTES>().0.iter().enumerate() {
        let px = pixels(tile);
        let mut edges = [[0u32; 4]; 4];
        for (flips, sides) in edges.iter_mut().enumerate() {
            for (side, e) in sides.iter_mut().enumerate() {
                *e = edge(&px, flips as u16, side);
            }
        }
        // O espelho nao muda se o quadro emenda nele mesmo.
        if edges[0][RIGHT] == edges[0][LEFT] {
            measured.beside[t / 64] |= 1 << (t % 64);
        }
        if edges[0][BOTTOM] == edges[0][TOP] {
            measured.above[t / 64] |= 1 << (t % 64);
        }
        measured.edges.push(edges);
        measured.lettered.push(lettered(&px));
    }
    measured
}

/// 64 bits de `set` a partir do bit `from` (zero alem do fim).
fn bits_from(set: &[u64], from: usize) -> u64 {
    let (word, shift) = (from / 64, from % 64);
    let low = set.get(word).copied().unwrap_or(0) >> shift;
    let high = match shift {
        0 => 0,
        _ => set.get(word + 1).copied().unwrap_or(0) << (64 - shift),
    };
    low | high
}

/// Borda de uma cor so.
fn flat(edge: u32) -> bool {
    edge == (edge & 0x0F) * 0x1111_1111
}

/// Pixels iguais entre duas bordas.
fn same_pixels(a: u32, b: u32) -> u32 {
    let diff = a ^ b;
    8 - ((diff | diff >> 1 | diff >> 2 | diff >> 3) & 0x1111_1111).count_ones()
}

/// Uma janela com cara de mapa: as entradas repetidas lado a lado (quadro, de
/// cima pra baixo) e as emendas entre entradas diferentes que nao sao vizinhas
/// na folha guardada (quadro e espelho de cada lado, de cima pra baixo).
struct Window {
    highest: usize,
    repeated: Vec<(usize, bool)>,
    seams: Vec<(u16, u16, bool)>,
}

fn window(entries: &[u16]) -> Option<Window> {
    if entries.len() < WINDOW {
        return None;
    }
    let entries = &entries[..WINDOW];
    let count = |gap: usize, test: &dyn Fn(u16, u16) -> bool| {
        (0..WINDOW - gap)
            .filter(|&i| test(entries[i], entries[i + gap]))
            .count()
    };
    let repeated = count(COLS, &|a, b| a == b);
    let runs = count(1, &|a, b| a & 0x03FF != 0 && b == a.wrapping_add(1));
    let below = count(COLS, &|a, b| {
        a & 0x03FF != 0 && b == a.wrapping_add(COLS as u16)
    });
    let flipped = entries.iter().filter(|&&e| e & 0x0C00 != 0).count();
    if repeated < MIN_REPEATED || runs < MIN_RUNS || below < MIN_RUNS || flipped > MAX_FLIPPED {
        return None;
    }
    let mut same = BTreeSet::new();
    let mut seams = BTreeSet::new();
    for i in 0..WINDOW {
        // A paleta nao muda o desenho.
        let a = entries[i] & 0x0FFF;
        for (j, vertical) in [(i + 1, false), (i + COLS, true)] {
            if j >= WINDOW || (!vertical && j % COLS == 0) {
                continue;
            }
            let b = entries[j] & 0x0FFF;
            if a == b {
                same.insert(((a & 0x03FF) as usize, vertical));
                continue;
            }
            let step = if vertical { COLS as u16 } else { 1 };
            if (a | b) & 0x0C00 == 0 && b == a.wrapping_add(step) {
                continue;
            }
            seams.insert((a, b, vertical));
        }
    }
    Some(Window {
        highest: entries.iter().map(|&e| (e & 0x03FF) as usize).max()?,
        repeated: same.into_iter().collect(),
        seams: seams.into_iter().collect(),
    })
}

/// (pixels iguais, pixels medidos) nas emendas com desenho da janela, com o
/// comeco do bloco em `base`; `wrap` conta os quadros em volta do fim da
/// imagem (pra medir comeco vizinho que passaria do fim).
fn seam(window: &Window, tiles: &Measured, base: usize, wrap: bool) -> (u32, u32) {
    let index = |e: u16| {
        let t = tile_of(e, base);
        if wrap {
            t % tiles.count
        } else {
            t
        }
    };
    let (mut same, mut seen) = (0, 0);
    for &(a, b, vertical) in &window.seams {
        let (ta, tb) = (index(a), index(b));
        let (Some(ea), Some(eb)) = (tiles.edges.get(ta), tiles.edges.get(tb)) else {
            continue;
        };
        // Letra com letra nao conta: a emenda entre duas linhas de texto cai
        // no topo da letra de baixo tanto no desenho certo quanto num
        // qualquer.
        if tiles.lettered[ta] && tiles.lettered[tb] {
            continue;
        }
        let (ea, eb) = (ea[usize::from(a >> 10)], eb[usize::from(b >> 10)]);
        let (x, y) = match vertical {
            true => (ea[BOTTOM], eb[TOP]),
            false => (ea[RIGHT], eb[LEFT]),
        };
        if flat(x) && flat(y) {
            continue;
        }
        seen += 8;
        same += same_pixels(x, y);
    }
    (same, seen)
}

/// O comeco do bloco na imagem que faz da janela um desenho continuo, e
/// quanto as emendas nele passam da media nos comecos vizinhos.
fn pairing(window: &Window, tiles: &Measured) -> Option<(usize, f64)> {
    let span = tiles.count.checked_sub(window.highest)?;
    // Comecos em que toda entrada repetida emenda nela mesma: um bit cada.
    let mut fits: Vec<u64> = (0..span.div_ceil(64))
        .map(|k| match span - 64 * k {
            rest if rest >= 64 => u64::MAX,
            rest => (1 << rest) - 1,
        })
        .collect();
    for &(t, vertical) in &window.repeated {
        let set = if vertical {
            &tiles.above
        } else {
            &tiles.beside
        };
        for (k, word) in fits.iter_mut().enumerate() {
            *word &= bits_from(set, t + 64 * k);
        }
    }
    let mut best: Option<(f64, usize)> = None;
    for (k, &word) in fits.iter().enumerate() {
        let mut word = word;
        while word != 0 {
            let base = 64 * k + word.trailing_zeros() as usize;
            word &= word - 1;
            let (same, seen) = seam(window, tiles, base, false);
            if seen < MIN_SEAMS * 8 || same * 10 < seen * MIN_SEAM {
                continue;
            }
            let score = f64::from(same) / f64::from(seen);
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, base));
            }
        }
    }
    let (score, base) = best?;
    // Os vizinhos de perto (ate 2) ainda pegam o mesmo desenho deslocado.
    let around: Vec<f64> = (base.saturating_sub(AROUND)..=base + AROUND)
        .filter(|&b| b.abs_diff(base) > 2)
        .map(|b| seam(window, tiles, b, true))
        .filter(|&(_, seen)| seen > 0)
        .map(|(same, seen)| f64::from(same) / f64::from(seen))
        .collect();
    if around.is_empty() {
        return None;
    }
    let margin = score - around.iter().sum::<f64>() / around.len() as f64;
    (margin >= MIN_MARGIN).then_some((base, margin))
}

/// As imagens montadas por mapa de quadros: janelas com cara de mapa (a cada
/// `STEP` entradas) emparelhadas com a imagem de `pictures` que mais as torna
/// um desenho continuo, as vizinhas com a mesma imagem e o mesmo comeco
/// juntas num mapa so.
pub fn find(data: &[u8], pictures: &[Picture]) -> Vec<Picture> {
    let sources: Vec<(&Picture, Measured)> = pictures
        .iter()
        .filter(|p| p.map.is_none())
        .filter_map(|p| Some((p, measure(&tiles_of(data, p)?))))
        .filter(|(_, tiles)| tiles.count > 0)
        .collect();
    let total = data.len() / 2;
    let mut found: Vec<(usize, usize, usize)> = Vec::new();
    let mut buffer = vec![0u16; WINDOW];
    let mut at = 0;
    while at + WINDOW <= total {
        for (k, slot) in buffer.iter_mut().enumerate() {
            let i = 2 * (at + k);
            *slot = u16::from_le_bytes([data[i], data[i + 1]]);
        }
        let start = at;
        at += STEP;
        let Some(window) = window(&buffer) else {
            continue;
        };
        let best = sources
            .iter()
            .enumerate()
            .filter_map(|(i, (_, tiles))| {
                let (base, margin) = pairing(&window, tiles)?;
                let lettered = buffer
                    .iter()
                    .map(|&e| tile_of(e, base))
                    .filter(|&t| tiles.lettered.get(t).copied().unwrap_or(false))
                    .collect::<HashSet<_>>()
                    .len();
                (lettered >= MIN_LETTERED).then_some((margin, i, base))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, source, base)) = best {
            found.push((start, source, base));
        }
    }
    // Janelas vizinhas (sobrepostas) com a mesma imagem e o mesmo comeco: um
    // mapa so, da primeira ao fim da ultima.
    let mut spans: Vec<(usize, usize, usize, usize)> = Vec::new();
    for (start, source, base) in found {
        match spans.last_mut() {
            Some(last) if last.2 == source && last.3 == base && start <= last.1 => {
                last.1 = start + WINDOW
            }
            _ => spans.push((start, start + WINDOW, source, base)),
        }
    }
    spans
        .into_iter()
        .filter_map(|(start, end, source, base)| {
            let (picture, tiles) = &sources[source];
            let raw: Vec<u16> = (start..end)
                .map(|i| u16::from_le_bytes([data[2 * i], data[2 * i + 1]]))
                .collect();
            // As linhas de uma entrada so, nas pontas, ficam de fora: sao o
            // vao entre um mapa e outro.
            let first = row_start(&raw);
            let rows: Vec<&[u16; COLS]> = raw[first..].as_chunks::<COLS>().0.iter().collect();
            let blank = |row: &[u16; COLS]| row.iter().all(|&e| e == row[0]);
            let top = rows.iter().take_while(|row| blank(row)).count();
            let bottom = rows.len() - rows.iter().rev().take_while(|row| blank(row)).count();
            if top >= bottom {
                return None;
            }
            let map = TileMap {
                offset: 2 * (start + first + top * COLS),
                rows: bottom - top,
                base,
            };
            let lettered = rows[top..bottom]
                .iter()
                .flat_map(|row| row.iter())
                .map(|&e| tile_of(e, base))
                .filter(|&t| tiles.lettered.get(t).copied().unwrap_or(false))
                .collect::<HashSet<_>>()
                .len();
            Some(Picture {
                id: format!("map-{:08x}", map.offset),
                offset: picture.offset,
                packed: picture.packed,
                bytes: picture.bytes,
                lettered,
                sprite: None,
                map: Some(map),
            })
        })
        .collect()
}

/// Onde comeca uma linha do mapa dentro da faixa achada: numa posicao que nao
/// corta fila de quadros seguidos (a palavra guardada em ordem), de
/// preferencia logo depois de duas colunas de sobra — a tela do GBA mostra
/// 30 das 32 colunas, e as 2 de fora quase sempre tem uma entrada so. Contar
/// a emenda nao serve: a borda da tela e o fundo transparente emendam com
/// qualquer coisa.
fn row_start(entries: &[u16]) -> usize {
    let mut cuts = [0usize; COLS];
    for i in 1..entries.len() {
        let (a, b) = (entries[i - 1], entries[i]);
        if a & 0x03FF != 0 && b == a.wrapping_add(1) {
            cuts[i % COLS] += 1;
        }
    }
    let spare = |c: usize| {
        let column: Vec<u16> = entries.iter().skip(c).step_by(COLS).copied().collect();
        let most = column
            .iter()
            .map(|e| column.iter().filter(|&x| x == e).count())
            .max()
            .unwrap_or(0);
        most * 10 >= column.len() * 9
    };
    (0..COLS)
        .step_by(2)
        .min_by_key(|&k| {
            let padded = spare((k + COLS - 1) % COLS) && spare((k + COLS - 2) % COLS);
            (cuts[k], !padded, k)
        })
        .unwrap_or(0)
}

/// Os quadros (contados na imagem) que o mapa mostra.
pub fn shown(data: &[u8], map: &TileMap) -> HashSet<usize> {
    entries(data, map)
        .unwrap_or_default()
        .iter()
        .map(|&e| tile_of(e, map.base))
        .collect()
}

/// Guarda as celulas redesenhadas (`after`, comparadas com `before`, 32
/// bytes por celula). Cada desenho novo vai pra um quadro do mapa que fica
/// igual e ja tem aquele desenho (o fundo, em qualquer espelho), pra um ja
/// escrito, ou pra um quadro livre: que so as celulas redesenhadas usavam e
/// que nenhum outro mapa achado mostra (`shared`) — o da propria celula
/// primeiro. Quadro livre que ninguem toma fica com o desenho novo da celula
/// que saiu dele: a tela que o jogo monta de outro jeito com ele (e que a
/// busca nao achou) ve o mesmo que este mapa. Muda `tiles` e devolve as
/// entradas novas; `Err` se faltar quadro — e nada muda.
// ponytail: quadro livre tomado por outra celula muda tambem numa tela que a
// busca nao achou; so o mapa achado sabe quem mais usa o quadro.
pub fn store(
    tiles: &mut [u8],
    entries: &[u16],
    base: usize,
    before: &[u8],
    after: &[u8],
    shared: &HashSet<usize>,
) -> Result<Vec<u16>, String> {
    let count = tiles.len() / TILE_BYTES;
    let cell = |bytes: &[u8], c: usize| -> [u8; TILE_BYTES] {
        let mut out = [0u8; TILE_BYTES];
        if let Some(src) = bytes.get(c * TILE_BYTES..(c + 1) * TILE_BYTES) {
            out.copy_from_slice(src);
        }
        out
    };
    let changed: Vec<usize> = (0..entries.len())
        .filter(|&c| cell(before, c) != cell(after, c))
        .collect();
    let mut users: HashMap<usize, Vec<usize>> = HashMap::new();
    for (c, &e) in entries.iter().enumerate() {
        let t = tile_of(e, base);
        if t < count {
            users.entry(t).or_default().push(c);
        }
    }
    let redrawn: HashSet<usize> = changed.iter().copied().collect();
    let mut free: BTreeSet<usize> = users
        .iter()
        .filter(|(t, cells)| !shared.contains(t) && cells.iter().all(|c| redrawn.contains(c)))
        .map(|(&t, _)| t)
        .collect();
    // Os desenhos que ficam: os quadros do mapa que nao sao livres, em todo
    // espelho.
    let mut kept: HashMap<[u8; TILE_BYTES], (usize, u16)> = HashMap::new();
    let mut stay: Vec<usize> = users
        .keys()
        .filter(|t| !free.contains(t))
        .copied()
        .collect();
    stay.sort_unstable();
    for t in stay {
        for flips in 0..4 {
            kept.entry(flip(&tiles[t * TILE_BYTES..(t + 1) * TILE_BYTES], flips))
                .or_insert((t, flips));
        }
    }
    let mut out = entries.to_vec();
    let mut written: HashMap<[u8; TILE_BYTES], usize> = HashMap::new();
    let mut new_tiles: Vec<(usize, [u8; TILE_BYTES])> = Vec::new();
    let mut left: HashMap<usize, [u8; TILE_BYTES]> = HashMap::new();
    for &c in &changed {
        let want = cell(after, c);
        let own = tile_of(entries[c], base);
        let (t, flips) = if let Some(&found) = kept.get(&want) {
            found
        } else if let Some(&t) = written.get(&want) {
            (t, 0)
        } else {
            let t = if free.remove(&own) {
                own
            } else {
                free.pop_first().ok_or_else(|| {
                    "o desenho novo precisa de mais quadros do que o trecho libera: os \
                     quadros dele se repetem no desenho ou aparecem em outra tela"
                        .to_string()
                })?
            };
            written.insert(want, t);
            new_tiles.push((t, want));
            (t, 0)
        };
        if t != own {
            left.entry(own).or_insert(want);
        }
        out[c] = (entries[c] & 0xF000) | (flips << 10) | (t - base) as u16;
    }
    for t in free {
        if let Some(&want) = left.get(&t) {
            new_tiles.push((t, want));
        }
    }
    for (t, want) in new_tiles {
        tiles[t * TILE_BYTES..(t + 1) * TILE_BYTES].copy_from_slice(&want);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flipping_twice_gives_the_tile_back() {
        let tile: Vec<u8> = (0..TILE_BYTES as u8).map(|i| i.wrapping_mul(37)).collect();
        for flips in 0..4 {
            assert_eq!(flip(&flip(&tile, flips), flips).as_slice(), tile.as_slice());
        }
        // Espelho horizontal: o primeiro pixel vira o ultimo da linha.
        let mut one = [0u8; TILE_BYTES];
        one[0] = 0x05;
        assert_eq!(flip(&one, 1)[3], 0x50);
        assert_eq!(flip(&one, 2)[28], 0x05);
    }

    #[test]
    fn bits_are_read_across_words() {
        let set = [0x8000_0000_0000_0001u64, 0x3];
        assert_eq!(bits_from(&set, 0), 0x8000_0000_0000_0001);
        assert_eq!(bits_from(&set, 63), 0b111);
        assert_eq!(bits_from(&set, 200), 0);
    }

    use crate::synth::{
        make_gba_rom_with_tilemap, TILEMAP_LIST_CELL, TILEMAP_MAP_AT, TILEMAP_OTHER_MAP_AT,
        TILEMAP_PITCH, TILEMAP_REPEAT_CELL, TILEMAP_TILES_AT,
    };
    use crate::types::TextEntry;

    /// A linha 2 da lista (partida entre duas tiras na ordem guardada), como
    /// frase desenhada traduzida.
    fn line_two(data: &[u8], picture: &Picture, translation: &str) -> TextEntry {
        let (_, sheet) = super::super::open(data, picture).unwrap();
        let (lr, lc) = TILEMAP_LIST_CELL;
        let y = lr * TILE + 2 * TILEMAP_PITCH + 3;
        let block = super::super::blocks(&sheet)
            .into_iter()
            .find(|b| b.y == y)
            .expect("a linha 2 vira um trecho inteiro na folha montada");
        assert_eq!((block.x, block.h), (lc * TILE + 2, 7), "{block:?}");
        TextEntry {
            translated_text: Some(translation.to_string()),
            ..super::super::entries_of(picture, &[block], &["ことば".to_string()]).remove(0)
        }
    }

    #[test]
    fn a_picture_built_by_a_tilemap_is_found_and_written_where_the_screen_shows_it() {
        let mut data = make_gba_rom_with_tilemap();
        let found = super::super::find(&data);
        let raw = found
            .iter()
            .find(|p| p.map.is_none() && p.offset <= TILEMAP_TILES_AT)
            .expect("os quadros crus sao achados");
        let maps: Vec<&Picture> = found.iter().filter(|p| p.map.is_some()).collect();
        assert_eq!(maps.len(), 2, "{maps:?}");
        let base = (TILEMAP_TILES_AT - raw.offset) / TILE_BYTES;
        for map in &maps {
            assert_eq!((map.offset, map.map.unwrap().base), (raw.offset, base));
        }
        // O painel comeca onde o mapa comeca; a outra tela tem colunas vazias
        // dos dois lados, e qualquer comeco nelas mostra o mesmo desenho.
        let (panel, other) = match maps[0].map.unwrap().offset == TILEMAP_MAP_AT {
            true => (maps[0].clone(), maps[1].clone()),
            false => (maps[1].clone(), maps[0].clone()),
        };
        assert_eq!(panel.id, format!("map-{TILEMAP_MAP_AT:08x}"));
        assert!(other.map.unwrap().offset.abs_diff(TILEMAP_OTHER_MAP_AT) < 64);

        let open = |data: &[u8], p: &Picture| super::super::open(data, p).unwrap().1;
        let (panel_before, other_before) = (open(&data, &panel), open(&data, &other));
        let stored_before = data.clone();
        let entry = line_two(&data, &panel, "Ok");
        let applied = super::super::apply(&mut data, &[entry]);
        assert_eq!((applied.pictures, applied.drawn), (1, 1), "{applied:?}");
        assert!(applied.left_out.is_empty(), "{applied:?}");
        // A outra tela, que mostra o comeco da linha 2 com os mesmos quadros,
        // fica igual; no painel so muda a faixa da linha 2 — a celula que
        // repete um quadro dela tambem fica como era.
        assert_eq!(open(&data, &other), other_before);
        let panel_after = open(&data, &panel);
        assert_ne!(panel_after, panel_before);
        let (lr, _) = TILEMAP_LIST_CELL;
        let top = lr * TILE + 2 * TILEMAP_PITCH;
        let rows = top / TILE..=(top + 10) / TILE;
        assert!(!rows.contains(&TILEMAP_REPEAT_CELL.0));
        for y in (0..panel_before.height).filter(|y| !rows.contains(&(y / TILE))) {
            for x in 0..panel_before.width {
                assert_eq!(panel_after.get(x, y), panel_before.get(x, y), "({x}, {y})");
            }
        }
        // Fora dos quadros e do mapa do painel, nada muda.
        let tiles = raw.offset..raw.offset + raw.bytes;
        let map = TILEMAP_MAP_AT..TILEMAP_MAP_AT + 2048;
        assert!((0..data.len())
            .filter(|&i| data[i] != stored_before[i])
            .all(|i| tiles.contains(&i) || map.contains(&i)));
    }

    #[test]
    fn a_drawing_that_needs_more_tiles_than_it_frees_is_left_out_untouched() {
        let mut data = make_gba_rom_with_tilemap();
        let panel = super::super::find(&data)
            .into_iter()
            .find(|p| p.map.is_some_and(|m| m.offset == TILEMAP_MAP_AT))
            .unwrap();
        // Larga: precisa de um quadro novo em cada celula das duas linhas de
        // quadros, e a de cima mostra quadros da outra tela.
        let before = data.clone();
        let entry = line_two(&data, &panel, "Banana Ok");
        let applied = super::super::apply(&mut data, &[entry]);
        assert_eq!(applied.drawn, 0, "{applied:?}");
        assert_eq!(applied.left_out.len(), 1);
        assert!(applied.left_out[0].1.contains("quadros"), "{applied:?}");
        assert!(data == before, "nada muda");
    }

    #[test]
    fn a_vacated_tile_gets_the_drawing_of_the_cell_that_left_it() {
        // Duas celulas, cada uma com seu quadro; a primeira vira fundo (igual
        // ao quadro 0, que fica) e a segunda ganha desenho novo.
        let mut tiles = vec![0u8; 3 * TILE_BYTES];
        tiles[TILE_BYTES..2 * TILE_BYTES].fill(0x55);
        tiles[2 * TILE_BYTES..].fill(0x66);
        let entries = [1u16, 2, 0];
        let before: Vec<u8> = cells(&tiles, &entries, 0);
        let mut after = before.clone();
        after[..TILE_BYTES].fill(0);
        after[TILE_BYTES..2 * TILE_BYTES].fill(0x77);
        let out = store(&mut tiles, &entries, 0, &before, &after, &HashSet::new()).unwrap();
        // A primeira aponta pro fundo; o quadro dela, que ninguem mais usa,
        // fica vazio como ela (outra tela com ele ve o mesmo).
        assert_eq!(out, [0, 2, 0]);
        assert!(tiles[TILE_BYTES..2 * TILE_BYTES].iter().all(|&b| b == 0));
        assert!(tiles[2 * TILE_BYTES..].iter().all(|&b| b == 0x77));
        // Quadro de outra tela nao e livre: sem quadro, nada muda.
        let mut tiles = vec![0x11u8; 2 * TILE_BYTES];
        let entries = [1u16];
        let before = cells(&tiles, &entries, 0);
        let after = vec![0x22u8; TILE_BYTES];
        let shared = HashSet::from([1usize]);
        assert!(store(&mut tiles, &entries, 0, &before, &after, &shared).is_err());
        assert!(tiles.iter().all(|&b| b == 0x11));
    }

    #[test]
    fn no_map_in_empty_short_or_random_data() {
        assert!(window(&[]).is_none());
        assert!(window(&[0u16; 10]).is_none());
        for data in [
            Vec::new(),
            vec![0u8; 3],
            crate::synth::make_random(64 * 1024, 7),
        ] {
            assert!(find(&data, &super::super::find(&data)).is_empty());
        }
    }
}

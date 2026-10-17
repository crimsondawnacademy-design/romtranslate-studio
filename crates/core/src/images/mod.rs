//! Texto desenhado em imagem: o japones que o jogo guarda como desenho
//! (aviso de saude da abertura, rotulos de menu, teclado de nomes), e nao
//! como codigo de letra. Medido em 14/10/2026 num RPG japones de GBA: as
//! telas de nomes e de ajustes guardam os paineis em quadros 4bpp crus, um
//! atras do outro na ordem da tela (32 por linha), e o aviso de saude em
//! quadros comprimidos (LZ77) na mesma ordem. Desenhados assim, 32 por
//! linha, o texto ja e legivel.
//!
//! O caminho: `find` acha as imagens com quadros que parecem letra (local,
//! sem IA); quem usa escolhe as que tem japones; `blocks` separa os trechos
//! de letra de cada uma; a IA le os trechos (`reading_sheet`, `READ_PROMPT`,
//! `parse_reading`); a traducao segue o caminho de qualquer frase
//! (`TextEncoding::Image`); e `apply` apaga o japones e escreve a traducao
//! com a fonte do app (`font`), nas cores do original.
// ponytail: so quadros 4bpp de 8x8 na ordem da tela (32 por linha), crus ou
// em LZ77 tipo 0x10. Imagem montada por mapa de quadros fora de ordem,
// 8bpp ou com outra compressao fica de fora; a imagem comprimida que,
// regravada, passa do tamanho original nao muda de lugar (fica no original).

pub mod font;

use font::Font;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::lz77;
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

/// Bytes de um quadro de 8x8 pixels com 4 bits por pixel (o do GBA e do DS).
pub const TILE_BYTES: usize = 32;
const TILE: usize = 8;
/// Quadros por linha da folha: a largura de uma tela de GBA guardada em ordem
/// (256 pixels).
pub const SHEET_TILES: usize = 32;
/// Imagem com menos quadros que isto nao e tela nem painel.
const MIN_TILES: usize = 32;
/// Janela da varredura de quadros crus, fora dos blocos comprimidos.
const RAW_WINDOW: usize = 8 * 1024;
/// Quadros que parecem letra pra imagem valer a pena (`worth`).
const MIN_LETTERED: usize = 24;
/// Margem em volta de cada trecho na folha que vai pra IA.
const MARGIN: usize = 2;
/// Ampliacao dos trechos na folha da IA: letra de 12 pixels e pequena demais
/// pra ler.
const ZOOM: usize = 3;

/// Uma imagem do arquivo: quadros 4bpp crus ou num bloco LZ77.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    pub id: String,
    pub offset: usize,
    /// Bytes do bloco LZ77 no arquivo; `None` = quadros crus.
    pub packed: Option<usize>,
    /// Bytes dos quadros, ja descomprimidos.
    pub bytes: usize,
    /// Quadros que parecem letra (`lettered`).
    pub lettered: usize,
    /// Os quadros vem em sprites deste tamanho (largura, altura, em quadros),
    /// um atras do outro: e assim que a folha os poe lado a lado. `None` =
    /// na ordem da tela, 32 por linha.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite: Option<[usize; 2]>,
}

/// Formatos de sprite que `arrangement` experimenta (largura, altura, em
/// quadros).
const SPRITES: [[usize; 2]; 6] = [[1, 2], [2, 2], [4, 2], [8, 2], [2, 4], [4, 4]];

/// Onde o quadro `t` (na ordem guardada) fica na folha, em quadros.
fn sheet_index(t: usize, sprite: Option<[usize; 2]>) -> usize {
    let Some([w, h]) = sprite else {
        return t;
    };
    let (s, k) = (t / (w * h), t % (w * h));
    let per_row = SHEET_TILES / w;
    let (x, y) = ((s % per_row) * w + k % w, (s / per_row) * h + k / w);
    y * SHEET_TILES + x
}

/// Os quadros na ordem da folha (quadro vazio onde o sprite nao tem).
fn arrange(tiles: &[u8], sprite: Option<[usize; 2]>) -> Vec<u8> {
    let count = tiles.len() / TILE_BYTES;
    let slots = (0..count)
        .map(|t| sheet_index(t, sprite) + 1)
        .max()
        .unwrap_or(0);
    let mut out = vec![0u8; slots * TILE_BYTES];
    for (t, tile) in tiles.as_chunks::<TILE_BYTES>().0.iter().enumerate() {
        let at = sheet_index(t, sprite) * TILE_BYTES;
        out[at..at + TILE_BYTES].copy_from_slice(tile);
    }
    out
}

/// O contrario de `arrange`: os `count` quadros da folha de volta na ordem
/// guardada.
fn unarrange(sheet_tiles: &[u8], sprite: Option<[usize; 2]>, count: usize) -> Vec<u8> {
    (0..count)
        .flat_map(|t| {
            let at = sheet_index(t, sprite) * TILE_BYTES;
            sheet_tiles[at..at + TILE_BYTES].iter().copied()
        })
        .collect()
}

/// A folha da imagem, com os quadros arrumados como ela os guarda.
pub fn sheet_of(tiles: &[u8], picture: &Picture) -> Sheet {
    Sheet::new(&arrange(tiles, picture.sprite))
}

/// Traco de letra que continua de um quadro pro de baixo, com os quadros
/// arrumados em `sprite`: em cada coluna das emendas entre dois quadros que a
/// imagem tem, tinta (cor diferente da mais comum) nos dois lados.
fn seams(tiles: &[u8], sprite: Option<[usize; 2]>) -> usize {
    let count = tiles.len() / TILE_BYTES;
    let px = |t: usize| pixels(&tiles[t * TILE_BYTES..(t + 1) * TILE_BYTES]);
    let mut seen = [0usize; 16];
    for t in 0..count {
        for p in px(t) {
            seen[p as usize] += 1;
        }
    }
    let paper = (0..16).max_by_key(|&c| seen[c]).unwrap_or(0) as u8;
    let at: HashMap<usize, usize> = (0..count).map(|t| (sheet_index(t, sprite), t)).collect();
    at.iter()
        .filter_map(|(&i, &t)| Some((t, *at.get(&(i + SHEET_TILES))?)))
        .map(|(up, down)| {
            let (up, down) = (px(up), px(down));
            (0..TILE)
                .filter(|&x| up[(TILE - 1) * TILE + x] != paper && down[x] != paper)
                .count()
        })
        .sum()
}

/// Como os quadros vem guardados: na ordem da tela ou em sprites (`SPRITES`)
/// — o que deixa mais traco continuo entre um quadro e o de baixo, se deixa
/// bem mais que a ordem da tela. No jogo medido, o "aperte um botao" do
/// aviso de saude e uma fila de sprites de 4x2 quadros: na ordem da tela as
/// metades das letras saiam lado a lado, ilegiveis.
fn arrangement(tiles: &[u8]) -> Option<[usize; 2]> {
    let plain = seams(tiles, None);
    // Empate fica com o sprite menor: o de 4x4 tambem junta cada metade de
    // cima com a de baixo numa fila de 4x2, mas empilha os sprites.
    SPRITES
        .iter()
        .enumerate()
        .map(|(i, &sprite)| (seams(tiles, Some(sprite)), i, sprite))
        .filter(|&(joined, _, _)| joined > plain * 3 / 2 + 8)
        .max_by_key(|&(joined, i, _)| (joined, std::cmp::Reverse(i)))
        .map(|(_, _, sprite)| sprite)
}

/// Um trecho de letra numa imagem, em pixels da folha.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    /// Cor do fundo (indice da paleta).
    pub paper: u8,
    /// Cor da letra.
    pub ink: u8,
    /// Cor da sombra da letra, um pixel abaixo e a direita, se o original tem.
    pub shade: Option<u8>,
    /// Largura que a traducao pode ocupar: o trecho e o fundo livre a direita.
    pub room: usize,
    /// O retangulo dos quadros do trecho (x0, y0, x1, y1): e nele que o
    /// japones e apagado, furigana inclusive.
    pub area: [usize; 4],
}

fn id_of(offset: usize) -> String {
    format!("img-{offset:08x}")
}

/// Os 64 pixels de um quadro, linha a linha.
fn pixels(tile: &[u8]) -> [u8; 64] {
    let mut px = [0u8; 64];
    for (i, byte) in tile.iter().take(TILE_BYTES).enumerate() {
        px[i * 2] = byte & 0x0F;
        px[i * 2 + 1] = byte >> 4;
    }
    px
}

/// O quadro parece um pedaco de letra: 2 ou 3 cores, a tinta (o que nao e a
/// cor do fundo) entre 5 e 29 dos 64 pixels, e nem todas as linhas nem todas
/// as colunas iguais (listra e enfeite).
fn lettered(px: &[u8; 64]) -> bool {
    let mut count = [0usize; 16];
    for &p in px {
        count[p as usize] += 1;
    }
    let colors = count.iter().filter(|&&n| n > 0).count();
    let ink = 64 - count.iter().max().copied().unwrap_or(0);
    let rows_same = (1..TILE).all(|y| px[y * TILE..(y + 1) * TILE] == px[..TILE]);
    let cols_same = (1..TILE).all(|x| (0..TILE).all(|y| px[y * TILE + x] == px[y * TILE]));
    (2..=3).contains(&colors) && (5..=29).contains(&ink) && !rows_same && !cols_same
}

/// (quadros que parecem letra, quadros com desenho) de uma sequencia de
/// quadros.
fn score(bytes: &[u8]) -> (usize, usize) {
    let mut lettered_tiles = 0;
    let mut drawn = 0;
    for tile in bytes.as_chunks::<TILE_BYTES>().0 {
        let px = pixels(tile);
        if px.iter().any(|&p| p != px[0]) {
            drawn += 1;
            lettered_tiles += usize::from(lettered(&px));
        }
    }
    (lettered_tiles, drawn)
}

/// Quadro de uma cor so (fundo, preenchimento).
fn uniform(tile: &[u8]) -> bool {
    let px = pixels(tile);
    px.iter().all(|&p| p == px[0])
}

/// Onde os quadros de um trecho cru comecam de verdade: o deslocamento (de 4
/// em 4 bytes, ate 28) com mais quadros de uma cor so. Com o deslocamento
/// errado cada quadro sai uma linha de pixel torto — a letra ainda se le, mas
/// o redesenho atravessa a borda do quadro. No jogo medido, o painel de
/// ajustes comeca 28 bytes depois da janela: 471 quadros de uma cor la,
/// contra uns 380 nos outros deslocamentos.
fn phase(data: &[u8]) -> usize {
    (0..TILE_BYTES)
        .step_by(4)
        .max_by_key(|&phase| {
            let one_color = data.get(phase..).map_or(0, |rest| {
                rest.as_chunks::<TILE_BYTES>()
                    .0
                    .iter()
                    .filter(|t| uniform(t.as_slice()))
                    .count()
            });
            (one_color, std::cmp::Reverse(phase))
        })
        .unwrap_or(0)
}

fn worth(lettered_tiles: usize, drawn: usize) -> bool {
    lettered_tiles >= MIN_LETTERED && lettered_tiles * 10 >= drawn * 3
}

/// As imagens de `data` com quadros que parecem letra: blocos LZ77 de quadros
/// e trechos de quadros crus (janelas de `RAW_WINDOW` juntadas com as
/// vizinhas), fora dos blocos comprimidos. Em ordem de posicao.
pub fn find(data: &[u8]) -> Vec<Picture> {
    let mut found = Vec::new();
    let mut packed_spans = Vec::new();
    let mut budget = lz77::SCAN_BUDGET;
    lz77::each_block(data, &mut budget, |at, block| {
        packed_spans.push((at, block.packed_len));
        let tiles = block.data.len() / TILE_BYTES;
        if !block.data.len().is_multiple_of(TILE_BYTES) || tiles < MIN_TILES {
            return;
        }
        let (lettered_tiles, drawn) = score(&block.data);
        if worth(lettered_tiles, drawn) {
            found.push(Picture {
                id: id_of(at),
                offset: at,
                packed: Some(block.packed_len),
                bytes: block.data.len(),
                lettered: lettered_tiles,
                sprite: arrangement(&block.data),
            });
        }
    });
    let packed_at =
        |at: usize, end: usize| packed_spans.iter().any(|&(p, n)| p < end && at < p + n);
    // Quadros crus: as janelas que passam no corte (quadro cru solto e mais
    // raro que bloco comprimido, entao pede mais letra), cada uma com a
    // vizinha de cada lado — o painel que comeca no fim de uma janela que nao
    // passou saia cortado no meio (no jogo medido, a lista de cores da tela de
    // ajustes) —, juntas com as encostadas.
    let windows = data.len() / RAW_WINDOW;
    let lettered_in: Vec<usize> = (0..windows)
        .map(|w| {
            let (at, end) = (w * RAW_WINDOW, (w + 1) * RAW_WINDOW);
            let (lettered_tiles, drawn) = score(&data[at..end]);
            let good = lettered_tiles >= 64 && lettered_tiles * 10 >= drawn * 4;
            if good && !packed_at(at, end) {
                lettered_tiles
            } else {
                0
            }
        })
        .collect();
    let taken = |w: usize| {
        let (at, end) = (w * RAW_WINDOW, (w + 1) * RAW_WINDOW);
        let near = |k: Option<usize>| k.is_some_and(|k| lettered_in.get(k).is_some_and(|&n| n > 0));
        !packed_at(at, end) && (near(Some(w)) || near(w.checked_sub(1)) || near(Some(w + 1)))
    };
    let mut w = 0;
    while w < windows {
        if !taken(w) {
            w += 1;
            continue;
        }
        let first = w;
        while w < windows && taken(w) {
            w += 1;
        }
        let (at, len) = (first * RAW_WINDOW, (w - first) * RAW_WINDOW);
        let phase = phase(&data[at..at + len]);
        let bytes = if phase == 0 { len } else { len - TILE_BYTES };
        found.push(Picture {
            id: id_of(at + phase),
            offset: at + phase,
            packed: None,
            bytes,
            lettered: lettered_in[first..w].iter().sum(),
            sprite: arrangement(&data[at + phase..at + phase + bytes]),
        });
    }
    found.sort_by_key(|picture| picture.offset);
    found
}

/// Os quadros da imagem (descomprimidos), lidos de `data`.
pub fn tiles_of(data: &[u8], picture: &Picture) -> Option<Vec<u8>> {
    match picture.packed {
        Some(_) => lz77::unpack(data.get(picture.offset..)?)
            .map(|unpacked| unpacked.data)
            .filter(|tiles| tiles.len() == picture.bytes),
        None => data
            .get(picture.offset..picture.offset.checked_add(picture.bytes)?)
            .map(<[u8]>::to_vec),
    }
}

/// Uma imagem aberta em pixels (indices da paleta), 32 quadros por linha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    pub width: usize,
    pub height: usize,
    tiles: usize,
    px: Vec<u8>,
}

impl Sheet {
    pub fn new(tiles: &[u8]) -> Sheet {
        let count = tiles.len() / TILE_BYTES;
        let (width, height) = (SHEET_TILES * TILE, count.div_ceil(SHEET_TILES) * TILE);
        let mut px = vec![0u8; width * height];
        for (t, tile) in tiles.as_chunks::<TILE_BYTES>().0.iter().enumerate() {
            let tile_px = pixels(tile);
            let (tx, ty) = ((t % SHEET_TILES) * TILE, (t / SHEET_TILES) * TILE);
            for y in 0..TILE {
                px[(ty + y) * width + tx..(ty + y) * width + tx + TILE]
                    .copy_from_slice(&tile_px[y * TILE..(y + 1) * TILE]);
            }
        }
        Sheet {
            width,
            height,
            tiles: count,
            px,
        }
    }

    /// O pixel esta num quadro que a imagem tem (a ultima linha da folha
    /// pode ficar incompleta).
    fn inside(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height && (y / TILE) * SHEET_TILES + x / TILE < self.tiles
    }

    pub fn get(&self, x: usize, y: usize) -> Option<u8> {
        self.inside(x, y).then(|| self.px[y * self.width + x])
    }

    fn set(&mut self, x: usize, y: usize, color: u8) {
        if self.inside(x, y) {
            self.px[y * self.width + x] = color & 0x0F;
        }
    }

    /// Os quadros de volta, na ordem e no formato de onde vieram.
    pub fn tiles(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.tiles * TILE_BYTES);
        for t in 0..self.tiles {
            let (tx, ty) = ((t % SHEET_TILES) * TILE, (t / SHEET_TILES) * TILE);
            for y in 0..TILE {
                for x in (0..TILE).step_by(2) {
                    let row = (ty + y) * self.width + tx;
                    out.push(self.px[row + x] | (self.px[row + x + 1] << 4));
                }
            }
        }
        out
    }

    fn tile_px(&self, tx: usize, ty: usize) -> [u8; 64] {
        let mut px = [0u8; 64];
        for y in 0..TILE {
            for x in 0..TILE {
                px[y * TILE + x] = self.px[(ty * TILE + y) * self.width + tx * TILE + x];
            }
        }
        px
    }

    /// A folha em PNG, cinza: a cor 0 da paleta em branco, a 15 em preto.
    pub fn png(&self) -> Result<Vec<u8>> {
        let gray: Vec<u8> = self.px.iter().map(|&p| 255 - p * 17).collect();
        encode_png(self.width, self.height, &gray)
    }
}

fn encode_png(width: usize, height: usize, gray: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width as u32, height as u32);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| CoreError::Project(format!("png: {e}")))?;
        writer
            .write_image_data(gray)
            .map_err(|e| CoreError::Project(format!("png: {e}")))?;
    }
    Ok(out)
}

/// Os trechos de letra de uma imagem: grupos de quadros que parecem letra,
/// encostados (inclusive na diagonal), cada um com o retangulo da tinta, as
/// cores e o fundo livre a direita. Em ordem de leitura.
pub fn blocks(sheet: &Sheet) -> Vec<Block> {
    let (cols, rows) = (SHEET_TILES, sheet.height / TILE);
    // Cada quadro que parece letra, com o fundo dele e as outras cores (um
    // bit por cor): so entram no mesmo trecho quadros de mesmo fundo que
    // dividem uma cor de letra — no jogo medido, a lista de cores da tela de
    // ajustes vinha grudada no desenho da casa ao lado.
    let mut colors_at: Vec<Option<(u8, u16)>> = vec![None; cols * rows];
    for ty in 0..rows {
        for tx in 0..cols {
            if ty * cols + tx >= sheet.tiles {
                continue;
            }
            let px = sheet.tile_px(tx, ty);
            if lettered(&px) {
                let mut count = [0usize; 16];
                for &p in &px {
                    count[p as usize] += 1;
                }
                let paper = (0..16u8)
                    .max_by_key(|&c| (count[c as usize], std::cmp::Reverse(c)))
                    .unwrap_or(0);
                let others = (0..16u16)
                    .filter(|&c| count[c as usize] > 0 && c as u8 != paper)
                    .fold(0u16, |mask, c| mask | (1 << c));
                colors_at[ty * cols + tx] = Some((paper, others));
            }
        }
    }
    let mut seen = vec![false; cols * rows];
    let mut lines = Vec::new();
    for start in 0..cols * rows {
        let Some(key) = colors_at[start] else {
            continue;
        };
        if seen[start] {
            continue;
        }
        // Grupo de quadros encostados (inclusive na diagonal) de mesmas cores:
        // o retangulo deles, em quadros.
        let (mut x0, mut y0, mut x1, mut y1) = (cols, rows, 0, 0);
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        while let Some(cell) = queue.pop_front() {
            let (tx, ty) = (cell % cols, cell / cols);
            (x0, y0, x1, y1) = (x0.min(tx), y0.min(ty), x1.max(tx), y1.max(ty));
            for (dx, dy) in [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let (nx, ny) = (tx as isize + dx, ty as isize + dy);
                if nx < 0 || ny < 0 || nx as usize >= cols || ny as usize >= rows {
                    continue;
                }
                let next = ny as usize * cols + nx as usize;
                let alike = colors_at[next]
                    .is_some_and(|(paper, others)| paper == key.0 && others & key.1 != 0);
                if alike && !seen[next] {
                    seen[next] = true;
                    queue.push_back(next);
                }
            }
        }
        lines.extend(lines_in_group(
            sheet,
            key.0,
            x0 * TILE,
            y0 * TILE,
            (x1 + 1) * TILE,
            (y1 + 1) * TILE,
        ));
    }
    let out = blocks_of(sheet, lines);
    // Pedaco de um trecho maior (quadro so com a cor da sombra vira grupo
    // proprio) sai: o que tem metade ou mais da area dentro de outro maior.
    let inside: Vec<bool> = out
        .iter()
        .enumerate()
        .map(|(i, small)| {
            out.iter().enumerate().any(|(j, big)| {
                let overlap_w = (small.x + small.w)
                    .min(big.x + big.w)
                    .saturating_sub(small.x.max(big.x));
                let overlap_h = (small.y + small.h)
                    .min(big.y + big.h)
                    .saturating_sub(small.y.max(big.y));
                i != j
                    && big.w * big.h > small.w * small.h
                    && overlap_w * overlap_h * 2 >= small.w * small.h
            })
        })
        .collect();
    let out: Vec<Block> = out
        .into_iter()
        .zip(inside)
        .filter(|(_, inside)| !inside)
        .map(|(block, _)| block)
        .collect();
    // Furigana: trecho baixo logo acima de outro e dentro da largura dele. E
    // leitura do kanji de baixo, nao texto; o trecho de baixo o apaga junto.
    let furigana: Vec<bool> = out
        .iter()
        .map(|small| {
            small.h <= 7
                && out.iter().any(|big| {
                    big.h > small.h + 2
                        && big.y >= small.y + small.h
                        && big.y <= small.y + small.h + 6
                        && small.x >= big.x
                        && small.x + small.w <= big.x + big.w
                })
        })
        .collect();
    let mut out: Vec<Block> = out
        .into_iter()
        .zip(furigana)
        .filter(|(_, furigana)| !furigana)
        .map(|(block, _)| block)
        .collect();
    out.sort_by_key(|b| (b.y, b.x));
    out
}

/// As linhas de letra dentro do retangulo de quadros de um grupo, com o
/// fundo dele: letra = a cor que mais aparece fora o fundo; sombra = a
/// seguinte, se ela fica quase sempre logo abaixo e a direita da letra. Linha
/// de teclado (letras soltas a intervalos iguais) sai.
fn lines_in_group(
    sheet: &Sheet,
    paper: u8,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
) -> Vec<(Line, Colors)> {
    let mut count = [0usize; 16];
    for y in y0..y1 {
        for x in x0..x1 {
            if let Some(p) = sheet.get(x, y) {
                count[p as usize] += 1;
            }
        }
    }
    let mut by_count: Vec<u8> = (0..16u8)
        .filter(|&c| count[c as usize] > 0 && c != paper)
        .collect();
    by_count.sort_by_key(|&c| (std::cmp::Reverse(count[c as usize]), c));
    let Some(&ink) = by_count.first() else {
        return Vec::new();
    };
    let shade = by_count.get(1).copied().filter(|&shade| {
        let (mut below, mut total) = (0, 0);
        for y in y0.max(1)..y1 {
            for x in x0.max(1)..x1 {
                if sheet.get(x, y) == Some(shade) {
                    total += 1;
                    below += usize::from(sheet.get(x - 1, y - 1) == Some(ink));
                }
            }
        }
        total > 0 && below * 10 >= total * 6
    });
    let colors = Colors { paper, ink, shade };
    // Quadros com tinta da letra: neles, toda cor que nao e o fundo e do texto.
    let inked_tile = |tx: usize, ty: usize| {
        tx < SHEET_TILES
            && ty * SHEET_TILES + tx < sheet.tiles
            && sheet
                .tile_px(tx, ty)
                .iter()
                .any(|&p| colors.painted(Some(p)))
    };
    let marked = |x: usize, y: usize| {
        sheet.get(x, y).is_some_and(|p| p != colors.paper) && inked_tile(x / TILE, y / TILE)
    };
    lines_of(
        &|x, y| colors.painted(sheet.get(x, y)),
        &marked,
        x0,
        y0,
        x1,
        y1,
    )
    .into_iter()
    .filter(|line| !line.keyboard)
    .map(|line| (line, colors))
    .collect()
}

/// Fundo, letra e sombra de um trecho.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Colors {
    paper: u8,
    ink: u8,
    shade: Option<u8>,
}

impl Colors {
    fn painted(&self, p: Option<u8>) -> bool {
        p == Some(self.ink) || (self.shade.is_some() && p == self.shade)
    }
}

/// Os trechos feitos das linhas de letra de todos os grupos: linhas largas,
/// das mesmas cores, alinhadas a esquerda e logo uma depois da outra (o vao
/// pode ter o furigana da de baixo) viram um paragrafo; o resto (lista de
/// opcoes, rotulo) fica uma linha por trecho.
fn blocks_of(sheet: &Sheet, mut lines: Vec<(Line, Colors)>) -> Vec<Block> {
    lines.sort_by_key(|(line, _)| (line.y0, line.x0));
    let widest = lines.iter().map(|(l, _)| l.x1 - l.x0).max().unwrap_or(0);
    let mut groups: Vec<Vec<(Line, Colors)>> = Vec::new();
    for (line, colors) in lines {
        let open = groups.iter_mut().find(|group| {
            let (prev, prev_colors) = &group[group.len() - 1];
            let full = prev.x1 - prev.x0 >= (widest * 7 / 10).max(80);
            *prev_colors == colors
                && full
                && prev.x0.abs_diff(line.x0) <= 4
                && line.y0 >= prev.y1
                && line.y0 <= prev.y1 + 12
        });
        match open {
            Some(group) => group.push((line, colors)),
            None => groups.push(vec![(line, colors)]),
        }
    }
    let mut out = Vec::new();
    for group in groups {
        let colors = group[0].1;
        let bx0 = group.iter().map(|(l, _)| l.x0).min().unwrap_or(0);
        let bx1 = group.iter().map(|(l, _)| l.x1).max().unwrap_or(0);
        let wx0 = group.iter().map(|(l, _)| l.wide.0).min().unwrap_or(bx0);
        let wx1 = group.iter().map(|(l, _)| l.wide.1).max().unwrap_or(bx1);
        let (by0, by1) = (group[0].0.y0, group[group.len() - 1].0.y1);
        let (w, h) = (bx1 - bx0, by1 - by0);
        if w < 4 || h < 5 {
            continue;
        }
        let painted = |x: usize, y: usize| colors.painted(sheet.get(x, y));
        // Fundo livre a direita, na altura do trecho, ate o fim do quadro da
        // ultima letra (no maximo `SLACK` pixels): ate onde a traducao pode
        // ir. O jogo pode montar a tela copiando os quadros de cada item
        // (medido: na lista de cores da tela de ajustes, "Menta" saia cortado
        // no fim do ultimo quadro de "ミント").
        let limit = (bx1.div_ceil(TILE) * TILE).min(bx1 + SLACK);
        let mut right = bx1;
        while right < sheet.width
            && right < limit
            && (by0..by1).all(|y| sheet.get(right, y) == Some(colors.paper))
        {
            right += 1;
        }
        // O que se apaga: o trecho e, acima dele, as linhas sem a tinta da
        // letra (furigana, de outra cor), ate 8.
        let mut top = by0;
        while top > 0 && by0 - top < 8 && !(bx0..bx1).any(|x| painted(x, top - 1)) {
            top -= 1;
        }
        // Furigana da mesma cor da letra: uma faixa baixa (ate 6 linhas) logo
        // acima do trecho, com linha vazia em cima dela. No aviso de saude do
        // jogo medido sobravam pontinhos dele em cima do titulo traduzido.
        let blank = |y: usize| (wx0..wx1.max(bx1)).all(|x| sheet.get(x, y) == Some(colors.paper));
        let gap = (1..=2).find(|&k| top >= k && !blank(top - k));
        if let Some(gap) = gap {
            let band_end = top - gap;
            let mut band_top = band_end;
            while band_top > 0 && band_end - band_top < 6 && !blank(band_top - 1) {
                band_top -= 1;
            }
            if band_end - band_top < 6 && (band_top == 0 || blank(band_top - 1)) {
                top = band_top;
            }
        }
        // A tinta da letra logo a direita do trecho, na mesma faixa (a
        // pontuacao do fim, que ficou fora da linha), tambem se apaga: ate 2
        // quadros, parando num vao de 8 colunas ou em qualquer outro desenho
        // (o conjunto de quadros pode ter uma ilustracao colada no rotulo).
        let start = (wx1.max(bx1) + 1).min(sheet.width);
        let mut erase_right = start;
        let mut empty = 0;
        for x in start..(start + 2 * TILE).min(sheet.width) {
            let column: Vec<Option<u8>> = (top..by1 + 1).map(|y| sheet.get(x, y)).collect();
            let drawn = |c: &Option<u8>| c.is_some_and(|c| c != colors.paper);
            if !column.iter().any(drawn) {
                empty += 1;
                if empty >= 8 {
                    break;
                }
                continue;
            }
            if column.iter().any(|&c| drawn(&c) && !colors.painted(c)) {
                break;
            }
            empty = 0;
            erase_right = x + 1;
        }
        out.push(Block {
            x: bx0,
            y: by0,
            w,
            h,
            paper: colors.paper,
            ink: colors.ink,
            shade: colors.shade,
            room: right - bx0,
            area: [wx0.min(bx0).saturating_sub(1), top, erase_right, by1 + 1],
        });
    }
    out
}

/// Uma linha de tinta de um grupo de quadros, em pixels.
struct Line {
    /// Da primeira a ultima coluna com a tinta da letra: onde a traducao e
    /// escrita.
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    /// Da primeira a ultima coluna com qualquer cor que nao e fundo nos
    /// quadros de texto: o que se apaga.
    wide: (usize, usize),
    /// Letras soltas a intervalos largos: linha de teclado de nomes.
    keyboard: bool,
}

/// Pixels de fundo livre a direita do texto que a traducao pode ocupar.
const SLACK: usize = 8;

/// Vao entre duas colunas de uma lista na mesma linha ("ミント   メロン"): bem
/// mais largo que o espaco entre palavras.
const COLUMN_GAP: usize = 9;

/// As linhas de tinta do retangulo: faixas de linhas de pixel com tinta,
/// juntando as separadas por ate 2 linhas vazias (letra como "二" tem vao no
/// meio), e cada faixa partida nos vaos de `COLUMN_GAP` ou mais (lista em
/// colunas).
fn lines_of(
    painted: &impl Fn(usize, usize) -> bool,
    marked: &impl Fn(usize, usize) -> bool,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
) -> Vec<Line> {
    let inked: Vec<bool> = (y0..y1).map(|y| (x0..x1).any(|x| painted(x, y))).collect();
    let mut bands: Vec<(usize, usize)> = Vec::new();
    for (i, &has) in inked.iter().enumerate() {
        if !has {
            continue;
        }
        let y = y0 + i;
        match bands.last_mut() {
            Some(band) if y <= band.1 + 2 => band.1 = y + 1,
            _ => bands.push((y, y + 1)),
        }
    }
    bands
        .into_iter()
        .filter_map(|(top, bottom)| {
            // A largura da linha vai por toda cor que nao e fundo nos quadros
            // com tinta (`marked`): perna de letra de uma terceira cor ficava
            // fora e sobrava depois da traducao (medido: um pixel depois de
            // "Fim", resto do "り").
            let columns: Vec<bool> = (x0..x1)
                .map(|x| (top..bottom).any(|y| painted(x, y) || marked(x, y)))
                .collect();
            let first = columns.iter().position(|&c| c)?;
            let last = columns.iter().rposition(|&c| c)?;
            // Pedacos de tinta separados por vao: no teclado, cada letra e um
            // pedaco estreito e os vaos sao largos.
            let mut pieces: Vec<(usize, usize)> = Vec::new();
            for (i, &c) in columns.iter().enumerate().take(last + 1).skip(first) {
                if !c {
                    continue;
                }
                match pieces.last_mut() {
                    Some(piece) if i <= piece.1 + 2 => piece.1 = i + 1,
                    _ => pieces.push((i, i + 1)),
                }
            }
            let wide_gaps = pieces.windows(2).filter(|w| w[1].0 - w[0].1 >= 6).count();
            let narrow = pieces.iter().all(|p| p.1 - p.0 <= 14);
            let keyboard = pieces.len() >= 3 && narrow && wide_gaps * 10 >= (pieces.len() - 1) * 7;
            // Colunas: os pedacos separados por vao de lista.
            let mut columns_of_line: Vec<(usize, usize)> = Vec::new();
            for piece in &pieces {
                match columns_of_line.last_mut() {
                    Some(column) if piece.0 < column.1 + COLUMN_GAP || keyboard => {
                        column.1 = piece.1
                    }
                    _ => columns_of_line.push(*piece),
                }
            }
            let inked: Vec<bool> = (x0..x1)
                .map(|x| (top..bottom).any(|y| painted(x, y)))
                .collect();
            Some(
                columns_of_line
                    .into_iter()
                    .filter_map(|(a, b)| {
                        let first = (a..b).find(|&i| inked[i])?;
                        let last = (a..b).rev().find(|&i| inked[i])?;
                        Some(Line {
                            x0: x0 + first,
                            y0: top,
                            x1: x0 + last + 1,
                            y1: bottom,
                            wide: (x0 + a, x0 + b),
                            keyboard,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

/// A folha que vai pra IA ler: cada trecho ampliado (`ZOOM`), fundo branco e
/// letra preta, com o numero dele (de 1 em diante) a esquerda.
pub fn reading_sheet(sheet: &Sheet, blocks: &[Block]) -> Result<Vec<u8>> {
    const LABEL: usize = 36;
    const GAP: usize = 8;
    let crop = |b: &Block| (b.w + 2 * MARGIN, b.h + 2 * MARGIN);
    let width = LABEL + blocks.iter().map(|b| crop(b).0 * ZOOM).max().unwrap_or(1);
    let height: usize = blocks
        .iter()
        .map(|b| (crop(b).1 * ZOOM).max(2 * font::HEIGHT) + GAP)
        .sum::<usize>()
        .max(1);
    let mut gray = vec![255u8; width * height];
    let mut top = 0;
    for (i, block) in blocks.iter().enumerate() {
        // O numero, com a fonte do app em dobro.
        let mut x = 2;
        for c in (i + 1).to_string().chars() {
            let Some(glyph) = font::glyph(c) else {
                continue;
            };
            for gy in 0..font::HEIGHT {
                for gx in 0..glyph.width {
                    if glyph.ink(gx, gy) {
                        for (sx, sy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let (px, py) = (x + gx * 2 + sx, top + gy * 2 + sy);
                            if px < width && py < height {
                                gray[py * width + px] = 0;
                            }
                        }
                    }
                }
            }
            x += glyph.advance() * 2;
        }
        let (cw, ch) = crop(block);
        for cy in 0..ch {
            for cx in 0..cw {
                let (sx, sy) = (
                    (block.x + cx).checked_sub(MARGIN),
                    (block.y + cy).checked_sub(MARGIN),
                );
                // Letra preta; outra cor (moldura que cruza a linha, furigana)
                // cinza claro: preta, a moldura riscava a palavra e a IA
                // adivinhava.
                let value = match (sx, sy) {
                    (Some(sx), Some(sy)) => match sheet.get(sx, sy) {
                        Some(p) if p == block.paper => 255,
                        Some(p) if p == block.ink || Some(p) == block.shade => 0,
                        Some(_) => 200,
                        None => 255,
                    },
                    _ => 255,
                };
                for zy in 0..ZOOM {
                    for zx in 0..ZOOM {
                        let (px, py) = (LABEL + cx * ZOOM + zx, top + cy * ZOOM + zy);
                        if px < width && py < height {
                            gray[py * width + px] = value;
                        }
                    }
                }
            }
        }
        top += (ch * ZOOM).max(2 * font::HEIGHT) + GAP;
    }
    encode_png(width, height, &gray)
}

/// O pedido que vai junto da folha de `reading_sheet`.
pub const READ_PROMPT: &str = "The attached image has numbered strips. Each strip was cut \
from the graphics of a Japanese video game (white paper, black ink, enlarged). Read only \
the strips that show text (words or sentences in kana, kanji, letters or digits) and write \
the text exactly as it is drawn in that strip, character by character: do not guess a word \
from the context, do not translate, and do not complete a sentence that continues in another \
strip. Leave out strips that are not text, that show a \
single character, or that are part of a grid of characters used as a keyboard. Answer only \
with JSON whose keys are the strip numbers, for example {\"texts\": {\"3\": \"おまかせ\", \
\"7\": \"もどる\"}}.";

/// O texto de cada trecho na resposta da IA a `READ_PROMPT`, pelo numero do
/// trecho (de 1 em diante); o que a resposta nao traz fica vazio. Lista na
/// ordem (`{"texts": [...]}`) tambem vale.
pub fn parse_reading(answer: &str, count: usize) -> Result<Vec<String>> {
    let start = answer.find('{');
    let end = answer.rfind('}');
    let json = match (start, end) {
        (Some(start), Some(end)) if start < end => &answer[start..=end],
        _ => {
            return Err(CoreError::Provider(format!(
                "a leitura da imagem nao veio em JSON: {}",
                answer.chars().take(200).collect::<String>()
            )))
        }
    };
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Texts {
        // Pelo numero do trecho: a lista na ordem escorrega quando a resposta
        // pula um (medido: com 101 trechos, o texto ia pro trecho seguinte).
        Numbered(BTreeMap<String, String>),
        Ordered(Vec<String>),
    }
    #[derive(Deserialize)]
    struct Reading {
        texts: Texts,
    }
    let reading: Reading = serde_json::from_str(json).map_err(|e| {
        CoreError::Provider(format!(
            "a leitura da imagem veio num JSON fora do formato: {e}"
        ))
    })?;
    let mut texts = vec![String::new(); count];
    match reading.texts {
        Texts::Numbered(map) => {
            for (key, text) in map {
                let slot = key
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|i| texts.get_mut(i));
                if let Some(slot) = slot {
                    *slot = text;
                }
            }
        }
        Texts::Ordered(list) => {
            for (slot, text) in texts.iter_mut().zip(list) {
                *slot = text;
            }
        }
    }
    Ok(texts)
}

/// Texto que vale traduzir: tem kana ou kanji, mais de um caractere e nao e
/// linha de teclado de nomes (`keyboard_text`).
fn worth_translating(text: &str) -> bool {
    let japanese = text
        .chars()
        .filter(|&c| crate::sjis::is_kana(c) || crate::sjis::is_kanji(c))
        .count();
    japanese > 0 && text.trim().chars().count() > 1 && !keyboard_text(text)
}

/// Linha de teclado de nomes lida como texto: letras soltas separadas por
/// espaco ("さ し す せ そ"), ou 3 kana ou mais na ordem do silabario
/// ("さしすせそ", "がぎぐげご"), cada um logo depois do anterior na tabela
/// japonesa. Palavra de verdade quase nunca vem assim.
fn keyboard_text(text: &str) -> bool {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let single = tokens.iter().filter(|t| t.chars().count() == 1).count();
    if tokens.len() >= 3 && single * 2 >= tokens.len() {
        return true;
    }
    let codes: Vec<u16> = text
        .chars()
        .filter(|&c| crate::sjis::is_kana(c))
        .filter_map(crate::kuten::encode)
        .collect();
    codes.len() >= 3 && codes.windows(2).all(|w| w[1] > w[0] && w[1] - w[0] <= 3)
}

/// As frases de uma imagem lida pela IA: uma por trecho com japones.
pub fn entries_of(picture: &Picture, blocks: &[Block], texts: &[String]) -> Vec<TextEntry> {
    blocks
        .iter()
        .zip(texts)
        .enumerate()
        .filter(|(_, (_, text))| worth_translating(text))
        .map(|(i, (block, text))| TextEntry {
            id: format!("{}-{:02}", picture.id, i + 1),
            resource_path: None,
            offset: Some(picture.offset as u64),
            original_bytes: Vec::new(),
            // As linhas do desenho viram uma frase so: quem quebra e o app.
            source_text: text.split_whitespace().collect::<Vec<_>>().join(" "),
            translated_text: None,
            context: Some(format!(
                "texto desenhado numa imagem: o app apaga o japones e escreve a traducao no \
                 lugar, com a fonte dele (cabem umas {} letras)",
                chars_that_fit(block)
            )),
            max_bytes: None,
            encoding: TextEncoding::Image,
            status: TranslationStatus::Untranslated,
            metadata: serde_json::json!({ "image": { "picture": picture, "block": block } }),
        })
        .collect()
}

/// A imagem e o trecho de uma frase desenhada.
pub fn place_of(entry: &TextEntry) -> Option<(Picture, Block)> {
    let image = entry.metadata.get("image")?;
    Some((
        serde_json::from_value(image.get("picture")?.clone()).ok()?,
        serde_json::from_value(image.get("block")?.clone()).ok()?,
    ))
}

/// Linhas que cabem na altura do trecho, escritas com `with`.
fn lines_in(block: &Block, with: Font) -> usize {
    ((block.h + 2) / with.line()).max(1)
}

/// Quantas letras cabem no trecho com a fonte normal, contando pela largura
/// media de uma minuscula: o numero que vai pro prompt (a estreita fica de
/// reserva pro que passar um pouco).
pub fn chars_that_fit(block: &Block) -> usize {
    let average = font::glyph('a').map_or(6, font::Glyph::advance);
    lines_in(block, Font::Regular) * (block.room / average)
}

/// A fonte e as linhas da traducao no trecho: com a fonte normal se couber,
/// senao com a estreita. `Err` diz por que nao cabe nem nela.
pub fn layout(text: &str, block: &Block) -> std::result::Result<(Font, Vec<String>), String> {
    if let Some(c) = font::missing(text) {
        return Err(format!("a fonte do app nao tem o caractere {c:?}"));
    }
    match layout_with(Font::Regular, text, block) {
        Ok(lines) => Ok((Font::Regular, lines)),
        Err(why) => layout_with(Font::Narrow, text, block)
            .map(|lines| (Font::Narrow, lines))
            .map_err(|_| why),
    }
}

/// As linhas da traducao escrita com `with`: quebradas entre palavras na
/// largura `room`, ate as que a altura comporta.
fn layout_with(with: Font, text: &str, block: &Block) -> std::result::Result<Vec<String>, String> {
    let mut lines: Vec<String> = Vec::new();
    // Quebra de linha (de verdade ou escrita "\n" pela IA) e espaco: as linhas
    // sao do app.
    let text = text.replace("\\n", " ");
    for word in text.split_whitespace() {
        let candidate = match lines.last() {
            Some(line) => format!("{line} {word}"),
            None => word.to_string(),
        };
        if with.width(&candidate).unwrap_or(usize::MAX) <= block.room {
            match lines.last_mut() {
                Some(line) => *line = candidate,
                None => lines.push(candidate),
            }
        } else if with.width(word).unwrap_or(usize::MAX) <= block.room && !lines.is_empty() {
            lines.push(word.to_string());
        } else {
            return Err(format!(
                "a palavra \"{word}\" e mais larga que o desenho ({} pixels)",
                block.room
            ));
        }
    }
    if lines.len() > lines_in(block, with) {
        return Err(format!(
            "a traducao precisa de {} linhas e o desenho comporta {}",
            lines.len(),
            lines_in(block, with)
        ));
    }
    Ok(lines)
}

/// Apaga o japones do trecho e escreve `text` no lugar, com a fonte do app,
/// nas cores do original. Apagar e pintar com o fundo o que nao e fundo nos
/// quadros do trecho que parecem letra — a letra, a sombra e o furigana, de
/// outra cor; moldura e enfeite nao parecem letra e ficam. Escrever so pinta
/// pixel que esta com a cor do fundo: o desenho em volta fica intacto.
pub fn draw(sheet: &mut Sheet, block: &Block, text: &str) -> std::result::Result<(), String> {
    let (with, lines) = layout(text, block)?;
    let [x0, y0, x1, y1] = block.area;
    // Quadro de texto: parece letra ou tem tinta da letra (kanji com sombra
    // e furigana passa de 3 cores). So nele o que nao e fundo e apagado;
    // moldura e enfeite sem a tinta da letra ficam.
    let painted =
        |p: Option<u8>| p == Some(block.ink) || (block.shade.is_some() && p == block.shade);
    let text_tile: HashMap<(usize, usize), bool> = (y0 / TILE..y1.div_ceil(TILE))
        .flat_map(|ty| (x0 / TILE..x1.div_ceil(TILE)).map(move |tx| (tx, ty)))
        .filter(|&(tx, ty)| tx < SHEET_TILES && ty * SHEET_TILES + tx < sheet.tiles)
        .map(|(tx, ty)| {
            let px = sheet.tile_px(tx, ty);
            let inked = px.iter().any(|&p| painted(Some(p)));
            ((tx, ty), inked || lettered(&px))
        })
        .collect();
    // Perto da letra (ate 2 pixels de um pixel de tinta) tudo que nao e fundo
    // sai: sombra e contorno de outra cor, que o retangulo da tinta nao pega.
    // No retangulo do trecho (com a faixa do furigana acima), o que nao e
    // fundo nos quadros de texto.
    // Na vertical, so 1 pixel e dentro da faixa do trecho: em lista apertada
    // (2 pixels entre linhas) o pe da linha de cima saia junto.
    const NEAR: usize = 2;
    let (ex0, ey0) = (x0.saturating_sub(NEAR), block.y.saturating_sub(1).min(y0));
    let (ex1, ey1) = (x1 + NEAR, (block.y + block.h + 1).max(y1));
    let mut near = vec![false; (ex1 - ex0) * (ey1 - ey0)];
    for y in block.y..block.y + block.h {
        for x in block.x..block.x + block.w {
            if !painted(sheet.get(x, y)) {
                continue;
            }
            let rows = y.saturating_sub(1).max(ey0)..(y + 2).min(ey1).min(block.y + block.h + 1);
            for ny in rows {
                for nx in x.saturating_sub(NEAR).max(ex0)..(x + NEAR + 1).min(ex1) {
                    near[(ny - ey0) * (ex1 - ex0) + nx - ex0] = true;
                }
            }
        }
    }
    let boxed =
        |x: usize, y: usize| (x0..x1).contains(&x) && (block.y..block.y + block.h).contains(&y);
    // Na faixa acima do texto (o furigana), a fileira de quadros que tem
    // algum quadro de texto vale inteira: o quadro com 2 ou 3 pixels de
    // furigana nao parece letra, e os pixels dele ficavam (pontinhos em cima
    // do titulo traduzido do aviso de saude do jogo medido).
    let band_rows: HashSet<usize> = text_tile
        .iter()
        .filter(|&(&(_, ty), &text)| text && ty * TILE < block.y)
        .map(|(&(_, ty), _)| ty)
        .collect();
    for y in ey0..ey1 {
        for x in ex0..ex1 {
            let close = near[(y - ey0) * (ex1 - ex0) + x - ex0] || boxed(x, y);
            let furigana = (x0..x1).contains(&x)
                && (y0..y1).contains(&y)
                && (text_tile
                    .get(&(x / TILE, y / TILE))
                    .copied()
                    .unwrap_or(false)
                    || (y < block.y && band_rows.contains(&(y / TILE))));
            if (close || furigana) && sheet.get(x, y).is_some_and(|p| p != block.paper) {
                sheet.set(x, y, block.paper);
            }
        }
    }
    // As linhas centradas na altura do trecho, pela altura da maiuscula.
    let span = (lines.len() - 1) * with.line() + with.cap();
    let cap_top = block.y + block.h.saturating_sub(span) / 2;
    let paint = |sheet: &mut Sheet, color: u8, dx: usize, dy: usize| {
        for (n, line) in lines.iter().enumerate() {
            let top = (cap_top + n * with.line() + dy).saturating_sub(font::CAP_TOP);
            let mut x = block.x + dx;
            for c in line.chars() {
                let Some(glyph) = with.glyph(c) else {
                    continue;
                };
                for gy in 0..font::HEIGHT {
                    for gx in 0..glyph.width {
                        if glyph.ink(gx, gy) && sheet.get(x + gx, top + gy) == Some(block.paper) {
                            sheet.set(x + gx, top + gy, color);
                        }
                    }
                }
                x += glyph.advance();
            }
        }
    };
    if let Some(shade) = block.shade {
        paint(sheet, shade, 1, 1);
    }
    paint(sheet, block.ink, 0, 0);
    Ok(())
}

/// O que `apply` fez: imagens regravadas e frases que ficaram no original.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub pictures: usize,
    pub drawn: usize,
    /// (id da frase, por que ficou no original).
    pub left_out: Vec<(String, String)>,
}

/// Uma frase desenhada a escrever: a entry, o trecho e a traducao.
type Drawing<'a> = (&'a TextEntry, Block, &'a str);

/// Escreve em `out` as frases desenhadas de `entries` que tem traducao: cada
/// imagem e aberta, cada trecho e redesenhado, e a imagem volta pro lugar
/// dela — a comprimida, recomprimida, so se couber no espaco que ocupava.
pub fn apply(out: &mut [u8], entries: &[TextEntry]) -> Applied {
    let mut by_picture: BTreeMap<usize, (Picture, Vec<Drawing>)> = BTreeMap::new();
    let mut applied = Applied::default();
    for entry in entries.iter().filter(|e| e.encoding == TextEncoding::Image) {
        let Some(text) = entry.translated_text.as_deref() else {
            continue;
        };
        let Some((picture, block)) = place_of(entry) else {
            applied
                .left_out
                .push((entry.id.clone(), "sem a posicao do desenho".to_string()));
            continue;
        };
        by_picture
            .entry(picture.offset)
            .or_insert_with(|| (picture, Vec::new()))
            .1
            .push((entry, block, text));
    }
    for (picture, items) in by_picture.into_values() {
        let fail = |applied: &mut Applied, why: &str| {
            for (entry, _, _) in &items {
                applied.left_out.push((entry.id.clone(), why.to_string()));
            }
        };
        let Some(tiles) = tiles_of(out, &picture) else {
            fail(
                &mut applied,
                "a imagem nao esta mais onde estava no arquivo",
            );
            continue;
        };
        let mut sheet = sheet_of(&tiles, &picture);
        // A medida de agora do trecho na mesma posicao vale mais que a da
        // leitura: o app melhora a medida de uma versao pra outra e a frase
        // lida antes nao precisa voltar pra IA.
        let fresh = blocks(&sheet);
        let mut drawn = 0;
        for (entry, stored, text) in &items {
            let block = fresh
                .iter()
                .find(|b| b.x.abs_diff(stored.x) <= 2 && b.y.abs_diff(stored.y) <= 2)
                .unwrap_or(stored);
            match draw(&mut sheet, block, text) {
                Ok(()) => drawn += 1,
                Err(why) => applied.left_out.push((entry.id.clone(), why)),
            }
        }
        if drawn == 0 {
            continue;
        }
        let new_tiles = unarrange(&sheet.tiles(), picture.sprite, tiles.len() / TILE_BYTES);
        match picture.packed {
            None => {
                out[picture.offset..picture.offset + new_tiles.len()].copy_from_slice(&new_tiles)
            }
            // O bloco vai ate o proximo multiplo de 4: o resto e o preenchimento
            // dele (`lz77::each_block` anda assim).
            Some(packed_len) => match lz77::pack(&new_tiles) {
                Some(packed) if packed.len() <= packed_len.next_multiple_of(4) => {
                    let room = packed_len.next_multiple_of(4);
                    out[picture.offset..picture.offset + packed.len()].copy_from_slice(&packed);
                    out[picture.offset + packed.len()..picture.offset + room].fill(0);
                }
                _ => {
                    fail(
                        &mut applied,
                        "a imagem comprimida, redesenhada, ficou maior que a original",
                    );
                    continue;
                }
            },
        }
        applied.pictures += 1;
        applied.drawn += drawn;
    }
    applied
}

/// Ids e trechos das frases desenhadas de uma imagem, por imagem: o que a
/// tela mostra pra conferir.
pub fn by_picture(entries: &[TextEntry]) -> HashMap<String, Vec<&TextEntry>> {
    let mut out: HashMap<String, Vec<&TextEntry>> = HashMap::new();
    for entry in entries.iter().filter(|e| e.encoding == TextEncoding::Image) {
        if let Some((picture, _)) = place_of(entry) {
            out.entry(picture.id).or_default().push(entry);
        }
    }
    out
}

/// Pasta do projeto com as folhas das imagens.
pub const PICTURES_DIR: &str = "imagens";

/// As imagens com letra do arquivo do projeto. A folha de cada uma fica em
/// `<projeto>/imagens/<id>.png`: e por ela que quem usa ve quais tem japones.
pub fn list_pictures(project_dir: &Path) -> Result<Vec<Picture>> {
    let game = crate::project::load_project(project_dir)?;
    let data = crate::fileio::read_view(&game.source_path)?;
    let pictures = find(&data);
    let dir = project_dir.join(PICTURES_DIR);
    fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    for picture in &pictures {
        let path = dir.join(format!("{}.png", picture.id));
        if path.exists() {
            continue;
        }
        if let Some(tiles) = tiles_of(&data, picture) {
            let png = sheet_of(&tiles, picture).png()?;
            fs::write(&path, png).map_err(|e| CoreError::io(&path, e))?;
        }
    }
    Ok(pictures)
}

/// Atualiza no banco a medida dos trechos das frases desenhadas: cada imagem
/// e aberta de novo e o trecho na mesma posicao (ate 2 pixels) da a medida de
/// agora. Sem IA. Devolve quantas frases mudaram. A reinsercao chama antes de
/// validar: validacao e desenho usam a mesma medida.
pub fn refresh(project_dir: &Path) -> Result<usize> {
    let mut db = crate::db::ProjectDb::open(project_dir)?;
    let entries: Vec<TextEntry> = db
        .load_entries()?
        .into_iter()
        .filter(|e| e.encoding == TextEncoding::Image)
        .collect();
    if entries.is_empty() {
        return Ok(0);
    }
    let game = crate::project::load_project(project_dir)?;
    let data = crate::fileio::read_view(&game.source_path)?;
    let mut fresh: HashMap<usize, Vec<Block>> = HashMap::new();
    let mut changed = Vec::new();
    for entry in entries {
        let Some((picture, stored)) = place_of(&entry) else {
            continue;
        };
        let found = fresh.entry(picture.offset).or_insert_with(|| {
            tiles_of(&data, &picture)
                .map(|tiles| blocks(&sheet_of(&tiles, &picture)))
                .unwrap_or_default()
        });
        let Some(block) = found
            .iter()
            .find(|b| b.x.abs_diff(stored.x) <= 2 && b.y.abs_diff(stored.y) <= 2)
        else {
            continue;
        };
        if *block != stored {
            let mut updated = entry.clone();
            updated.metadata =
                serde_json::json!({ "image": { "picture": picture, "block": block } });
            changed.push(updated);
        }
    }
    db.upsert_entries(&changed)?;
    Ok(changed.len())
}

/// O que `read_pictures` fez.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadOutcome {
    /// Imagens lidas pela IA.
    pub pictures: usize,
    /// Trechos de letra achados nas imagens (os que a IA leu ou, na previa,
    /// leria).
    pub blocks: usize,
    /// Frases com japones que entraram no projeto.
    pub entries: usize,
    /// (id da imagem, por que nao foi lida).
    pub failed: Vec<(String, String)>,
}

/// Quem le o texto das imagens.
pub enum Reader<'a> {
    /// A IA do provider: uma chamada por imagem.
    Provider(&'a dyn crate::provider::TranslationProvider),
    /// Uma leitura pronta, no formato que `READ_PROMPT` pede, de quem leu a
    /// folha por fora (outro modelo, uma pessoa): nada vai pra IA.
    Answer(&'a str),
    /// Ninguem: so grava as folhas, a previa do que iria pra IA.
    Preview,
}

/// O texto das imagens `ids` do arquivo do projeto, lido por `reader` na folha
/// dos trechos de cada uma (`reading_sheet`), que tambem fica em
/// `<projeto>/imagens/<id>-leitura.png`. Cada trecho com japones vira uma
/// frase desenhada no banco, pronta pra traduzir.
pub async fn read_pictures(
    project_dir: &Path,
    reader: Reader<'_>,
    ids: &[String],
) -> Result<ReadOutcome> {
    let game = crate::project::load_project(project_dir)?;
    let data = crate::fileio::read_view(&game.source_path)?;
    let pictures = find(&data);
    let dir = project_dir.join(PICTURES_DIR);
    fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    let mut db = crate::db::ProjectDb::open(project_dir)?;
    let mut outcome = ReadOutcome::default();
    for id in ids {
        let Some(picture) = pictures.iter().find(|p| &p.id == id) else {
            outcome.failed.push((
                id.clone(),
                "nao ha imagem com esse id no arquivo".to_string(),
            ));
            continue;
        };
        let Some(tiles) = tiles_of(&data, picture) else {
            outcome
                .failed
                .push((id.clone(), "a imagem nao abriu".to_string()));
            continue;
        };
        let sheet = sheet_of(&tiles, picture);
        let found = blocks(&sheet);
        if found.is_empty() {
            outcome
                .failed
                .push((id.clone(), "nenhum trecho de letra na imagem".to_string()));
            continue;
        }
        let png = reading_sheet(&sheet, &found)?;
        let path = dir.join(format!("{id}-leitura.png"));
        fs::write(&path, &png).map_err(|e| CoreError::io(&path, e))?;
        outcome.blocks += found.len();
        let answer = match reader {
            // A previa: a folha fica gravada e nada vai pra IA.
            Reader::Preview => continue,
            Reader::Answer(answer) => answer.to_string(),
            Reader::Provider(provider) => match provider.read_image(&png, READ_PROMPT).await {
                Ok(answer) => answer,
                Err(e) => {
                    outcome.failed.push((id.clone(), e.to_string()));
                    continue;
                }
            },
        };
        let texts = match parse_reading(&answer, found.len()) {
            Ok(texts) => texts,
            Err(e) => {
                outcome.failed.push((id.clone(), e.to_string()));
                continue;
            }
        };
        let entries = entries_of(picture, &found, &texts);
        // Ler de novo troca as frases da imagem inteiras: o id e o numero do
        // trecho, e um trecho que mudou de numero herdaria a traducao de
        // outro (o banco guarda a traducao de quem tem o mesmo id). A memoria
        // de traducao devolve o que ja foi traduzido.
        let prefix = format!("{}-", picture.id);
        let gone: Vec<String> = db
            .load_entries()?
            .into_iter()
            .filter(|e| e.encoding == TextEncoding::Image && e.id.starts_with(&prefix))
            .map(|e| e.id)
            .collect();
        db.remove_entries(&gone)?;
        db.upsert_entries(&entries)?;
        outcome.pictures += 1;
        outcome.entries += entries.len();
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Folha de 256 quadros, fundo 1, com `lines` linhas de "letras" (riscos
    /// finos, cor 15, sombra 3 abaixo e a direita), uma a cada 2 linhas de
    /// quadros, nos quadros 2 a 7 de cada uma: tinta e sombra na densidade de
    /// uma letra de verdade.
    fn panel(lines: usize) -> Vec<u8> {
        drawn_panel(lines, |x, y, top| x % 5 == 0 || (y == top + 4 && x % 5 < 3))
    }

    /// `panel` com riscos sem padrao (letra de verdade comprime mal como
    /// esta): o bloco LZ77 dele nao sai pequeno so por repeticao.
    fn noisy_panel(lines: usize) -> Vec<u8> {
        drawn_panel(lines, |x, y, _| (x * 7919 + y * 104_729) % 13 < 3)
    }

    fn drawn_panel(lines: usize, stroke: impl Fn(usize, usize, usize) -> bool) -> Vec<u8> {
        let mut sheet = Sheet::new(&vec![0x11; 256 * TILE_BYTES]);
        for line in 0..lines {
            let top = (2 * line + 1) * TILE;
            for x in (2 * TILE + 2)..(8 * TILE - 2) {
                for y in (top + 2)..(top + TILE - 1) {
                    let stroke = stroke(x, y, top);
                    if stroke {
                        sheet.set(x, y, 15);
                        if sheet.get(x + 1, y + 1) == Some(1) {
                            sheet.set(x + 1, y + 1, 3);
                        }
                    }
                }
            }
        }
        sheet.tiles()
    }

    /// LZ77 tipo 0x10 so com literais e repeticao do byte anterior (copia a
    /// 1 de distancia), como um compressor simples de jogo.
    fn runs_only(data: &[u8]) -> Vec<u8> {
        let mut out = vec![
            0x10,
            data.len() as u8,
            (data.len() >> 8) as u8,
            (data.len() >> 16) as u8,
        ];
        let mut i = 0;
        while i < data.len() {
            let flag_at = out.len();
            out.push(0);
            for bit in (0..8).rev() {
                if i >= data.len() {
                    break;
                }
                let run = data[i..]
                    .iter()
                    .take(18)
                    .take_while(|&&b| i > 0 && b == data[i - 1])
                    .count();
                if run >= 3 {
                    out[flag_at] |= 1 << bit;
                    out.extend([((run - 3) << 4) as u8, 0]);
                    i += run;
                } else {
                    out.push(data[i]);
                    i += 1;
                }
            }
        }
        out.resize(out.len().next_multiple_of(4), 0);
        out
    }

    #[test]
    fn tiles_go_back_as_they_came() {
        let tiles: Vec<u8> = (0..64 * TILE_BYTES).map(|i| (i * 7 % 251) as u8).collect();
        assert_eq!(Sheet::new(&tiles).tiles(), tiles);
    }

    #[test]
    fn a_line_of_letters_becomes_one_block_with_its_colors() {
        let sheet = Sheet::new(&panel(1));
        let found = blocks(&sheet);
        assert_eq!(found.len(), 1, "{found:?}");
        let block = &found[0];
        assert_eq!((block.paper, block.ink, block.shade), (1, 15, Some(3)));
        // O primeiro risco: a primeira coluna multipla de 5 do trecho.
        assert_eq!((block.x, block.y), (20, TILE + 2));
        // Fundo livre a direita ate o fim do quadro da ultima letra.
        assert_eq!(
            block.room,
            (block.x + block.w).div_ceil(TILE) * TILE - block.x
        );
    }

    #[test]
    fn the_translation_replaces_the_letters_and_nothing_else() {
        let mut sheet = Sheet::new(&panel(1));
        let block = blocks(&sheet).remove(0);
        // Um enfeite fora do trecho, na cor da letra.
        sheet.set(0, 0, 15);
        draw(&mut sheet, &block, "Fim").unwrap();
        assert_eq!(sheet.get(0, 0), Some(15), "o resto do desenho fica");
        let inked: usize = (0..sheet.height)
            .flat_map(|y| (0..sheet.width).map(move |x| (x, y)))
            .filter(|&(x, y)| sheet.get(x, y) == Some(15))
            .count();
        assert!(inked > 10, "a traducao foi escrita");
        // Nada do risco original sobra fora do que a fonte pintou: a coluna
        // do fim do trecho ficou com o fundo.
        let last = block.x + block.w - 1;
        assert!((block.y..block.y + block.h).all(|y| sheet.get(last, y) != Some(15)));
        assert!(draw(
            &mut sheet,
            &block,
            "Uma frase longa demais pra caber neste desenho"
        )
        .is_err());
    }

    #[test]
    fn the_reading_of_the_ai_is_taken_strip_by_strip() {
        let texts = parse_reading("ok: {\"texts\": [\"おまかせ\", \"\"]}", 3).unwrap();
        assert_eq!(texts, ["おまかせ", "", ""]);
        // Pelo numero: o trecho que a resposta pula nao empurra os outros.
        let texts = parse_reading(
            "{\"texts\": {\"3\": \"もどる\", \"1\": \"かな\", \"9\": \"x\"}}",
            3,
        )
        .unwrap();
        assert_eq!(texts, ["かな", "", "もどる"]);
        assert!(parse_reading("nada", 1).is_err());
        assert!(
            worth_translating("おまかせ") && !worth_translating("あ") && !worth_translating("ABC")
        );
        // Linha de teclado: letras soltas ou silabario em ordem.
        assert!(!worth_translating("さ し す せ そ"));
        assert!(!worth_translating("がぎぐげご") && !worth_translating("アイウエオ"));
        assert!(worth_translating("もどる") && worth_translating("ふつう"));
    }

    #[test]
    fn text_kept_as_sprites_is_put_back_together() {
        // Uma linha de letras de 16 pixels de altura guardada em 8 sprites de
        // 4x2 quadros: cada sprite e os 4 quadros de cima e depois os 4 de
        // baixo, como o "aperte um botao" do aviso de saude do jogo medido.
        let mut screen = Sheet::new(&vec![0x11; 64 * TILE_BYTES]);
        for x in 4..252 {
            for y in 3..13 {
                if x % 6 == 0 || (y == 8 && x % 6 < 4) {
                    screen.set(x, y, 15);
                }
            }
        }
        let screen_tiles = screen.tiles();
        let mut stored = Vec::new();
        for sprite in 0..8 {
            for row in 0..2 {
                for col in 0..4 {
                    let t = row * SHEET_TILES + 4 * sprite + col;
                    stored.extend(&screen_tiles[t * TILE_BYTES..(t + 1) * TILE_BYTES]);
                }
            }
        }
        assert_eq!(arrangement(&stored), Some([4, 2]));
        assert_eq!(arrangement(&screen_tiles), None);
        assert_eq!(arrange(&stored, Some([4, 2])), screen_tiles);
        assert_eq!(unarrange(&screen_tiles, Some([4, 2]), 64), stored);
    }

    #[test]
    fn raw_tiles_start_where_most_tiles_are_one_color() {
        // Quadros de uma cor alternados com quadros desenhados, 12 bytes
        // depois do comeco do trecho.
        let mut data = vec![0x5A; 12];
        for k in 0..64u8 {
            if k % 2 == 0 {
                data.extend([0x11; TILE_BYTES]);
            } else {
                data.extend((0..TILE_BYTES as u8).map(|i| i.wrapping_mul(37) ^ k));
            }
        }
        assert_eq!(phase(&data), 12);
        assert_eq!(phase(&data[12..]), 0);
        assert_eq!(phase(&[]), 0);
    }

    #[test]
    fn a_compressed_picture_is_found_and_written_back_in_place() {
        // Seis linhas de letra: o bastante pra imagem valer (`worth`). O
        // bloco do "jogo" e de um compressor simples (so repeticao de byte):
        // o do app, regravando, faz menor.
        let tiles = noisy_panel(6);
        let packed = runs_only(&tiles);
        let mut data = vec![0xEE; 64];
        data.extend(&packed);
        data.resize(data.len().next_multiple_of(4) + 64, 0xEE);
        let found = find(&data);
        assert_eq!(found.len(), 1, "{found:?}");
        let picture = &found[0];
        let consumed = lz77::unpack(&packed).unwrap().packed_len;
        assert_eq!((picture.offset, picture.packed), (64, Some(consumed)));
        let block = blocks(&Sheet::new(&tiles_of(&data, picture).unwrap())).remove(0);
        let entry = TextEntry {
            translated_text: Some("Ok".to_string()),
            ..entries_of(picture, &[block], &["おわり".to_string()]).remove(0)
        };
        let before = data.clone();
        let applied = apply(&mut data, &[entry]);
        assert_eq!((applied.pictures, applied.drawn), (1, 1), "{applied:?}");
        assert_ne!(data, before);
        assert_eq!(&data[..64], &before[..64], "fora do bloco nada muda");
        assert!(
            tiles_of(&data, picture).is_some(),
            "o bloco continua valido"
        );
    }
}

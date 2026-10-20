//! Fonte NFTR do DS: a letra acentuada que a fonte nao desenha e desenhada
//! nela, com a letra base e o sinal da propria fonte.
//!
//! Medido em 19/10/2026 no Dragon Quest V (EUA, ingles/frances/espanhol, so
//! contagens): as duas fontes de letra ocidental mapeiam o Latin-1 inteiro,
//! mas o que nenhum dos cinco idiomas do cartucho usa (ã, Ã, å, ý...) aponta
//! pra um quadrado cheio, o mesmo desenho pra 76 caracteres. Na tela, "não"
//! saia "n▮o". O "õ" e o "ñ" existem: o til deles vai pro "a".
//!
//! Formato: "RTFN" + secoes FINF (as posicoes das outras tres), CGLP (os
//! desenhos, `bpp` bits por ponto, linha atras de linha, o bit mais alto
//! primeiro), CWDH (por desenho: recuo, largura e avanco) e CMAP
//! (caractere -> desenho, em tres formatos). So os bytes do desenho e da
//! largura da letra mudam: o arquivo fica do mesmo tamanho.

use std::collections::{HashMap, HashSet};

/// (letras, bases, sinal): cada letra acentuada do Latin-1 com a letra sem
/// acento e o sinal. Sinal 6 (cedilha) fica embaixo; os outros, em cima.
const ACCENTED: [(&str, &str, u8); 14] = [
    ("àèìòù", "aeiou", 0),
    ("ÀÈÌÒÙ", "AEIOU", 0),
    ("áéíóúý", "aeiouy", 1),
    ("ÁÉÍÓÚÝ", "AEIOUY", 1),
    ("âêîôû", "aeiou", 2),
    ("ÂÊÎÔÛ", "AEIOU", 2),
    ("ãñõ", "ano", 3),
    ("ÃÑÕ", "ANO", 3),
    ("äëïöüÿ", "aeiouy", 4),
    ("ÄËÏÖÜ", "AEIOU", 4),
    ("å", "a", 5),
    ("Å", "A", 5),
    ("ç", "c", 6),
    ("Ç", "C", 6),
];
const CEDILLA: u8 = 6;

/// (base, sinal) de uma letra acentuada.
fn split(c: char) -> Option<(char, u8)> {
    ACCENTED.iter().find_map(|(letters, bases, mark)| {
        let i = letters.chars().position(|l| l == c)?;
        Some((bases.chars().nth(i)?, *mark))
    })
}

fn u16_at(file: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(file.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(file: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(file.get(at..at + 4)?.try_into().ok()?) as usize)
}

struct Font {
    /// Onde comeca o primeiro desenho.
    data: usize,
    width: usize,
    height: usize,
    tile: usize,
    bpp: usize,
    glyphs: usize,
    /// Primeiro desenho com largura, posicao da primeira largura e o ultimo.
    widths: (usize, usize, usize),
    map: HashMap<char, usize>,
}

impl Font {
    fn parse(file: &[u8]) -> Option<Font> {
        if file.get(..4)? != b"RTFN" {
            return None;
        }
        let info = u16_at(file, 12)?;
        if file.get(info..info + 4)? != b"FNIF" {
            return None;
        }
        let section = |key: usize, magic: &[u8]| {
            let at = u32_at(file, info + key)?.checked_sub(8)?;
            (file.get(at..at + 4)? == magic).then_some(at)
        };
        let glyphs = section(16, b"PLGC")?;
        let widths = section(20, b"HDWC")?;
        let (width, height) = (
            *file.get(glyphs + 8)? as usize,
            *file.get(glyphs + 9)? as usize,
        );
        let tile = u16_at(file, glyphs + 10)?;
        let bpp = *file.get(glyphs + 14)? as usize;
        if !matches!(bpp, 1 | 2 | 4 | 8) || tile * 8 < width * height * bpp {
            return None;
        }
        let mut map = HashMap::new();
        let mut next = u32_at(file, info + 24)?;
        let mut seen = HashSet::new();
        while next != 0 && seen.insert(next) {
            let at = next.checked_sub(8)?;
            if file.get(at..at + 4)? != b"PAMC" {
                return None;
            }
            let (first, last) = (u16_at(file, at + 8)?, u16_at(file, at + 10)?);
            let kind = u32_at(file, at + 12)?;
            next = u32_at(file, at + 16)?;
            let body = at + 20;
            let mut put = |code: usize, glyph: usize| {
                if let Some(c) = char::from_u32(code as u32) {
                    map.insert(c, glyph);
                }
            };
            match kind {
                0 => {
                    let start = u16_at(file, body)?;
                    for code in first..=last {
                        put(code, start + code - first);
                    }
                }
                1 => {
                    for (i, code) in (first..=last).enumerate() {
                        match u16_at(file, body + 2 * i)? {
                            0xFFFF => {}
                            glyph => put(code, glyph),
                        }
                    }
                }
                _ => {
                    for i in 0..u16_at(file, body)? {
                        let at = body + 2 + 4 * i;
                        put(u16_at(file, at)?, u16_at(file, at + 2)?);
                    }
                }
            }
        }
        let count = u32_at(file, glyphs + 4)?.checked_sub(16)? / tile.max(1);
        if glyphs + 16 + count * tile > file.len() {
            return None;
        }
        Some(Font {
            data: glyphs + 16,
            width,
            height,
            tile,
            bpp,
            glyphs: count,
            widths: (
                u16_at(file, widths + 8)?,
                widths + 16,
                u16_at(file, widths + 10)?,
            ),
            map,
        })
        .filter(|font| font.map.values().all(|&g| g < font.glyphs))
    }

    fn pixels(&self, file: &[u8], glyph: usize) -> Vec<u8> {
        let at = self.data + glyph * self.tile;
        let mask = (1u16 << self.bpp) as u8 - 1;
        (0..self.width * self.height)
            .map(|i| {
                let bit = i * self.bpp;
                (file[at + bit / 8] >> (8 - self.bpp - bit % 8)) & mask
            })
            .collect()
    }

    fn store(&self, file: &mut [u8], glyph: usize, pixels: &[u8]) {
        let at = self.data + glyph * self.tile;
        file[at..at + self.tile].fill(0);
        for (i, &v) in pixels.iter().enumerate() {
            let bit = i * self.bpp;
            file[at + bit / 8] |= v << (8 - self.bpp - bit % 8);
        }
    }

    /// Posicao das 3 larguras (recuo, largura, avanco) do desenho.
    fn width_at(&self, glyph: usize) -> Option<usize> {
        let (first, at, last) = self.widths;
        (first..=last)
            .contains(&glyph)
            .then(|| at + 3 * (glyph - first))
    }
}

/// (topo, baixo, esquerda, direita) da tinta, inclusive.
fn ink(pixels: &[u8], width: usize, rows: std::ops::Range<usize>) -> Option<[usize; 4]> {
    let mut out: Option<[usize; 4]> = None;
    for y in rows {
        for x in 0..width {
            if pixels[y * width + x] != 0 {
                let b = out.get_or_insert([y, y, x, x]);
                *b = [b[0].min(y), b[1].max(y), b[2].min(x), b[3].max(x)];
            }
        }
    }
    out
}

/// Desenha em `file` (uma fonte NFTR) as letras de `wanted` que ela mapeia pra
/// um desenho de reserva — o mesmo de outra letra acentuada —, com a letra
/// base e o sinal tirado de uma letra que o tem (o til do "ñ" pro "ã").
/// Devolve quantas desenhou; arquivo que nao e NFTR fica como esta.
pub(crate) fn fill(file: &mut [u8], wanted: &HashSet<char>) -> usize {
    let Some(font) = Font::parse(file) else {
        return 0;
    };
    let mut by_drawing: HashMap<Vec<u8>, usize> = HashMap::new();
    for (&c, &glyph) in &font.map {
        if split(c).is_some() {
            *by_drawing.entry(font.pixels(file, glyph)).or_default() += 1;
        }
    }
    let usable = |file: &[u8], c: char| {
        let glyph = *font.map.get(&c)?;
        let pixels = font.pixels(file, glyph);
        (by_drawing.get(&pixels).copied().unwrap_or(0) < 2).then_some((glyph, pixels))
    };
    let mut drawn = 0;
    let mut targets: Vec<char> = wanted.iter().copied().collect();
    targets.sort_unstable();
    for c in targets {
        let (Some((base, mark)), Some(&glyph)) = (split(c), font.map.get(&c)) else {
            continue;
        };
        if usable(file, c).is_some() {
            continue;
        }
        let Some((base_glyph, base_pixels)) = usable(file, base) else {
            continue;
        };
        // O sinal: o que a outra letra com ele tem fora das linhas da base dela.
        let donor = ACCENTED
            .iter()
            .filter(|(_, bases, m)| {
                *m == mark
                    && bases.chars().next().map(char::is_uppercase) == Some(base.is_uppercase())
            })
            .flat_map(|(letters, bases, _)| letters.chars().zip(bases.chars()))
            .filter(|&(letter, _)| letter != c)
            .find_map(|(letter, donor_base)| {
                Some((usable(file, letter)?.1, usable(file, donor_base)?.1))
            });
        let Some((donor, donor_base)) = donor else {
            continue;
        };
        let Some(composed) = compose(&font, &base_pixels, &donor, &donor_base, mark) else {
            continue;
        };
        let (pixels, right) = composed;
        // Larguras: as da base, com a sobra que o sinal pedir.
        let (Some(to), Some(from)) = (font.width_at(glyph), font.width_at(base_glyph)) else {
            continue;
        };
        let Some(mut sizes) = file
            .get(from..from + 3)
            .and_then(|s| <[u8; 3]>::try_from(s).ok())
            .filter(|_| to + 3 <= file.len())
        else {
            continue;
        };
        let extra = (right + 1).saturating_sub(sizes[1] as usize) as u8;
        sizes[1] += extra;
        sizes[2] += extra;
        font.store(file, glyph, &pixels);
        file[to..to + 3].copy_from_slice(&sizes);
        drawn += 1;
    }
    drawn
}

/// A base com o sinal do doador, centrado nela e na mesma distancia do corpo.
/// Devolve os pontos e a coluna mais a direita com tinta.
fn compose(
    font: &Font,
    base: &[u8],
    donor: &[u8],
    donor_base: &[u8],
    mark: u8,
) -> Option<(Vec<u8>, usize)> {
    let (w, h) = (font.width, font.height);
    let body = ink(donor_base, w, 0..h)?;
    let mark_rows = match mark {
        CEDILLA => body[1] + 1..h,
        _ => 0..body[0],
    };
    let sign = ink(donor, w, mark_rows.clone())?;
    let donor_body = ink(donor, w, body[0]..body[1] + 1)?;
    let target = ink(base, w, 0..h)?;
    // Colunas: o centro do sinal fica onde estava em relacao ao corpo.
    let shift = (target[2] + target[3]) as i64 - (donor_body[2] + donor_body[3]) as i64;
    let dx = shift.div_euclid(2).max(-(sign[2] as i64));
    // Linhas: a mesma distancia da borda do corpo.
    let dy = match mark {
        CEDILLA => target[1] as i64 - body[1] as i64,
        _ => target[0] as i64 - body[0] as i64,
    };
    let mut out = base.to_vec();
    let mut right = target[3];
    for y in mark_rows {
        for x in 0..w {
            let v = donor[y * w + x];
            if v == 0 {
                continue;
            }
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            if nx < 0 || ny < 0 || nx as usize >= w || ny as usize >= h {
                return None;
            }
            out[ny as usize * w + nx as usize] = v;
            right = right.max(nx as usize);
        }
    }
    Some((out, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fonte de 6x6, 1 bit por ponto, com a, n, ñ e ã (este no desenho de
    /// reserva, igual ao do Å).
    fn font() -> Vec<u8> {
        let draw = |rows: [&str; 6]| -> Vec<u8> {
            let bits: Vec<u8> = rows
                .iter()
                .flat_map(|r| r.bytes().map(|b| u8::from(b == b'#')))
                .collect();
            let mut out = vec![0u8; 5];
            for (i, b) in bits.iter().enumerate() {
                out[i / 8] |= b << (7 - i % 8);
            }
            out
        };
        let glyphs = [
            draw(["      ", "      ", " ###  ", "#  #  ", "#  #  ", " ###  "]),
            draw(["      ", "      ", "###   ", "#  #  ", "#  #  ", "#  #  "]),
            draw([" ##   ", "      ", "###   ", "#  #  ", "#  #  ", "#  #  "]),
            draw(["######"; 6]),
            draw(["######"; 6]),
        ];
        let chars = ['a', 'n', 'ñ', 'ã', 'Å'];
        let mut f = b"RTFN\xFF\xFE\x00\x01".to_vec();
        f.extend_from_slice(&[0; 4]);
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(&4u16.to_le_bytes());
        let finf = f.len();
        f.extend_from_slice(b"FNIF");
        f.extend_from_slice(&28u32.to_le_bytes());
        f.extend_from_slice(&[0; 20]);
        let cglp = f.len();
        f.extend_from_slice(b"PLGC");
        f.extend_from_slice(&((16 + 5 * glyphs.len()) as u32).to_le_bytes());
        f.extend_from_slice(&[6, 6, 5, 0, 0, 0, 1, 0]);
        glyphs.iter().for_each(|g| f.extend_from_slice(g));
        let cwdh = f.len();
        f.extend_from_slice(b"HDWC");
        f.extend_from_slice(&((16 + 3 * glyphs.len()) as u32).to_le_bytes());
        f.extend_from_slice(&0u16.to_le_bytes());
        f.extend_from_slice(&((glyphs.len() - 1) as u16).to_le_bytes());
        f.extend_from_slice(&[0; 4]);
        for w in [4u8, 4, 4, 6, 6] {
            f.extend_from_slice(&[0, w, w + 1]);
        }
        let cmap = f.len();
        f.extend_from_slice(b"PAMC");
        f.extend_from_slice(&((22 + 4 * chars.len()) as u32).to_le_bytes());
        f.extend_from_slice(&0u16.to_le_bytes());
        f.extend_from_slice(&0xFFFFu16.to_le_bytes());
        f.extend_from_slice(&2u32.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&(chars.len() as u16).to_le_bytes());
        for (i, c) in chars.iter().enumerate() {
            f.extend_from_slice(&(*c as u16).to_le_bytes());
            f.extend_from_slice(&(i as u16).to_le_bytes());
        }
        for (key, at) in [(16, cglp), (20, cwdh), (24, cmap)] {
            f[finf + key..finf + key + 4].copy_from_slice(&((at + 8) as u32).to_le_bytes());
        }
        f
    }

    #[test]
    fn a_placeholder_letter_gets_the_base_and_the_mark_of_the_font() {
        let mut file = font();
        let before = file.clone();
        let wanted: HashSet<char> = "ãa".chars().collect();
        assert_eq!(fill(&mut file, &wanted), 1);
        assert_eq!(file.len(), before.len());
        let f = Font::parse(&file).unwrap();
        let rows: Vec<String> = f
            .pixels(&file, f.map[&'ã'])
            .chunks(6)
            .map(|r| r.iter().map(|&v| if v == 1 { '#' } else { ' ' }).collect())
            .collect();
        // O til do ñ (colunas 1-2 sobre um corpo de 0-3) vai pro meio do a
        // (corpo de 0-3), duas linhas acima do corpo, como no ñ.
        assert_eq!(
            rows,
            [" ##   ", "      ", " ###  ", "#  #  ", "#  #  ", " ###  "]
        );
        // Largura e avanco da base.
        let at = f.width_at(f.map[&'ã']).unwrap();
        assert_eq!(&file[at..at + 3], &[0, 4, 5]);
        // Ja desenhada, nao muda de novo; arquivo que nao e fonte, nada.
        assert_eq!(fill(&mut file, &wanted), 0);
        assert_eq!(fill(&mut [0u8; 64], &wanted), 0);
        assert_eq!(fill(&mut file[..40].to_vec(), &wanted), 0);
    }
}

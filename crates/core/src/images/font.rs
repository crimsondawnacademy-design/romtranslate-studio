//! Fonte de pixel do app pra escrever a traducao numa imagem: desenho
//! proprio, 5 colunas, 7 linhas de maiuscula e 2 de descendente, com acento e
//! cedilha compostos sobre a letra base. A largura de cada letra e a das
//! colunas pintadas (`Glyph::advance`): "i" e "l" ocupam menos que "m".

use std::collections::HashMap;
use std::sync::OnceLock;

/// Linhas de uma letra: 2 de acento de maiuscula, 7 da maiuscula, 2 de
/// descendente.
pub const HEIGHT: usize = 11;
/// Colunas do desenho de uma letra, antes de cortar as vazias.
pub const WIDTH: usize = 5;
/// Avanco do espaco entre palavras.
pub const SPACE: usize = 3;
/// Linha da fonte onde comeca a maiuscula.
pub const CAP_TOP: usize = 2;
/// Altura da maiuscula.
pub const CAP: usize = 7;

/// Letra base: linhas da maiuscula (e da descendente), de cima pra baixo.
const BASE: &[(char, &str)] = &[
    (' ', "...../...../...../...../...../...../....."),
    ('!', "..#../..#../..#../..#../..#../...../..#.."),
    ('"', ".#.#./.#.#./...../...../...../...../....."),
    ('#', ".#.#./.#.#./#####/.#.#./#####/.#.#./.#.#."),
    ('$', "..#../.####/#.#../.###./..#.#/####./..#.."),
    ('%', "##..#/##.#./...#./..#../.#.../.#.##/#..##"),
    ('&', ".##../#..#./#.#../.#.../#.#.#/#..#./.##.#"),
    ('\'', "..#../..#../...../...../...../...../....."),
    ('(', "...#./..#../.#.../.#.../.#.../..#../...#."),
    (')', ".#.../..#../...#./...#./...#./..#../.#..."),
    ('*', "...../..#../#.#.#/.###./#.#.#/..#../....."),
    ('+', "...../..#../..#../#####/..#../..#../....."),
    (',', "...../...../...../...../...../..#../..#../.#..."),
    ('-', "...../...../...../.###./...../...../....."),
    ('.', "...../...../...../...../...../...../..#.."),
    ('/', "....#/....#/...#./..#../.#.../#..../#...."),
    ('0', ".###./#...#/#..##/#.#.#/##..#/#...#/.###."),
    ('1', "..#../.##../..#../..#../..#../..#../.###."),
    ('2', ".###./#...#/....#/...#./..#../.#.../#####"),
    ('3', "#####/...#./..#../...#./....#/#...#/.###."),
    ('4', "...#./..##./.#.#./#..#./#####/...#./...#."),
    ('5', "#####/#..../####./....#/....#/#...#/.###."),
    ('6', "..##./.#.../#..../####./#...#/#...#/.###."),
    ('7', "#####/....#/...#./..#../.#.../.#.../.#..."),
    ('8', ".###./#...#/#...#/.###./#...#/#...#/.###."),
    ('9', ".###./#...#/#...#/.####/....#/...#./.##.."),
    (':', "...../...../..#../...../...../..#../....."),
    (';', "...../...../..#../...../...../..#../..#../.#..."),
    ('<', "...#./..#../.#.../#..../.#.../..#../...#."),
    ('=', "...../...../#####/...../#####/...../....."),
    ('>', ".#.../..#../...#./....#/...#./..#../.#..."),
    ('?', ".###./#...#/....#/...#./..#../...../..#.."),
    ('@', ".###./#...#/....#/.##.#/#.#.#/#.#.#/.###."),
    ('A', ".###./#...#/#...#/#####/#...#/#...#/#...#"),
    ('B', "####./#...#/#...#/####./#...#/#...#/####."),
    ('C', ".###./#...#/#..../#..../#..../#...#/.###."),
    ('D', "###../#..#./#...#/#...#/#...#/#..#./###.."),
    ('E', "#####/#..../#..../####./#..../#..../#####"),
    ('F', "#####/#..../#..../####./#..../#..../#...."),
    ('G', ".###./#...#/#..../#.###/#...#/#...#/.####"),
    ('H', "#...#/#...#/#...#/#####/#...#/#...#/#...#"),
    ('I', ".###./..#../..#../..#../..#../..#../.###."),
    ('J', "..###/...#./...#./...#./...#./#..#./.##.."),
    ('K', "#...#/#..#./#.#../##.../#.#../#..#./#...#"),
    ('L', "#..../#..../#..../#..../#..../#..../#####"),
    ('M', "#...#/##.##/#.#.#/#.#.#/#...#/#...#/#...#"),
    ('N', "#...#/#...#/##..#/#.#.#/#..##/#...#/#...#"),
    ('O', ".###./#...#/#...#/#...#/#...#/#...#/.###."),
    ('P', "####./#...#/#...#/####./#..../#..../#...."),
    ('Q', ".###./#...#/#...#/#...#/#.#.#/#..#./.##.#"),
    ('R', "####./#...#/#...#/####./#.#../#..#./#...#"),
    ('S', ".####/#..../#..../.###./....#/....#/####."),
    ('T', "#####/..#../..#../..#../..#../..#../..#.."),
    ('U', "#...#/#...#/#...#/#...#/#...#/#...#/.###."),
    ('V', "#...#/#...#/#...#/#...#/#...#/.#.#./..#.."),
    ('W', "#...#/#...#/#...#/#.#.#/#.#.#/#.#.#/.#.#."),
    ('X', "#...#/#...#/.#.#./..#../.#.#./#...#/#...#"),
    ('Y', "#...#/#...#/.#.#./..#../..#../..#../..#.."),
    ('Z', "#####/....#/...#./..#../.#.../#..../#####"),
    ('[', ".###./.#.../.#.../.#.../.#.../.#.../.###."),
    ('\\', "#..../#..../.#.../..#../...#./....#/....#"),
    (']', ".###./...#./...#./...#./...#./...#./.###."),
    ('^', "..#../.#.#./#...#/...../...../...../....."),
    ('_', "...../...../...../...../...../...../#####"),
    ('`', ".#.../..#../...../...../...../...../....."),
    ('a', "...../...../.###./....#/.####/#...#/.####"),
    ('b', "#..../#..../####./#...#/#...#/#...#/####."),
    ('c', "...../...../.###./#..../#..../#...#/.###."),
    ('d', "....#/....#/.####/#...#/#...#/#...#/.####"),
    ('e', "...../...../.###./#...#/#####/#..../.###."),
    ('f', "..##./.#..#/.#.../###../.#.../.#.../.#..."),
    ('g', "...../...../.####/#...#/#...#/.####/....#/.###."),
    ('h', "#..../#..../####./#...#/#...#/#...#/#...#"),
    ('i', "..#../...../.##../..#../..#../..#../.###."),
    ('j', "...#./...../..##./...#./...#./...#./#..#./.##.."),
    ('k', "#..../#..../#..#./#.#../##.../#.#../#..#."),
    ('l', ".##../..#../..#../..#../..#../..#../.###."),
    ('m', "...../...../##.#./#.#.#/#.#.#/#.#.#/#.#.#"),
    ('n', "...../...../####./#...#/#...#/#...#/#...#"),
    ('o', "...../...../.###./#...#/#...#/#...#/.###."),
    ('p', "...../...../####./#...#/#...#/####./#..../#...."),
    ('q', "...../...../.####/#...#/#...#/.####/....#/....#"),
    ('r', "...../...../#.##./##..#/#..../#..../#...."),
    ('s', "...../...../.####/#..../.###./....#/####."),
    ('t', ".#.../.#.../###../.#.../.#.../.#..#/..##."),
    ('u', "...../...../#...#/#...#/#...#/#..##/.##.#"),
    ('v', "...../...../#...#/#...#/#...#/.#.#./..#.."),
    ('w', "...../...../#...#/#...#/#.#.#/#.#.#/.#.#."),
    ('x', "...../...../#...#/.#.#./..#../.#.#./#...#"),
    ('y', "...../...../#...#/#...#/#...#/.####/....#/.###."),
    ('z', "...../...../#####/...#./..#../.#.../#####"),
    ('{', "...#./..#../..#../.#.../..#../..#../...#."),
    ('|', "..#../..#../..#../..#../..#../..#../..#.."),
    ('}', ".#.../..#../..#../...#./..#../..#../.#..."),
    ('~', "...../...../.#.../#.#.#/...#./...../....."),
];

/// Acentos, nas 2 linhas logo acima da letra (de maiuscula ou minuscula).
const ACCENTS: &[(&str, &str)] = &[
    ("acute", "...#./..#.."),
    ("grave", ".#.../..#.."),
    ("circ", "..#../.#.#."),
    ("tilde", ".##.#/#..#."),
    ("diaer", ".#.#./....."),
];

/// Letra acentuada = letra base + acento.
const COMPOSED: &[(char, char, &str)] = &[
    ('á', 'a', "acute"),
    ('à', 'a', "grave"),
    ('â', 'a', "circ"),
    ('ã', 'a', "tilde"),
    ('ä', 'a', "diaer"),
    ('é', 'e', "acute"),
    ('è', 'e', "grave"),
    ('ê', 'e', "circ"),
    ('ë', 'e', "diaer"),
    ('í', 'i', "acute"),
    ('ì', 'i', "grave"),
    ('î', 'i', "circ"),
    ('ï', 'i', "diaer"),
    ('ó', 'o', "acute"),
    ('ò', 'o', "grave"),
    ('ô', 'o', "circ"),
    ('õ', 'o', "tilde"),
    ('ö', 'o', "diaer"),
    ('ú', 'u', "acute"),
    ('ù', 'u', "grave"),
    ('û', 'u', "circ"),
    ('ü', 'u', "diaer"),
    ('ñ', 'n', "tilde"),
    ('Á', 'A', "acute"),
    ('À', 'A', "grave"),
    ('Â', 'A', "circ"),
    ('Ã', 'A', "tilde"),
    ('Ä', 'A', "diaer"),
    ('É', 'E', "acute"),
    ('È', 'E', "grave"),
    ('Ê', 'E', "circ"),
    ('Í', 'I', "acute"),
    ('Ó', 'O', "acute"),
    ('Ô', 'O', "circ"),
    ('Õ', 'O', "tilde"),
    ('Ú', 'U', "acute"),
    ('Ü', 'U', "diaer"),
    ('Ñ', 'N', "tilde"),
];

/// Letras com cedilha (a base ganha a cedilha na descendente).
const CEDILLA: &[(char, char)] = &[('ç', 'c'), ('Ç', 'C')];
/// A cedilha, nas 2 linhas logo abaixo da letra.
const CEDILLA_ART: &str = "..#../.##..";

// ---- Fonte estreita ----

/// Fonte estreita, pra espaco apertado: 3 colunas e 5 linhas, so maiuscula (a
/// minuscula sai como versalete), com acento e cedilha. Medido em 14/10/2026:
/// "カナ" deixa 16 pixels, e "Kata" so cabe nela.
const NARROW: &[(char, &str)] = &[
    ('A', ".#./#.#/###/#.#/#.#"),
    ('B', "##./#.#/##./#.#/##."),
    ('C', ".##/#../#../#../.##"),
    ('D', "##./#.#/#.#/#.#/##."),
    ('E', "###/#../##./#../###"),
    ('F', "###/#../##./#../#.."),
    ('G', ".##/#../#.#/#.#/.##"),
    ('H', "#.#/#.#/###/#.#/#.#"),
    ('I', "###/.#./.#./.#./###"),
    ('J', "..#/..#/..#/#.#/.#."),
    ('K', "#.#/#.#/##./#.#/#.#"),
    ('L', "#../#../#../#../###"),
    ('M', "#.#/###/###/#.#/#.#"),
    ('N', "##./#.#/#.#/#.#/#.#"),
    ('O', ".#./#.#/#.#/#.#/.#."),
    ('P', "##./#.#/##./#../#.."),
    ('Q', ".#./#.#/#.#/##./.##"),
    ('R', "##./#.#/##./#.#/#.#"),
    ('S', ".##/#../.#./..#/##."),
    ('T', "###/.#./.#./.#./.#."),
    ('U', "#.#/#.#/#.#/#.#/.##"),
    ('V', "#.#/#.#/#.#/#.#/.#."),
    ('W', "#.#/#.#/###/###/#.#"),
    ('X', "#.#/#.#/.#./#.#/#.#"),
    ('Y', "#.#/#.#/.#./.#./.#."),
    ('Z', "###/..#/.#./#../###"),
    ('0', ".#./#.#/#.#/#.#/.#."),
    ('1', ".#./##./.#./.#./###"),
    ('2', "##./..#/.#./#../###"),
    ('3', "##./..#/.#./..#/##."),
    ('4', "#.#/#.#/###/..#/..#"),
    ('5', "###/#../##./..#/##."),
    ('6', ".##/#../##./#.#/.#."),
    ('7', "###/..#/.#./.#./.#."),
    ('8', ".#./#.#/.#./#.#/.#."),
    ('9', ".#./#.#/.##/..#/##."),
    ('.', "./././././#"),
    (',', "./././././#"),
    ('!', "#/#/#/./#"),
    ('?', "##./..#/.#./.../.#."),
    ('-', ".../.../###/.../..."),
    (':', "./#/./#/."),
    (';', "./#/./#/#"),
    ('\'', "#/#/./././"),
    ('"', "#.#/#.#/.../.../..."),
    ('(', ".#/#./#./#./.#"),
    (')', "#./.#/.#/.#/#."),
    ('/', "..#/..#/.#./#../#.."),
    ('+', ".../.#./###/.#./..."),
    ('&', ".#./#.#/.#./#.#/.##"),
    ('%', "#.#/..#/.#./#../#.#"),
    ('*', ".../#.#/.#./#.#/..."),
    ('=', ".../###/.../###/..."),
    (' ', "./././././."),
];

/// Acentos da fonte estreita, nas 2 linhas de cima.
const NARROW_ACCENTS: &[(&str, &str)] = &[
    ("acute", "..#/.#."),
    ("grave", "#../.#."),
    ("circ", ".#./#.#"),
    ("tilde", "##./.##"),
    ("diaer", "#.#/..."),
];

/// Letras acentuadas da fonte estreita: base + acento.
const NARROW_COMPOSED: &[(char, char, &str)] = &[
    ('Á', 'A', "acute"),
    ('À', 'A', "grave"),
    ('Â', 'A', "circ"),
    ('Ã', 'A', "tilde"),
    ('É', 'E', "acute"),
    ('Ê', 'E', "circ"),
    ('Í', 'I', "acute"),
    ('Ó', 'O', "acute"),
    ('Ô', 'O', "circ"),
    ('Õ', 'O', "tilde"),
    ('Ú', 'U', "acute"),
    ('Ü', 'U', "diaer"),
];

/// Uma letra pronta: `HEIGHT` linhas de bits, so as colunas pintadas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    /// `rows[y]` tem o bit `x` ligado onde a coluna `x` (da esquerda) e tinta.
    pub rows: [u8; HEIGHT],
    /// Colunas pintadas (0 = espaco).
    pub width: usize,
    /// Quanto o espaco anda nesta fonte.
    space: usize,
}

impl Glyph {
    /// Quanto a letra anda: a largura e 1 coluna de folga; o espaco, o da
    /// fonte.
    pub fn advance(&self) -> usize {
        if self.width == 0 {
            self.space
        } else {
            self.width + 1
        }
    }

    pub fn ink(&self, x: usize, y: usize) -> bool {
        x < self.width && y < HEIGHT && self.rows[y] & (1 << x) != 0
    }
}

type Cell = [[bool; WIDTH]; HEIGHT];

fn paint(art: &str, first: usize, rows: &mut Cell) {
    for (y, line) in art.split('/').enumerate() {
        for (x, c) in line.chars().enumerate().take(WIDTH) {
            if let Some(row) = rows.get_mut(first + y) {
                row[x] = c == '#';
            }
        }
    }
}

fn base_cell(c: char) -> Option<Cell> {
    let (_, art) = BASE.iter().find(|(b, _)| *b == c)?;
    let mut rows = [[false; WIDTH]; HEIGHT];
    paint(art, CAP_TOP, &mut rows);
    Some(rows)
}

fn cell(c: char) -> Option<Cell> {
    if let Some(rows) = base_cell(c) {
        return Some(rows);
    }
    if let Some((_, base)) = CEDILLA.iter().find(|(ch, _)| *ch == c) {
        let mut rows = base_cell(*base)?;
        paint(CEDILLA_ART, CAP_TOP + CAP, &mut rows);
        return Some(rows);
    }
    let (_, base, accent) = COMPOSED.iter().find(|(ch, _, _)| *ch == c)?;
    let mut rows = base_cell(*base)?;
    let (_, art) = ACCENTS.iter().find(|(name, _)| name == accent)?;
    // Minuscula: o acento ocupa as 2 linhas de cima da maiuscula (o pingo do
    // "i" sai).
    let top = if base.is_uppercase() { 0 } else { CAP_TOP };
    if *base == 'i' {
        rows[CAP_TOP] = [false; WIDTH];
        rows[CAP_TOP + 1] = [false; WIDTH];
    }
    paint(art, top, &mut rows);
    Some(rows)
}

fn trimmed(rows: Cell, space: usize) -> Glyph {
    let used: Vec<usize> = (0..WIDTH).filter(|&x| rows.iter().any(|r| r[x])).collect();
    let (Some(&left), Some(&right)) = (used.first(), used.last()) else {
        return Glyph {
            rows: [0; HEIGHT],
            width: 0,
            space,
        };
    };
    let mut out = [0u8; HEIGHT];
    for (y, row) in rows.iter().enumerate() {
        for (x, &on) in row.iter().enumerate().take(right + 1).skip(left) {
            if on {
                out[y] |= 1 << (x - left);
            }
        }
    }
    Glyph {
        rows: out,
        width: right - left + 1,
        space,
    }
}

fn glyphs() -> &'static HashMap<char, Glyph> {
    static GLYPHS: OnceLock<HashMap<char, Glyph>> = OnceLock::new();
    GLYPHS.get_or_init(|| {
        BASE.iter()
            .map(|(c, _)| *c)
            .chain(COMPOSED.iter().map(|(c, _, _)| *c))
            .chain(CEDILLA.iter().map(|(c, _)| *c))
            .filter_map(|c| Some((c, trimmed(cell(c)?, SPACE))))
            .collect()
    })
}

/// A celula de uma letra da fonte estreita: 2 linhas de acento, 5 da letra e
/// 2 de cedilha (no lugar da descendente da normal).
fn narrow_cell(c: char) -> Option<Cell> {
    let up = c.to_uppercase().next()?;
    let base = |b: char| -> Option<Cell> {
        let (_, art) = NARROW.iter().find(|(n, _)| *n == b)?;
        let mut rows = [[false; WIDTH]; HEIGHT];
        paint(art, CAP_TOP, &mut rows);
        Some(rows)
    };
    if let Some(rows) = base(up) {
        return Some(rows);
    }
    if up == 'Ç' {
        let mut rows = base('C')?;
        paint(".#./##.", CAP_TOP + NARROW_CAP, &mut rows);
        return Some(rows);
    }
    let (_, b, accent) = NARROW_COMPOSED.iter().find(|(n, _, _)| *n == up)?;
    let mut rows = base(*b)?;
    let (_, art) = NARROW_ACCENTS.iter().find(|(name, _)| name == accent)?;
    paint(art, 0, &mut rows);
    Some(rows)
}

/// Altura da letra da fonte estreita.
pub const NARROW_CAP: usize = 5;

fn narrow_glyphs() -> &'static HashMap<char, Glyph> {
    static GLYPHS: OnceLock<HashMap<char, Glyph>> = OnceLock::new();
    GLYPHS.get_or_init(|| {
        glyphs()
            .keys()
            .filter_map(|&c| Some((c, trimmed(narrow_cell(c)?, 2))))
            .collect()
    })
}

/// As duas fontes do app: a normal e, pra espaco apertado, a estreita.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Regular,
    Narrow,
}

impl Font {
    pub fn glyph(self, c: char) -> Option<&'static Glyph> {
        match self {
            Font::Regular => glyphs().get(&c),
            Font::Narrow => narrow_glyphs().get(&c),
        }
    }

    /// Largura em pixels de `text` (sem a folga depois da ultima letra).
    /// `None` = tem letra que a fonte nao tem.
    pub fn width(self, text: &str) -> Option<usize> {
        let mut total = 0;
        for c in text.chars() {
            total += self.glyph(c)?.advance();
        }
        Some(total.saturating_sub(1))
    }

    /// Altura da maiuscula.
    pub fn cap(self) -> usize {
        match self {
            Font::Regular => CAP,
            Font::Narrow => NARROW_CAP,
        }
    }

    /// Linhas de pixel de uma linha de texto: a celula toda e 1 de folga.
    pub fn line(self) -> usize {
        match self {
            Font::Regular => HEIGHT + 1,
            Font::Narrow => CAP_TOP + NARROW_CAP + 2 + 1,
        }
    }
}

/// A letra `c`, se a fonte tem.
pub fn glyph(c: char) -> Option<&'static Glyph> {
    glyphs().get(&c)
}

/// Largura em pixels de `text` escrito com a fonte (sem a folga depois da
/// ultima letra). `None` = tem letra que a fonte nao tem.
pub fn width(text: &str) -> Option<usize> {
    let mut total = 0;
    for c in text.chars() {
        total += glyph(c)?.advance();
    }
    Some(total.saturating_sub(1))
}

/// Os caracteres que a fonte escreve, em ordem: a lista que vai pro prompt.
pub fn charset() -> String {
    let mut chars: Vec<char> = glyphs().keys().copied().collect();
    chars.sort_unstable();
    chars.into_iter().collect()
}

/// O sinal de uma letra que a fonte compoe (acento ou cedilha): a letra base,
/// o desenho do sinal (2 linhas de `WIDTH` colunas, separadas por `/`) e se
/// ele vai embaixo da letra.
pub fn mark(c: char) -> Option<(char, &'static str, bool)> {
    if let Some((_, base)) = CEDILLA.iter().find(|(ch, _)| *ch == c) {
        return Some((*base, CEDILLA_ART, true));
    }
    let (_, base, accent) = COMPOSED.iter().find(|(ch, _, _)| *ch == c)?;
    let (_, art) = ACCENTS.iter().find(|(name, _)| name == accent)?;
    Some((*base, art, false))
}

/// As letras que a fonte compoe com um sinal (`mark`), sempre na mesma ordem.
pub fn marked() -> impl Iterator<Item = char> {
    COMPOSED
        .iter()
        .map(|(c, _, _)| *c)
        .chain(CEDILLA.iter().map(|(c, _)| *c))
}

/// O primeiro caractere de `text` que a fonte nao escreve.
pub fn missing(text: &str) -> Option<char> {
    text.chars().find(|c| glyph(*c).is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_as_wide_as_their_ink_and_accents_sit_on_top() {
        assert_eq!(glyph('i').unwrap().width, 3);
        assert_eq!(glyph('m').unwrap().width, 5);
        assert_eq!(glyph(' ').unwrap().advance(), SPACE);
        let a = glyph('a').unwrap();
        let acute = glyph('á').unwrap();
        // Mesma letra da linha da minuscula pra baixo; o acento fica em cima.
        assert_eq!(a.rows[CAP_TOP + 2..], acute.rows[CAP_TOP + 2..]);
        assert!(a.rows[CAP_TOP] == 0 && acute.rows[CAP_TOP] != 0);
        assert!(
            glyph('Ç').unwrap().rows[CAP_TOP + CAP] != 0,
            "cedilha na descendente"
        );
        assert_eq!(width("ii"), Some(3 + 1 + 3));
        assert_eq!(missing("Opções ✓"), Some('✓'));
        assert!("ÁÉÍÓÚÂÊÔÃÕÇáéíóúâêôãõçàÀü"
            .chars()
            .all(|c| glyph(c).is_some()));
    }

    #[test]
    fn the_narrow_font_fits_what_the_regular_one_does_not() {
        // "カナ" no jogo medido: 16 pixels.
        assert!(Font::Regular.width("Kata").unwrap() > 16);
        assert!(Font::Narrow.width("Kata").unwrap() <= 16);
        // Versalete: a minuscula e a maiuscula sao a mesma letra.
        assert_eq!(Font::Narrow.glyph('a'), Font::Narrow.glyph('A'));
        assert!("Opções açúcar"
            .chars()
            .all(|c| Font::Narrow.glyph(c).is_some()));
    }
}

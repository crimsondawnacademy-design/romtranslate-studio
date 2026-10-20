//! Letra acentuada desenhada na fonte do proprio jogo. A fonte de um jogo
//! japones nao tem "á": tem "ａ" (largura inteira) e desenhos vazios em
//! codigos que o texto nunca usa. A extracao escolhe esses codigos
//! (`choose`) e poe a letra na tabela; a reinsercao a desenha la (`draw`), na
//! copia de trabalho. A letra base vem da propria fonte (mesmo tamanho, mesmo
//! traco) e o sinal, da fonte do app (`font::mark`).
//!
//! Medido em 18/10/2026 no RPG japones de 32 MiB (so contagens): duas fontes
//! na mesma ordem, 7.332 desenhos de 16x10 e de 8x8 pontos, 1 bit por ponto,
//! largura fixa (nenhuma tabela de larguras no arquivo); 371 desenhos vazios
//! nas mesmas posicoes das duas, 67 deles em codigos que o texto usa.

use std::collections::HashSet;

use super::font;
use crate::kuten::{self, FontTable};
use crate::tbl::TblTable;

/// Linhas que o sinal ocupa (os desenhos de `font::mark`).
const MARK_ROWS: usize = 2;
/// Larguras de linha que se tenta, em bytes.
const ROW_BYTES: [usize; 3] = [1, 2, 4];
/// Letras com a haste do lado esquerdo: e por elas que se ve a ordem dos bits.
const STEMMED: &str = "ＢＤＥＦＫＬＰＲｂｈｋ";
/// O corpo da letra nao fica com menos linhas que isto pra caber o sinal.
const MIN_BODY: usize = 3;
/// Corpo mais baixo que isto (3 vezes o sinal) e letra pequena: o acento
/// sobre ponta fina nao encosta nem na diagonal, se cabe. Medido em
/// 19/10/2026: na fonte de 8x8 do jogo a maiuscula achatada fica com 5 linhas
/// e o sinal encostado se emenda nela; na de 16x10, com 7, o sinal encostado
/// na diagonal le bem.
const LOOSE_BELOW: usize = 3 * MARK_ROWS;

/// Como os pontos de um desenho estao guardados: 1 bit por ponto, `row_bytes`
/// bytes por linha (o primeiro byte a esquerda) e, com `msb`, o bit mais alto
/// de cada byte e o ponto da esquerda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    row_bytes: usize,
    rows: usize,
    msb: bool,
}

/// Onde os desenhos de uma fonte tem tinta (a uniao de todos): o que fica
/// fora disso o jogo pode nem mostrar.
#[derive(Debug, Clone, Copy)]
struct Area {
    top: usize,
    bottom: usize,
    left: usize,
    right: usize,
}

/// Uma fonte em que da pra desenhar.
struct Drawable {
    table: FontTable,
    layout: Layout,
    area: Area,
}

/// Linhas de pontos: o bit `x` de cada uma e a coluna `x`, da esquerda.
fn read(layout: Layout, bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(layout.row_bytes)
        .take(layout.rows)
        .map(|row| {
            (0..row.len() * 8).fold(0, |bits, x| {
                let byte = row[x / 8];
                let on = if layout.msb {
                    byte >> (7 - x % 8) & 1
                } else {
                    byte >> (x % 8) & 1
                };
                bits | u32::from(on) << x
            })
        })
        .collect()
}

/// `read` ao contrario: grava `rows` nos bytes de um desenho.
fn store(layout: Layout, rows: &[u32], out: &mut [u8]) {
    out.fill(0);
    for (y, bits) in rows.iter().enumerate() {
        for x in (0..layout.row_bytes * 8).filter(|x| bits >> x & 1 == 1) {
            let bit = if layout.msb { 7 - x % 8 } else { x % 8 };
            if let Some(byte) = out.get_mut(y * layout.row_bytes + x / 8) {
                *byte |= 1 << bit;
            }
        }
    }
}

fn blank(glyph: &[u8]) -> bool {
    glyph.iter().all(|&b| b == 0)
}

/// A posicao de `c` na fonte de ordem `order`.
fn position(order: &[u16], c: char) -> Option<usize> {
    let unit = u16::try_from(c as u32).ok()?;
    order.iter().position(|&u| u == unit)
}

/// "｜" e uma coluna so (ou duas vizinhas), em 3 linhas seguidas ou mais.
fn is_bar(rows: &[u32]) -> bool {
    let columns = rows.iter().fold(0, |all, row| all | row);
    let inked: Vec<usize> = (0..rows.len()).filter(|&y| rows[y] != 0).collect();
    columns != 0
        && columns >> columns.trailing_zeros() <= 0b11
        && inked.len() >= 3
        && inked.last().zip(inked.first()).map(|(l, f)| l - f + 1) == Some(inked.len())
}

/// Quanto a tinta pende pra esquerda nas letras de haste: pontos na coluna
/// mais a esquerda menos os da mais a direita.
fn lean(rows: &[u32]) -> i64 {
    let columns = rows.iter().fold(0u32, |all, row| all | row);
    if columns == 0 {
        return 0;
    }
    let (left, right) = (columns.trailing_zeros(), 31 - columns.leading_zeros());
    let count = |x: u32| rows.iter().filter(|row| *row >> x & 1 == 1).count() as i64;
    count(left) - count(right)
}

/// Como `table` guarda os pontos, pelos desenhos que toda fonte japonesa tem:
/// "｜" so e uma coluna em linhas seguidas com a largura de linha certa, e a
/// haste de "Ｌ", "Ｆ", "Ｐ"... so fica a esquerda na ordem de bits certa.
/// `None` = falta um desses desenhos ou eles nao saem assim (fonte de mais
/// de 1 bit por ponto, ou o desenho nao vem logo depois do codigo).
fn layout(data: &[u8], table: FontTable, order: &[u16]) -> Option<Layout> {
    let glyph = |c: char| table.glyph(data, position(order, c)?);
    let bar = glyph('｜')?;
    let size = table.stride - 2;
    let row_bytes = ROW_BYTES
        .into_iter()
        .filter(|n| size.is_multiple_of(*n))
        .find(|&n| {
            let rows = size / n;
            is_bar(&read(
                Layout {
                    row_bytes: n,
                    rows,
                    msb: true,
                },
                bar,
            ))
        })?;
    let rows = size / row_bytes;
    let lean_with = |msb: bool| -> i64 {
        let layout = Layout {
            row_bytes,
            rows,
            msb,
        };
        STEMMED
            .chars()
            .filter_map(glyph)
            .map(|g| lean(&read(layout, g)))
            .sum()
    };
    let (msb, lsb) = (lean_with(true), lean_with(false));
    (msb.max(lsb) > 0).then_some(Layout {
        row_bytes,
        rows,
        msb: msb >= lsb,
    })
}

/// A caixa com tinta de todos os desenhos da fonte.
fn area(data: &[u8], table: FontTable, layout: Layout) -> Option<Area> {
    let (mut rows, mut columns) = (vec![false; layout.rows], 0u32);
    for k in 0..table.count {
        for (y, bits) in read(layout, table.glyph(data, k)?).into_iter().enumerate() {
            rows[y] |= bits != 0;
            columns |= bits;
        }
    }
    Some(Area {
        top: rows.iter().position(|&on| on)?,
        bottom: rows.iter().rposition(|&on| on)?,
        left: columns.trailing_zeros() as usize,
        right: 31usize.checked_sub(columns.leading_zeros() as usize)?,
    })
}

/// As fontes na mesma ordem da maior (`fonts[0]`, a do texto), cada uma com o
/// jeito de guardar os pontos. Vazio se alguma delas nao for entendida: o
/// codigo novo apareceria vazio no texto escrito com ela.
fn drawable(data: &[u8], fonts: &[FontTable], order: &[u16]) -> Vec<Drawable> {
    let Some(main) = fonts.first() else {
        return Vec::new();
    };
    let same = |font: &&FontTable| {
        font.count == main.count
            && (0..main.count).all(|k| font.code(data, k) == main.code(data, k))
    };
    let found: Option<Vec<Drawable>> = fonts
        .iter()
        .filter(same)
        .map(|&table| {
            let layout = layout(data, table, order)?;
            Some(Drawable {
                table,
                layout,
                area: area(data, table, layout)?,
            })
        })
        .collect();
    found.unwrap_or_default()
}

/// Pedacos de tinta (vizinhos ate na diagonal) e buracos (fundo cercado de
/// tinta, como o miolo do A e do O) do desenho.
fn shape(rows: &[u32]) -> (usize, usize) {
    // Uma volta de fundo em torno: o fundo que chega nela nao e buraco.
    let (w, h) = (34, rows.len() + 2);
    let ink = |x: usize, y: usize| {
        (1..=32).contains(&x) && (1..=rows.len()).contains(&y) && rows[y - 1] >> (x - 1) & 1 == 1
    };
    let (mut seen, mut pieces, mut holes) = (vec![false; w * h], 0, 0);
    for start in 0..w * h {
        if seen[start] {
            continue;
        }
        let on = ink(start % w, start / w);
        let (mut stack, mut open) = (vec![start], false);
        seen[start] = true;
        while let Some(p) = stack.pop() {
            let (x, y) = (p % w, p / w);
            open |= x == 0 || y == 0 || x == w - 1 || y == h - 1;
            for (dx, dy) in (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))) {
                // Tinta liga na diagonal; o fundo, so de lado.
                if (dx, dy) == (0, 0) || !on && dx != 0 && dy != 0 {
                    continue;
                }
                let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy))
                else {
                    continue;
                };
                if nx < w && ny < h && !seen[ny * w + nx] && ink(nx, ny) == on {
                    seen[ny * w + nx] = true;
                    stack.push(ny * w + nx);
                }
            }
        }
        if on {
            pieces += 1;
        } else if !open {
            holes += 1;
        }
    }
    (pieces, holes)
}

/// Quantas linhas iguais seguidas abrem e fecham o desenho: as pontas (o
/// topo do A, o pe dele).
fn ends(rows: &[u32]) -> (usize, usize) {
    let head = rows
        .iter()
        .take_while(|&row| Some(row) == rows.first())
        .count();
    let foot = rows
        .iter()
        .rev()
        .take_while(|&row| Some(row) == rows.last())
        .count();
    (head, foot)
}

/// Tira `n` linhas do corpo da letra (de `top` a `bottom`) e o encosta embaixo
/// (`keep_bottom`) ou em cima, sem desfazer a letra: ela fica com os mesmos
/// pedacos e buracos (a barra do A, o O fechado), e as pontas de cima e de
/// baixo (`ends`) com 2 linhas se tinham 2. Sai primeiro a linha igual a de
/// cima, a mais perto do meio; sem uma igual, a que menos muda. Medido em
/// 19/10/2026 na fonte de 8x8 do jogo: tirando as linhas repetidas sem essa
/// regra, saia a segunda linha do pe do A, e a barra virava a base de um
/// triangulo ("ESTᐃ", "NᐃO"). Sem linha que se possa tirar assim, tira como
/// antes. `None` = o corpo ficaria baixo demais.
fn squash(rows: &mut [u32], top: usize, bottom: usize, n: usize, keep_bottom: bool) -> Option<()> {
    let mut body = rows[top..=bottom].to_vec();
    if body.len() < n + MIN_BODY {
        return None;
    }
    let (form, (head, foot)) = (shape(&body), ends(&body));
    for _ in 0..n {
        let middle = body.len() / 2;
        let change = |&k: &usize| ((body[k] ^ body[k - 1]).count_ones(), k.abs_diff(middle));
        let keeps = |&k: &usize| {
            let mut rest = body.clone();
            rest.remove(k);
            let (h, f) = ends(&rest);
            shape(&rest) == form && h >= head.min(2) && f >= foot.min(2)
        };
        let k = (1..body.len())
            .filter(keeps)
            .min_by_key(change)
            .or_else(|| (1..body.len()).min_by_key(change))?;
        body.remove(k);
    }
    rows[top..=bottom].fill(0);
    let start = if keep_bottom {
        bottom + 1 - body.len()
    } else {
        top
    };
    rows[start..start + body.len()].copy_from_slice(&body);
    Some(())
}

/// A letra `c` (base + sinal de `font::mark`) a partir do desenho da base
/// (`base`, ja em linhas de pontos), dentro de `area`: o sinal vai nas
/// `MARK_ROWS` linhas logo acima do corpo (abaixo, a cedilha), centrado nele;
/// sem linhas livres, o corpo e achatado (`squash`). O pingo do "i" sai.
fn compose(c: char, base: &[u32], area: Area) -> Option<Vec<u32>> {
    let (_, art, below) = font::mark(c)?;
    let mark: Vec<u32> = art
        .split('/')
        .map(|line| {
            line.chars()
                .enumerate()
                .filter(|(_, ch)| *ch == '#')
                .fold(0, |bits, (x, _)| bits | 1 << x)
        })
        .collect();
    let mut rows = base.to_vec();
    let mut bottom = rows.iter().rposition(|&row| row != 0)?;
    // O corpo: as linhas com tinta seguidas que terminam na de baixo. O que
    // fica acima, separado por uma linha vazia, e o pingo.
    let mut top = bottom;
    while top > 0 && rows[top - 1] != 0 {
        top -= 1;
    }
    rows[..top].fill(0);
    let first = if below {
        let room = area.bottom.saturating_sub(bottom);
        if room < MARK_ROWS {
            squash(&mut rows, top, bottom, MARK_ROWS - room, false)?;
            bottom -= MARK_ROWS - room;
        }
        bottom + 1
    } else {
        let room = top.saturating_sub(area.top);
        if room < MARK_ROWS {
            squash(&mut rows, top, bottom, MARK_ROWS - room, true)?;
            top += MARK_ROWS - room;
        }
        top - MARK_ROWS
    };
    let body = rows.iter().fold(0u32, |all, row| all | row);
    let columns = mark.iter().fold(0u32, |all, row| all | row);
    let shift = columns.trailing_zeros();
    let width = (32 - (columns >> shift).leading_zeros()) as usize;
    let (left, right) = (
        body.trailing_zeros() as usize,
        31 - body.leading_zeros() as usize,
    );
    let x = (left + (right + 1 - left).saturating_sub(width).div_ceil(2))
        .min((area.right + 1).saturating_sub(width))
        .max(area.left);
    // O sinal que pousa em cima do traco some nele (o "i" de uma haste so):
    // anda uma coluna pro lado se isso o descola. Na letra pequena
    // (`LOOSE_BELOW`), o acento sobre ponta fina (o bico do A, a haste do I)
    // se emenda nela ate na diagonal (o do Á continuava o bico): ali ele
    // tenta antes, ate 2 colunas, nao encostar nem assim.
    let (near, edge) = if below {
        (mark[0], rows[first - 1])
    } else {
        (mark[MARK_ROWS - 1], rows[first + MARK_ROWS])
    };
    let clear = |x: usize, reach: u32| {
        x >= area.left && x + width <= area.right + 1 && (near >> shift << x) & reach == 0
    };
    let point = !below && bottom + 1 - top < LOOSE_BELOW && edge & edge << 1 == 0;
    let loose = [0, 1, -1, 2, -2]
        .into_iter()
        .filter(|_| point)
        .filter_map(|d| x.checked_add_signed(d))
        .find(|&x| clear(x, edge | edge << 1 | edge >> 1));
    let x = loose
        .or_else(|| {
            [Some(x), Some(x + 1), x.checked_sub(1)]
                .into_iter()
                .flatten()
                .find(|&x| clear(x, edge))
        })
        .unwrap_or(x);
    for (y, bits) in mark.iter().enumerate() {
        *rows.get_mut(first + y)? |= bits >> shift << x;
    }
    Some(rows)
}

/// A letra `c` desenhada pra fonte `drawable`, se a letra base dela (em largura
/// inteira) tem desenho la.
fn letter(data: &[u8], drawable: &Drawable, order: &[u16], c: char) -> Option<Vec<u32>> {
    let (base, _, _) = font::mark(c)?;
    let wide = char::from_u32(base as u32 + 0xFEE0)?;
    let glyph = drawable.table.glyph(data, position(order, wide)?)?;
    (!blank(glyph))
        .then(|| compose(c, &read(drawable.layout, glyph), drawable.area))
        .flatten()
}

/// Codigo -> letra acentuada que a reinsercao vai desenhar: cada letra que a
/// fonte do app compoe (`font::marked`) e cuja base tem desenho em todas as
/// fontes na ordem do texto, num codigo de desenho vazio em todas elas que
/// nenhum texto usa (`used`) e que nao passa de `shown`, o maior codigo de
/// caractere que o texto do jogo mostra: acima dele, nada garante que o jogo
/// desenhe. Medido em 18/10/2026: a fonte do jogo tem 7.332 desenhos, mas a
/// fala so alcanca os 4.096 primeiros (o codigo 7.326 saiu como o desenho
/// 3.230); o texto dele usa os desenhos ate o 3.798 (fora 6 usos de um codigo
/// de kanji raro), e as letras cairam nos vazios logo acima, entre o kanji
/// comum e o raro. Os codigos vem de `shown` pra tras. Vazio quando a fonte
/// nao e entendida (`layout`).
// ponytail: so fonte de 1 bit por ponto, achada pelo codigo Shift-JIS na
// frente de cada desenho, e de largura fixa. Fonte de largura variavel
// (tabela de larguras) ou de 2 e 4 bits fica sem acento, com a troca pela
// letra sem acento de sempre; tratar quando aparecer um jogo assim pra medir.
pub(crate) fn choose(
    data: &[u8],
    fonts: &[FontTable],
    order: &[u16],
    used: &HashSet<u16>,
    shown: u16,
) -> Vec<(u16, char)> {
    let drawable = drawable(data, fonts, order);
    if drawable.is_empty() {
        return Vec::new();
    }
    let free = (1..=shown)
        .rev()
        .filter(|code| !used.contains(code))
        .filter(|&code| {
            drawable
                .iter()
                .all(|d| d.table.glyph(data, code as usize).is_some_and(blank))
        });
    let letters =
        font::marked().filter(|&c| drawable.iter().all(|d| letter(data, d, order, c).is_some()));
    free.zip(letters).collect()
}

// ---- Pares de letras (DTE de fonte) ----
//
// Japones diz em um codigo de 2 bytes o que o portugues diz em 2 ou 3
// letras, e cada letra traduzida ocupa um codigo. O par ("de", "a ", "ão")
// desenhado num codigo que o texto do jogo nao usa poe 2 letras em cada
// codigo. Medido em 20/10/2026 no RPG japones de 32 MiB: a fonte grande
// guarda 16 colunas por desenho, mas o jogo anda 10 pontos por codigo (um
// desenho cheio, posto no lugar do "ｏ", cobre a letra seguinte) e as letras
// tem tinta nas colunas 1 a 9; na fonte pequena, 1 a 7. O par cabe na caixa
// de tinta da fonte (`Area`), nunca no desenho inteiro.

/// Colunas que o espaco ocupa num par ("a ", " o"): a folga entre as
/// palavras.
const PAIR_SPACE: usize = 3;

/// As colunas de tinta de `rows`, da esquerda: o bit `y` de cada uma e a
/// linha `y`.
fn columns(rows: &[u32]) -> Vec<u32> {
    let all = rows.iter().fold(0, |all, row| all | row);
    if all == 0 {
        return Vec::new();
    }
    (all.trailing_zeros()..32 - all.leading_zeros())
        .map(|x| {
            rows.iter()
                .enumerate()
                .fold(0, |column, (y, row)| column | (row >> x & 1) << y)
        })
        .collect()
}

/// `columns` com `width` colunas: sai a coluna igual a da esquerda dela, a
/// mais perto do meio primeiro (o traco de cima do "ａ" e 5 colunas iguais).
/// A letra fica com o desenho da fonte, so mais estreita. `None` = nao ha
/// coluna repetida pra tirar: letra de diagonal ("v", "x") ou redonda
/// demais. Medido em 20/10/2026: tirar a coluna que menos muda (como
/// `squash` faz com as linhas) deixava o "v" e o "m" ilegiveis com 4 pontos.
fn narrow(mut cols: Vec<u32>, width: usize) -> Option<Vec<u32>> {
    while cols.len() > width {
        let middle = cols.len() / 2;
        let k = (1..cols.len())
            .filter(|&k| cols[k] == cols[k - 1])
            .min_by_key(|k| k.abs_diff(middle))?;
        cols.remove(k);
    }
    Some(cols)
}

/// O par de letras `first` e `second` (colunas de `columns`; `None` = o
/// espaco) numa celula de `area`, com `rows` linhas: as duas lado a lado, 1
/// coluna entre elas, centradas; cada uma estreitada (`narrow`) so o que
/// falta pra caber, a mais larga primeiro. Com o espaco, a letra fica no
/// lado dela e o espaco leva `PAIR_SPACE` colunas. `None` = nao cabe.
fn pair(
    first: Option<Vec<u32>>,
    second: Option<Vec<u32>>,
    area: Area,
    rows: usize,
) -> Option<Vec<u32>> {
    let room = (area.right + 1).checked_sub(area.left)?;
    let (placed, x) = match (first, second) {
        (Some(a), Some(b)) => {
            let (mut wa, mut wb) = (a.len(), b.len());
            while wa + 1 + wb > room {
                let a_first = wa >= wb;
                let shrunk = |cols: &Vec<u32>, w: usize| narrow(cols.clone(), w - 1).is_some();
                match (shrunk(&a, wa), shrunk(&b, wb)) {
                    (true, true) if a_first => wa -= 1,
                    (true, true) => wb -= 1,
                    (true, false) => wa -= 1,
                    (false, true) => wb -= 1,
                    (false, false) => return None,
                }
            }
            let mut cols = narrow(a, wa)?;
            cols.push(0);
            cols.extend(narrow(b, wb)?);
            let x = area.left + (room - cols.len()) / 2;
            (cols, x)
        }
        (Some(a), None) => (narrow(a, room.checked_sub(PAIR_SPACE)?)?, area.left),
        (None, Some(b)) => {
            let cols = narrow(b, room.checked_sub(PAIR_SPACE)?)?;
            let x = area.right + 1 - cols.len();
            (cols, x)
        }
        (None, None) => return None,
    };
    let mut out = vec![0u32; rows];
    for (dx, column) in placed.iter().enumerate() {
        for (y, row) in out.iter_mut().enumerate() {
            *row |= (column >> y & 1) << (x + dx);
        }
    }
    Some(out)
}

/// O desenho que a letra `c` tem (ou vai ter) na fonte `drawable`: a de
/// largura inteira da propria fonte ("ａ" pra "a") ou a acentuada que
/// `letter` compoe. `None` = a fonte nao tem.
fn letter_of(data: &[u8], drawable: &Drawable, order: &[u16], c: char) -> Option<Vec<u32>> {
    if font::mark(c).is_some() {
        return letter(data, drawable, order, c);
    }
    let wide = char::from_u32(c as u32 + 0xFEE0).filter(|_| c.is_ascii_graphic())?;
    let glyph = drawable.table.glyph(data, position(order, wide)?)?;
    (!blank(glyph)).then(|| read(drawable.layout, glyph))
}

/// O par `text` (2 caracteres, letra ou espaco) desenhado pra `drawable`.
fn pair_of(data: &[u8], drawable: &Drawable, order: &[u16], text: &str) -> Option<Vec<u32>> {
    let mut chars = text.chars();
    let (Some(a), Some(b), None) = (chars.next(), chars.next(), chars.next()) else {
        return None;
    };
    let side = |c: char| -> Option<Option<Vec<u32>>> {
        if c == ' ' {
            return Some(None);
        }
        Some(Some(columns(&letter_of(data, drawable, order, c)?)))
    };
    pair(side(a)?, side(b)?, drawable.area, drawable.layout.rows)
}

/// O codigo `code` pode virar par: desenho vazio em todas as fontes, ou um
/// kanji (o que a traducao tira do texto). Kana, letra, sinal e numero ficam:
/// o teclado da tela de nomes, por exemplo, mostra kana que nenhum texto tem.
fn reusable(data: &[u8], drawable: &[Drawable], order: &[u16], code: u16) -> bool {
    let kanji = order
        .get(code as usize)
        .and_then(|&unit| char::from_u32(unit.into()))
        .is_some_and(crate::sjis::is_kanji);
    kanji
        || drawable
            .iter()
            .all(|d| d.table.glyph(data, code as usize).is_some_and(blank))
}

/// Codigo -> par de letras que a reinsercao vai desenhar: cada par de
/// `wanted` (na ordem, o mais usado primeiro) que cabe na celula de TODAS as
/// fontes na ordem do texto (`pair`) — uma fonte que nao comporta o par
/// mostraria o kanji de antes no lugar dele —, num codigo que nenhum texto do
/// jogo usa (`used`, traduzido ou nao), que pode virar par (`reusable`) e que
/// nao passa de `shown`, ate onde o texto mostra codigos (`kuten::reach`: a
/// fala pode nao alcancar acima dele). Os codigos vem de `shown` pra tras.
pub(crate) fn choose_pairs(
    data: &[u8],
    fonts: &[FontTable],
    order: &[u16],
    used: &HashSet<u16>,
    shown: u16,
    wanted: &[String],
) -> Vec<(u16, String)> {
    let drawable = drawable(data, fonts, order);
    if drawable.is_empty() {
        return Vec::new();
    }
    let free = (1..=shown)
        .rev()
        .filter(|code| !used.contains(code))
        .filter(|&code| reusable(data, &drawable, order, code));
    let pairs = wanted
        .iter()
        .filter(|text| {
            drawable
                .iter()
                .all(|d| pair_of(data, d, order, text).is_some())
        })
        .cloned();
    free.zip(pairs).collect()
}

/// Desenha em `out` (a copia de trabalho, com a fonte nos mesmos lugares que
/// em `original`) o que `table` (a tabela `kuten`) poe em codigos novos da
/// fonte, em todas as fontes na ordem do texto: cada letra acentuada num
/// codigo de desenho vazio (desenho que o original tem nao muda) e cada par
/// de letras num codigo que pode virar par (`reusable`). Devolve quantas
/// letras e quantos pares desenhou.
pub fn draw(original: &[u8], out: &mut [u8], table: &TblTable) -> (usize, usize) {
    let fonts = kuten::fonts(original);
    let Some(order) = kuten::font_order(original, &fonts) else {
        return (0, 0);
    };
    let drawable = drawable(original, &fonts, &order);
    let (mut letters, mut pairs) = (0, 0);
    for (key, text) in table.entries() {
        let Ok(key) = <[u8; 2]>::try_from(key) else {
            continue;
        };
        let k = u16::from_le_bytes(key);
        let mut chars = text.chars();
        let drawn: Option<Vec<Vec<u32>>> = match (chars.next(), chars.next(), chars.next()) {
            (Some(c), None, _) => drawable
                .iter()
                .map(|d| {
                    let empty = d.table.glyph(original, k as usize).is_some_and(blank);
                    empty.then(|| letter(original, d, &order, c)).flatten()
                })
                .collect(),
            (Some(_), Some(_), None)
                if !text.starts_with('[') && reusable(original, &drawable, &order, k) =>
            {
                drawable
                    .iter()
                    .map(|d| pair_of(original, d, &order, text))
                    .collect()
            }
            _ => None,
        };
        let Some(drawn) = drawn.filter(|drawn| !drawn.is_empty()) else {
            continue;
        };
        for (d, rows) in drawable.iter().zip(drawn) {
            let at = d.table.start + k as usize * d.table.stride + 2;
            if let Some(glyph) = out.get_mut(at..at + d.table.stride - 2) {
                store(d.layout, &rows, glyph);
            }
        }
        match text.chars().count() {
            1 => letters += 1,
            _ => pairs += 1,
        }
    }
    (letters, pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(art: &str) -> Vec<u32> {
        art.split('/')
            .map(|line| {
                line.chars()
                    .enumerate()
                    .filter(|(_, c)| *c == '#')
                    .fold(0, |bits, (x, _)| bits | 1 << x)
            })
            .collect()
    }

    const AREA: Area = Area {
        top: 0,
        bottom: 8,
        left: 1,
        right: 9,
    };

    #[test]
    fn the_mark_goes_over_the_body_and_a_tall_letter_is_squashed() {
        // Minuscula com 2 linhas livres em cima: o corpo nao muda.
        let a = rows(".........../........../..#####.../.......#../..######../.#.....#../.#.....#../..######../........./..........");
        let acute = compose('á', &a, AREA).unwrap();
        assert_eq!(acute[2..], a[2..]);
        assert!(acute[0] != 0 && acute[1] != 0);
        // Maiuscula de 9 linhas: sai uma linha repetida de cada par, e o
        // acento entra em cima, com o pe da letra no mesmo lugar.
        let big = rows("....#...../....#...../...#.#..../...#.#..../..#...#.../..#####.../.#.....#../.#.....#../.#.....#../..........");
        let capital = compose('Á', &big, AREA).unwrap();
        assert_eq!(
            capital[2..9],
            [big[0], big[1], big[2], big[4], big[5], big[6], big[8]],
            "o topo desce 2 linhas; sai uma da diagonal e uma do pe"
        );
        // O corpo tem 7 linhas: o acento encosta na diagonal e fica (a fonte
        // de 16x10 do jogo medido, que ja lia bem).
        assert_eq!(capital[..2], [1 << 6, 1 << 5]);
        // O pingo do "i" sai e o acento entra no lugar dele.
        let i = rows("...#....../........../..##....../...#....../...#....../...#....../...#....../...#....../..###...../..........");
        assert_eq!(compose('í', &i, AREA).unwrap()[2..], i[2..]);
        // Cedilha: sem linha livre embaixo, o corpo sobe.
        let c = rows("........../........../..####..../.#....#.../.#......../.#......../.#......../.#....#.../..####..../..........");
        let cedilla = compose('ç', &c, AREA).unwrap();
        assert_eq!(cedilla[2], c[2], "o topo fica");
        assert!(cedilla[7] != 0 && cedilla[8] != 0);
    }

    /// Fonte inventada de 8x8 como a pequena do jogo medido: a maiuscula
    /// ocupa as 7 linhas que a fonte usa, sem nenhuma livre pro sinal, e o A
    /// tem bico de 1 ponto e pe de 2 linhas.
    const SMALL: Area = Area {
        top: 0,
        bottom: 6,
        left: 0,
        right: 7,
    };
    const SMALL_CAPITALS: [(char, &str); 6] = [
        (
            'A',
            "...#.../..#.#../..#.#../.#...#./.#####./#.....#/#.....#",
        ),
        (
            'E',
            "#####../#....../#....../####.../#....../#....../#####..",
        ),
        (
            'I',
            "...#.../...#.../...#.../...#.../...#.../...#.../...#...",
        ),
        (
            'O',
            ".###.../#...#../#...#../#...#../#...#../#...#../.###...",
        ),
        (
            'U',
            "#...#../#...#../#...#../#...#../#...#../#...#../.###...",
        ),
        (
            'C',
            ".###.../#...#../#....../#....../#....../#...#../.###...",
        ),
    ];

    /// A maiuscula achatada continua a letra: so perdeu linhas (nada
    /// inventado), tem os mesmos pedacos e buracos, a linha de cima e a de
    /// baixo de antes, e o pe com 2 linhas se tinha 2. Medido em 19/10/2026:
    /// sem isso, o A perdia a segunda linha do pe e virava triangulo.
    #[test]
    fn a_capital_squashed_in_a_small_font_is_still_the_letter() {
        for c in "ÁÀÂÃÉÊÍÓÔÕÚÇ".chars() {
            let (base, _, below) = font::mark(c).unwrap();
            let (_, art) = SMALL_CAPITALS.iter().find(|(b, _)| *b == base).unwrap();
            let base = rows(art);
            let drawn = compose(c, &base, SMALL).unwrap();
            let body = if below { &drawn[..5] } else { &drawn[2..] };
            let mut rest = base.iter();
            assert!(
                body.iter().all(|row| rest.any(|b| b == row)),
                "{c}: so tirou linhas"
            );
            assert_eq!(shape(body), shape(&base), "{c}: pedacos e buracos");
            assert_eq!((body[0], body[4]), (base[0], base[6]), "{c}: topo e base");
            assert!(ends(body).1 >= ends(&base).1.min(2), "{c}: o pe");
            // Sobre o bico do A e a haste do I, o acento fica solto: um
            // pedaco a mais.
            if "ÁÀÍ".contains(c) {
                assert_eq!(shape(&drawn).0, shape(body).0 + 1, "{c}: acento solto");
            }
        }
        // O A: bico, a diagonal de cima, a barra e o pe inteiro.
        let a = rows(SMALL_CAPITALS[0].1);
        assert_eq!(
            compose('Ã', &a, SMALL).unwrap()[2..],
            [a[0], a[1], a[4], a[5], a[6]]
        );
    }

    /// So sai coluna repetida, a do meio primeiro: a letra fica com o
    /// desenho dela. A de diagonal nao estreita.
    #[test]
    fn a_letter_narrows_only_by_its_repeated_columns() {
        let a = columns(&rows(
            "..#####../.#.....#./.......#./..######./.#.....#./..######.",
        ));
        assert_eq!(a.len(), 7);
        // O traco de cima e o de baixo ocupam 5 colunas iguais.
        assert_eq!(narrow(a.clone(), 3).unwrap(), [a[0], a[1], a[6]]);
        assert_eq!(narrow(a.clone(), 2), None);
        let v = columns(&rows("#.....#/.#...#./..#.#../...#..."));
        assert_eq!(narrow(v.clone(), 7), Some(v.clone()));
        assert_eq!(narrow(v, 6), None);
    }

    /// Duas letras lado a lado com uma coluna entre elas, centradas na caixa
    /// de tinta; com o espaco, a letra no lado dela; o que nao cabe, `None`.
    #[test]
    fn a_pair_fits_in_the_ink_box_of_the_font() {
        let area = Area {
            top: 0,
            bottom: 3,
            left: 1,
            right: 9,
        };
        let i = columns(&rows("#/#/#/#"));
        let o = columns(&rows(".##./#..#/#..#/.##."));
        let drawn = pair(Some(i.clone()), Some(o.clone()), area, 4).unwrap();
        // 1 + 1 + 4 = 6 colunas, centradas em 9: comecam na coluna 2.
        assert_eq!(columns(&drawn), [i[0], 0, o[0], o[1], o[2], o[3]]);
        assert_eq!(
            drawn.iter().fold(0, |all, row| all | row).trailing_zeros(),
            2
        );
        // "o ": a letra encostada na esquerda; " i": na direita.
        let left = pair(Some(o.clone()), None, area, 4).unwrap();
        assert_eq!(
            left.iter().fold(0, |all, row| all | row).trailing_zeros(),
            1
        );
        let right = pair(None, Some(i.clone()), area, 4).unwrap();
        assert_eq!(right.iter().fold(0, |all, row| all | row), 1 << 9);
        // Letra que nao estreita e nao cabe com a outra.
        let wide = columns(&rows("#.#.#.#"));
        assert!(pair(Some(wide.clone()), Some(o), area, 4).is_none());
        assert!(pair(Some(wide.clone()), Some(i), area, 4).is_some());
        // Com o espaco, sobram `PAIR_SPACE` colunas.
        assert!(pair(Some(wide), None, area, 4).is_none());
        assert!(pair(None, None, area, 4).is_none());
    }

    #[test]
    fn the_bar_tells_the_row_width() {
        assert!(is_bar(&rows("...#/...#/...#/...#")));
        assert!(!is_bar(&rows("...#/..../...#/....")), "linhas salteadas");
        assert!(!is_bar(&rows("...#...#/...#...#/...#...#")), "duas colunas");
    }

    #[test]
    fn stored_glyphs_read_back_in_both_bit_orders() {
        for msb in [true, false] {
            let layout = Layout {
                row_bytes: 2,
                rows: 3,
                msb,
            };
            let art = rows("#..........#/.#........#./..########..");
            let mut bytes = [0u8; 6];
            store(layout, &art, &mut bytes);
            assert_eq!(read(layout, &bytes), art);
        }
    }

    #[test]
    fn no_font_means_no_letters_and_nothing_drawn() {
        let table = TblTable::parse("0100=á\n").unwrap();
        for data in [Vec::new(), vec![0x81, 0x40], vec![0xFF; 4096]] {
            assert!(choose(&data, &kuten::fonts(&data), &[], &HashSet::new(), 100).is_empty());
            let mut out = data.clone();
            assert_eq!(draw(&data, &mut out, &table), (0, 0));
            assert_eq!(out, data);
        }
    }
}

//! Camada D (spec §10): LZ77 do BIOS do GBA/NDS, tipo 0x10 (SWI 11h/12h) —
//! o compressor de quase todo jogo de GBA e dos primeiros de DS. Formato
//! conferido no GBATEK, "BIOS Decompression Functions":
//!
//! - header u32 LE: bits 0-3 reservados (0), bits 4-7 tipo (1), bits 8-31
//!   tamanho descomprimido;
//! - depois, repetido: 1 byte de flags (MSB primeiro) e 8 blocos. Flag 0 =
//!   1 byte literal. Flag 1 = 2 bytes: nibble alto do primeiro = N-3, nibble
//!   baixo = MSBs do disp, segundo byte = LSBs do disp; copia N bytes de
//!   (dest - disp - 1);
//! - a origem fica alinhada em 4 bytes e o fim e preenchido com zeros.

pub const MIN_MATCH: usize = 3;
pub const MAX_MATCH: usize = 18;
const WINDOW: usize = 4096;
/// O tamanho mora em 24 bits: nenhum bloco passa de 16 MiB descomprimido —
/// o proprio formato limita a "bomba" de um bloco.
pub const MAX_UNPACKED: usize = 0x00FF_FFFF;
/// Abaixo disto um "bloco" valido e coincidencia barata demais.
pub const MIN_UNPACKED: usize = 32;
pub const MIN_MATCHES: usize = 2;
/// Teto de bytes descomprimidos numa varredura inteira, tentativas falhas
/// inclusas: arquivo com milhares de headers falsos nao trava o app.
pub const SCAN_BUDGET: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpacked {
    pub data: Vec<u8>,
    /// Bytes consumidos da origem (header incluso, padding final nao).
    pub packed_len: usize,
    /// Quantos blocos eram copia. Zero = so literais, que qualquer dado cru
    /// precedido de um 0x10 tambem "descomprime".
    pub matches: usize,
}

/// Descomprime um bloco que comeca em `src[0]`. `None` para qualquer coisa
/// que nao seja um bloco inteiro e valido — nunca panica.
pub fn unpack(src: &[u8]) -> Option<Unpacked> {
    let mut unlimited = usize::MAX;
    unpack_within(src, &mut unlimited)
}

/// Igual a `unpack`, descontando de `budget` cada byte produzido; quando nao
/// ha saldo, zera o budget e desiste. Quem varre um arquivo atras de blocos
/// divide um budget so
/// entre todas as tentativas: e o que segura arquivo malicioso com milhares
/// de headers falsos (spec §22, decompression bombs).
pub fn unpack_within(src: &[u8], budget: &mut usize) -> Option<Unpacked> {
    let header = src.get(..4)?;
    if header[0] != 0x10 {
        return None;
    }
    let size = u32::from_le_bytes([header[1], header[2], header[3], 0]) as usize;
    let mut out = Vec::with_capacity(size.min(64 * 1024));
    let mut pos = 4;
    let mut matches = 0;
    while out.len() < size {
        let flags = *src.get(pos)?;
        pos += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            let len = if flags & (1 << bit) == 0 {
                out.push(*src.get(pos)?);
                pos += 1;
                1
            } else {
                let pair = src.get(pos..pos + 2)?;
                pos += 2;
                let dist = ((((pair[0] & 0x0F) as usize) << 8) | pair[1] as usize) + 1;
                if dist > out.len() {
                    return None;
                }
                // O ultimo bloco pode passar do tamanho declarado; o BIOS para nele.
                let len = ((pair[0] >> 4) as usize + MIN_MATCH).min(size - out.len());
                for _ in 0..len {
                    out.push(out[out.len() - dist]);
                }
                matches += 1;
                len
            };
            let Some(rest) = budget.checked_sub(len) else {
                *budget = 0;
                return None;
            };
            *budget = rest;
        }
    }
    Some(Unpacked {
        data: out,
        packed_len: pos,
        matches,
    })
}

/// Visita, em ordem de posicao, cada bloco valido de `data` (descomprime
/// inteiro, `MIN_UNPACKED`+ bytes, `MIN_MATCHES`+ copias). Devolve `true` se
/// parou por falta de `budget`: ha arquivo alem do que foi olhado.
// ponytail: so offsets multiplos de 4 — o BIOS exige a origem alinhada
// (GBATEK), e e o que derruba falso positivo. Bloco guardado desalinhado
// dentro de um arquivo proprio do jogo passa batido. So o tipo 0x10; o 0x11
// (LZ11, DS mais novo) entra quando houver fonte primaria e fixture.
pub fn each_block(
    data: &[u8],
    budget: &mut usize,
    mut visit: impl FnMut(usize, &Unpacked),
) -> bool {
    let mut at = 0;
    while at + 4 <= data.len() {
        if *budget == 0 {
            return true;
        }
        let block = (data[at] == 0x10)
            .then(|| unpack_within(&data[at..], budget))
            .flatten()
            .filter(|b| b.data.len() >= MIN_UNPACKED && b.matches >= MIN_MATCHES);
        let Some(block) = block else {
            at += 4;
            continue;
        };
        visit(at, &block);
        at += block.packed_len.next_multiple_of(4);
    }
    false
}

/// Comprime no mesmo formato. `None` se `data` nao cabe nos 24 bits do header.
///
/// Escolha otima das copias (programacao dinamica de tras pra frente: literal
/// custa 9 bits, copia 17), nao gulosa: o resultado nunca e maior que o de um
/// compressor guloso sobre os mesmos dados. E o que deixa um bloco do jogo,
/// regravado com texto do mesmo tamanho, caber no espaco que ja ocupava.
pub fn pack(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() > MAX_UNPACKED {
        return None;
    }
    let n = data.len();
    let longest: Vec<(u8, u16)> = (0..n).map(|i| longest_match(data, i)).collect();
    // cost[i] = bits pra codificar data[i..]; take[i] = bytes do bloco em i.
    let mut cost = vec![0u32; n + 1];
    let mut take = vec![1u8; n];
    for i in (0..n).rev() {
        cost[i] = cost[i + 1] + 9;
        // Copia mais curta, na mesma distancia, tambem vale.
        for len in MIN_MATCH..=longest[i].0 as usize {
            if cost[i + len] + 17 < cost[i] {
                cost[i] = cost[i + len] + 17;
                take[i] = len as u8;
            }
        }
    }
    let mut out = vec![0x10, n as u8, (n >> 8) as u8, (n >> 16) as u8];
    let mut i = 0;
    while i < n {
        let flags_at = out.len();
        out.push(0);
        for bit in (0..8).rev() {
            if i >= n {
                break;
            }
            let len = take[i] as usize;
            if len >= MIN_MATCH {
                let disp = longest[i].1 as usize - 1;
                out[flags_at] |= 1 << bit;
                out.push((((len - MIN_MATCH) as u8) << 4) | (disp >> 8) as u8);
                out.push(disp as u8);
            } else {
                out.push(data[i]);
            }
            i += len;
        }
    }
    out.resize(out.len().next_multiple_of(4), 0);
    Some(out)
}

// ponytail: busca O(n * janela) em toda posicao — um bloco de texto de 64 KiB
// sai em menos de um segundo; trocar por hash-chain se um dia comprimir
// megabytes de uma vez.
fn longest_match(data: &[u8], at: usize) -> (u8, u16) {
    let mut best = (0, 0);
    // dist >= 2: a variante do BIOS que escreve em VRAM nao le disp=0 (GBATEK).
    for dist in 2..=at.min(WINDOW) {
        let mut len = 0;
        while len < MAX_MATCH && at + len < data.len() && data[at + len - dist] == data[at + len] {
            len += 1;
        }
        if len > best.0 as usize {
            best = (len as u8, dist as u16);
            if len == MAX_MATCH {
                break;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::make_random;

    #[test]
    fn unpacks_a_stream_assembled_by_hand_from_the_gbatek_layout() {
        // "ABC" literal + copia de 8 bytes a 3 de distancia (disp = 2):
        // flags 0b0001_0000, bloco de copia = ((8-3) << 4 | 0), 0x02.
        let stream = [
            0x10, 11, 0, 0, 0x10, b'A', b'B', b'C', 0x50, 0x02, 0x00, 0x00,
        ];
        let got = unpack(&stream).unwrap();
        assert_eq!(got.data, b"ABCABCABCAB");
        assert_eq!(got.packed_len, 10);
        assert_eq!(got.matches, 1);
    }

    #[test]
    fn pack_then_unpack_gives_the_input_back() {
        let text =
            b"It is dangerous to go alone! Take this. It is dangerous to go alone!".repeat(9);
        let zeros = vec![0u8; 5000];
        let noise = make_random(3000, 7);
        for input in [&text[..], &zeros[..], &noise[..], b"", b"A", b"AB"] {
            let packed = pack(input).unwrap();
            assert_eq!(packed.len() % 4, 0);
            assert_eq!(unpack(&packed).unwrap().data, input);
        }
        assert!(pack(&text).unwrap().len() < text.len() / 4);
    }

    #[test]
    fn optimal_parse_beats_taking_the_longest_copy_first() {
        // Em "bcdeXabcbcdeY" a copia mais longa a partir do "a" final (abc)
        // impede a de 4 (bcde) logo depois: o guloso gasta 2 literais a mais.
        let mut data = b"abc_bcde_".to_vec();
        data.extend(b"abcde");
        let packed = pack(&data).unwrap();
        assert_eq!(unpack(&packed).unwrap().data, data);
        // 9 literais + literal "a" + copia de 4: 10 + 2 + 2 bytes de flags.
        assert_eq!(unpack(&packed).unwrap().packed_len, 4 + 14);
    }

    #[test]
    fn each_block_finds_aligned_blocks_and_reports_the_budget() {
        let script = b"The quick brown fox jumps over the lazy dog. ".repeat(40);
        let block = pack(&script).unwrap();
        let mut data = vec![0u8; 8];
        data.extend(&block);
        data.extend(&block);
        let mut seen = Vec::new();
        let mut budget = usize::MAX;
        assert!(!each_block(&data, &mut budget, |at, b| seen.push((at, b.data.len()))));
        assert_eq!(seen, [(8, script.len()), (8 + block.len(), script.len())]);
        let mut short = script.len() + 10;
        let mut count = 0;
        assert!(each_block(&data, &mut short, |_, _| count += 1));
        assert_eq!(count, 1);
    }

    #[test]
    fn malformed_streams_are_rejected_not_panicked_on() {
        let packed = pack(&b"HELLO HELLO HELLO HELLO HELLO".repeat(4)).unwrap();
        for cut in 0..packed.len() - 4 {
            // Cortado antes do fim dos dados: sempre invalido.
            let _ = unpack(&packed[..cut]);
        }
        assert_eq!(unpack(&[]), None);
        assert_eq!(unpack(&[0x11, 4, 0, 0, 0, 1, 2, 3, 4]), None);
        // Copia apontando pra antes do inicio da saida.
        assert_eq!(unpack(&[0x10, 8, 0, 0, 0x80, 0x00, 0x05]), None);
        // Header promete mais do que a origem tem.
        assert_eq!(unpack(&[0x10, 0xFF, 0xFF, 0xFF, 0x00, 1, 2, 3]), None);
        for seed in 1..50 {
            let _ = unpack(&make_random(600, seed));
        }
    }

    #[test]
    fn budget_stops_a_block_that_would_produce_too_much() {
        let packed = pack(&vec![0u8; 100_000]).unwrap();
        let mut budget = 99_999;
        assert!(unpack_within(&packed, &mut budget).is_none());
        assert_eq!(budget, 0);
        budget = 100_000;
        assert!(unpack_within(&packed, &mut budget).is_some());
        assert_eq!(budget, 0);
    }
}

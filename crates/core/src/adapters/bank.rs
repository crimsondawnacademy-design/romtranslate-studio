//! Banco de mensagens em blocos: a estrutura que deixa a traducao CRESCER.
//!
//! E como o primeiro RPG japones medido guarda o roteiro (13/10/2026, so
//! contagens: um banco so, 563 blocos, 8.120 mensagens, 85,6% do texto do
//! cartucho):
//!
//! - no inicio do banco, um u32 com a quantidade N e N offsets u32 relativos
//!   a esse inicio (0 = vazio), aos pares: a tabela de um bloco e o texto dele;
//! - a tabela e uma fila de offsets u16 relativos ao comeco do bloco de texto
//!   (0xFFFF = sem mensagem), um por mensagem;
//! - cada mensagem fecha com 0xFFFF.
//!
//! Nada aqui supoe um jogo: o banco e reconhecido pela propria estrutura, com
//! centenas de offsets que tem que cair no lugar certo. Com ele, mensagem que
//! cresce nao precisa caber no espaco da original: o bloco e remontado com as
//! mensagens em fila e a tabela refeita. Se o bloco novo cabe onde o antigo
//! estava, fica la; senao vai pro espaco livre do arquivo (preenchimento de
//! 0xFF, e o lugar dos blocos que sairam) e o diretorio e reapontado.

use std::collections::{BTreeMap, HashMap};

use crate::adapter::ApplyReport;
use crate::error::{CoreError, Result};
use crate::types::TextEntry;

const END: u16 = 0xFFFF;
/// Entradas do diretorio de um banco: par, e nem pouco nem um absurdo.
const DIR_ENTRIES: std::ops::RangeInclusive<usize> = 8..=20_000;
/// O primeiro bloco vem logo depois do diretorio, com no maximo isto de
/// alinhamento.
const DIR_SLACK: usize = 64;
/// Um bloco de texto nao passa disto: os offsets da tabela sao u16, e
/// 0xFFFF quer dizer "sem mensagem".
const MAX_TEXT: usize = 0xFFFE;
/// So preenchimento longo assim (em bytes de 0xFF seguidos) e tratado como
/// espaco livre: trecho curto de 0xFF pode ser dado.
const MIN_PADDING: usize = 64 * 1024;

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Um bloco: a tabela de offsets e o texto das mensagens.
#[derive(Debug, Clone)]
pub(crate) struct Room {
    /// Os pares do diretorio que apontam pra este bloco (quase sempre um).
    pairs: Vec<usize>,
    table: usize,
    /// Quantos offsets a tabela tem.
    slots: usize,
    text: usize,
    text_len: usize,
    /// Onde cada mensagem comeca dentro do bloco de texto: os offsets da
    /// tabela, em ordem e sem repetir, com o 0 na frente. Vazio = bloco com
    /// offset fora dele: nao e remontado.
    starts: Vec<usize>,
}

impl Room {
    /// Bytes que o bloco ocupa, tabela e texto.
    fn size(&self) -> usize {
        self.text + self.text_len - self.table
    }

    /// (inicio, fim) do trecho da mensagem `i` dentro do bloco de texto.
    fn segment(&self, i: usize) -> (usize, usize) {
        let end = self.starts.get(i + 1).copied().unwrap_or(self.text_len);
        (self.starts[i], end)
    }

    /// A mensagem que contem os bytes `[at, at + len)` do arquivo, se eles
    /// cabem inteiros numa so.
    fn segment_of(&self, at: usize, len: usize) -> Option<usize> {
        let rel = at.checked_sub(self.text)?;
        let i = self
            .starts
            .partition_point(|&start| start <= rel)
            .checked_sub(1)?;
        (rel + len <= self.segment(i).1).then_some(i)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Bank {
    /// Onde o banco comeca (o u32 da quantidade).
    pub at: usize,
    rooms: Vec<Room>,
}

impl Bank {
    fn room_at(&self, at: usize) -> Option<&Room> {
        let i = self
            .rooms
            .partition_point(|room| room.text <= at)
            .checked_sub(1)?;
        self.rooms
            .get(i)
            .filter(|room| at < room.text + room.text_len)
    }
}

/// O banco que comeca em `at`, se a estrutura inteira confere.
fn bank_at(data: &[u8], at: usize) -> Option<Bank> {
    let count = u32_at(data, at)?;
    if !DIR_ENTRIES.contains(&count) || count % 2 != 0 {
        return None;
    }
    let dir_len = 4 + 4 * count;
    // O diretorio: zero, ou offsets que nunca diminuem, o primeiro logo
    // depois do proprio diretorio. A checagem barata vem primeiro, sem
    // guardar nada: esta funcao roda em cada posicao alinhada do arquivo.
    let (mut last, mut filled) = (0, 0);
    for i in 0..count {
        let value = u32_at(data, at + 4 + 4 * i)?;
        if value != 0 {
            let floor = if last == 0 { dir_len } else { last };
            let ceiling = if last == 0 {
                dir_len + DIR_SLACK
            } else {
                data.len() - at
            };
            if value < floor || value > ceiling {
                return None;
            }
            last = value;
            filled += 1;
        }
    }
    if filled < 8 {
        return None;
    }
    let dir: Vec<usize> = (0..count)
        .map(|i| u32_at(data, at + 4 + 4 * i))
        .collect::<Option<_>>()?;
    // Onde cada bloco de texto termina: no proximo offset do diretorio.
    let mut bounds: Vec<usize> = dir.iter().copied().filter(|v| *v != 0).collect();
    bounds.dedup();
    let next = |value: usize| {
        let i = bounds.partition_point(|b| *b <= value);
        bounds.get(i).copied()
    };

    let mut by_block: BTreeMap<(usize, usize), Room> = BTreeMap::new();
    let (mut total, mut in_range, mut after_end) = (0usize, 0usize, 0usize);
    for pair in 0..count / 2 {
        let (a, b) = (dir[2 * pair], dir[2 * pair + 1]);
        if a == 0 || b == 0 || a >= b {
            continue;
        }
        if let Some(room) = by_block.get_mut(&(a, b)) {
            room.pairs.push(pair);
            continue;
        }
        let (table, text, slots) = (at + a, at + b, (b - a) / 2);
        let offsets: Vec<u16> = (0..slots)
            .map(|k| u16_at(data, table + 2 * k))
            .collect::<Option<_>>()?;
        // O ultimo bloco nao tem vizinho: vai ate o fim da ultima mensagem.
        let text_len = match next(b) {
            Some(end) => end - b,
            None => {
                let far = offsets.iter().filter(|o| **o != END).max().copied()? as usize;
                let closes = (far..MAX_TEXT)
                    .step_by(2)
                    .find(|rel| u16_at(data, text + rel) == Some(END))?;
                (closes + 2).next_multiple_of(4)
            }
        };
        if text + text_len > data.len() {
            return None;
        }
        let mut starts = vec![0];
        // Tabela e texto colados, e texto que um offset de 16 bits alcanca.
        let mut whole = text_len <= MAX_TEXT + 2 && (b - a) % 2 == 0;
        for &offset in offsets.iter().filter(|o| **o != END) {
            let rel = offset as usize;
            total += 1;
            if rel + 2 > text_len {
                whole = false;
                continue;
            }
            in_range += 1;
            // Mensagem comeca no comeco do bloco ou depois do fim de outra.
            let follows = rel <= 4 || u16_at(data, text + rel - 2) == Some(END);
            after_end += usize::from(follows);
            starts.push(rel);
        }
        starts.sort_unstable();
        starts.dedup();
        by_block.insert(
            (a, b),
            Room {
                pairs: vec![pair],
                table,
                slots,
                text,
                text_len,
                starts: if whole { starts } else { Vec::new() },
            },
        );
    }
    // Quase todo offset dentro do bloco dele, e a maioria logo depois de um
    // fim de mensagem: no jogo medido, 99,9% e 96%.
    let sound = by_block.len() >= 4
        && total >= 16
        && in_range * 100 >= total * 98
        && after_end * 10 >= total * 8;
    sound.then(|| Bank {
        at,
        rooms: by_block.into_values().collect(),
    })
}

/// Os bancos de mensagens de `data`, em ordem de posicao.
pub(crate) fn find(data: &[u8]) -> Vec<Bank> {
    let mut banks = Vec::new();
    let mut at = 0;
    while at + 8 <= data.len() {
        match bank_at(data, at) {
            Some(bank) => {
                at += 4 + 4 * u32_at(data, at).unwrap_or(0);
                banks.push(bank);
            }
            None => at += 4,
        }
    }
    banks
}

/// Onde comeca cada mensagem dos bancos (posicao no arquivo): o que a
/// tabela de cada bloco aponta. E o que impede a leitura de emendar o numero
/// da sala, ou o fim de uma mensagem, na mensagem seguinte.
pub(crate) fn message_starts(banks: &[Bank]) -> std::collections::HashSet<usize> {
    banks
        .iter()
        .flat_map(|bank| &bank.rooms)
        .flat_map(|room| room.starts.iter().map(move |start| room.text + start))
        .collect()
}

/// A entry esta numa mensagem de banco: a traducao dela pode crescer.
pub(crate) fn is_banked(entry: &TextEntry) -> bool {
    entry.metadata.get("bank").is_some_and(|v| v.is_object())
}

/// O tamanho do trecho mais longo de `text` entre dois codigos em colchetes.
pub(crate) fn longest_line(text: &str) -> usize {
    let (mut longest, mut line, mut inside) = (0, 0, false);
    for c in text.chars() {
        match c {
            '[' => {
                inside = true;
                line = 0;
            }
            ']' if inside => inside = false,
            _ if !inside => {
                line += 1;
                longest = longest.max(line);
            }
            _ => {}
        }
    }
    longest
}

/// Marca as entries que estao inteiras dentro de uma mensagem de banco: elas
/// deixam de ter teto de bytes. Devolve quantas marcou.
///
/// Junto vai a largura da caixa de texto, medida no proprio jogo: o tamanho
/// de linha (trecho entre dois comandos) que 999 em cada mil linhas do banco
/// respeitam. No jogo medido sao 31 mil linhas e o corte e seco: 99,96% tem
/// ate 20 caracteres. A traducao pode crescer, mas cada linha dela tem que
/// caber na caixa.
pub(crate) fn tag(entries: &mut [TextEntry], banks: &[Bank]) -> usize {
    let mut tagged = 0;
    for bank in banks {
        let mut inside = Vec::new();
        let mut lines: Vec<usize> = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            let Some(at) = entry.offset.map(|o| o as usize) else {
                continue;
            };
            let room = bank
                .room_at(at)
                .filter(|room| room.segment_of(at, entry.original_bytes.len()).is_some());
            if let Some(room) = room {
                inside.push((i, room.pairs[0]));
                lines.extend(
                    entry
                        .source_text
                        .split(['[', ']'])
                        .step_by(2)
                        .map(|line| line.chars().count())
                        .filter(|n| *n > 0),
                );
            }
        }
        lines.sort_unstable();
        // ponytail: com menos de mil linhas o corte de 99,9% e a maior linha.
        let line = lines
            .get((lines.len() * 999).div_ceil(1000).saturating_sub(1))
            .copied();
        for (i, pair) in inside {
            let entry = &mut entries[i];
            entry.metadata["bank"] = serde_json::json!({
                "at": bank.at,
                "pair": pair,
                "line": line,
            });
            entry.max_bytes = None;
            entry.context = Some(match line {
                Some(line) => format!(
                    "realocavel: a mensagem pode crescer (o bloco e remontado); cada linha \
                     cabe {line} caracteres na caixa de texto do jogo"
                ),
                None => "realocavel: a mensagem pode crescer (o bloco e remontado)".to_string(),
            });
            tagged += 1;
        }
    }
    tagged
}

/// A largura da caixa de texto guardada na entry (`tag`), se ha uma.
pub(crate) fn line_width(entry: &TextEntry) -> Option<usize> {
    let line = entry.metadata.get("bank")?.get("line")?.as_u64()?;
    Some(line as usize)
}

pub(crate) struct BankPlan {
    /// (posicao, bytes): blocos remontados, diretorio reapontado e o
    /// preenchimento do lugar dos blocos que sairam.
    pub writes: Vec<(usize, Vec<u8>)>,
    /// (posicao, bytes) de todo bloco de texto citado pelas entries: nenhuma
    /// outra escrita pode cair ai.
    pub rooms: Vec<(usize, usize)>,
    pub report: ApplyReport,
}

impl BankPlan {
    pub fn covers(&self, start: usize, len: usize) -> bool {
        let i = self.rooms.partition_point(|&(at, _)| at < start + len);
        i > 0 && self.rooms[i - 1].0 + self.rooms[i - 1].1 > start
    }
}

/// Um bloco remontado, a espera de lugar.
struct Rebuilt<'a> {
    bank: usize,
    room: &'a Room,
    /// Tabela nova seguida do texto novo.
    bytes: Vec<u8>,
    applied: usize,
}

/// Os trechos de preenchimento (0xFF) de `data` com `MIN_PADDING` ou mais.
fn padding(data: &[u8]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < data.len() {
        if data[at] != 0xFF {
            // Pula de 4 em 4 fora do preenchimento; volta ao byte no achado.
            at += 4;
            continue;
        }
        let mut begin = at;
        while begin > 0 && data[begin - 1] == 0xFF {
            begin -= 1;
        }
        let mut end = at;
        while end < data.len() && data[end] == 0xFF {
            end += 1;
        }
        // Margem nas pontas: o ultimo dado antes do preenchimento pode
        // terminar em 0xFF, e o primeiro depois pode comecar com um.
        let (from, to) = (
            (begin + 16).next_multiple_of(4),
            end.saturating_sub(16) & !3,
        );
        if end - begin >= MIN_PADDING && from < to {
            found.push((from, to - from));
        }
        at = end.next_multiple_of(4);
    }
    found
}

/// Tira de `free` um trecho de `len` bytes (multiplo de 4) depois de `floor`
/// e devolve onde ele comeca. O primeiro que servir.
fn allocate(free: &mut Vec<(usize, usize)>, len: usize, floor: usize) -> Option<usize> {
    let i = free
        .iter()
        .position(|&(at, room)| at >= floor && room >= len)?;
    let (at, room) = free[i];
    if room == len {
        free.remove(i);
    } else {
        free[i] = (at + len, room - len);
    }
    Some(at)
}

/// Devolve um trecho a `free`, colado nos vizinhos.
fn release(free: &mut Vec<(usize, usize)>, at: usize, len: usize) {
    free.push((at, len));
    free.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(free.len());
    for &(at, len) in free.iter() {
        match merged.last_mut() {
            Some(last) if last.0 + last.1 == at => last.1 += len,
            _ => merged.push((at, len)),
        }
    }
    *free = merged;
}

fn drifted(what: &str) -> CoreError {
    CoreError::Project(format!(
        "{what} — arquivo diferente do que foi extraido? Re-extraia antes de reinserir"
    ))
}

/// Remonta os blocos de banco que tem traducao. `data` e a imagem ORIGINAL;
/// entry que nao e de banco e ignorada. As traducoes chegam codificadas na
/// propria entry (`TextEntry::encoded`, posto por `reinsert`).
pub(crate) fn plan(data: &[u8], entries: &[TextEntry]) -> Result<BankPlan> {
    let mut plan = BankPlan {
        writes: Vec::new(),
        rooms: Vec::new(),
        report: ApplyReport::default(),
    };
    // (banco, par) -> entries daquele bloco.
    let mut groups: BTreeMap<(usize, usize), Vec<&TextEntry>> = BTreeMap::new();
    for entry in entries.iter().filter(|e| is_banked(e)) {
        let number = |key: &str| {
            entry.metadata["bank"]
                .get(key)?
                .as_u64()
                .map(|n| n as usize)
        };
        let (Some(at), Some(pair)) = (number("at"), number("pair")) else {
            continue;
        };
        groups.entry((at, pair)).or_default().push(entry);
    }
    if groups.is_empty() {
        return Ok(plan);
    }
    let banks: HashMap<usize, Bank> = find(data).into_iter().map(|b| (b.at, b)).collect();

    let mut rebuilt: Vec<Rebuilt> = Vec::new();
    for ((at, pair), mut group) in groups {
        let room = banks
            .get(&at)
            .and_then(|bank| bank.rooms.iter().find(|room| room.pairs.contains(&pair)))
            .filter(|room| !room.starts.is_empty())
            .ok_or_else(|| {
                drifted(&format!(
                    "o bloco de mensagens {pair} do banco em 0x{at:X} nao esta mais la"
                ))
            })?;
        plan.rooms.push((room.text, room.text_len));
        if group.iter().all(|e| e.translated_text.is_none()) {
            plan.report.kept_original += group.len();
            continue;
        }
        group.sort_by_key(|e| e.offset);

        // O texto novo, mensagem por mensagem, e onde cada uma comeca.
        let mut text = Vec::with_capacity(room.text_len);
        let mut moved: HashMap<usize, usize> = HashMap::new();
        let mut pending = group.iter().peekable();
        let mut applied = 0;
        for i in 0..room.starts.len() {
            let (begin, end) = room.segment(i);
            moved.insert(begin, text.len());
            let mut cursor = begin;
            while let Some(entry) = pending.peek() {
                let from = entry.offset.unwrap_or(0) as usize;
                let len = entry.original_bytes.len();
                let Some(rel) = from.checked_sub(room.text).filter(|rel| rel + len <= end) else {
                    break;
                };
                if rel < cursor || data[from..from + len] != entry.original_bytes[..] {
                    return Err(drifted(&format!(
                        "os bytes em 0x{from:X} nao batem com a entry {}",
                        entry.id
                    )));
                }
                text.extend_from_slice(&data[room.text + cursor..from]);
                match (&entry.translated_text, entry.encoded()) {
                    (Some(_), Some(bytes)) => {
                        text.extend_from_slice(&bytes);
                        applied += 1;
                    }
                    (Some(_), None) => {
                        return Err(CoreError::Project(format!(
                            "entry {}: a traducao chegou sem codificar — a reinsercao de \
                             mensagem de banco passa por reinsert_project, que le a tabela do \
                             projeto",
                            entry.id
                        )))
                    }
                    (None, _) => {
                        text.extend_from_slice(&entry.original_bytes);
                        plan.report.kept_original += 1;
                    }
                }
                cursor = rel + len;
                pending.next();
            }
            text.extend_from_slice(&data[room.text + cursor..room.text + end]);
        }
        if pending.next().is_some() {
            return Err(drifted(&format!(
                "uma entry do bloco de mensagens {pair} nao cabe em mensagem nenhuma dele"
            )));
        }

        // A tabela, com cada offset levado pro comeco novo da mensagem dele.
        let mut bytes = Vec::with_capacity(2 * room.slots + text.len() + 2);
        let mut fits = text.len() <= MAX_TEXT;
        for k in 0..room.slots {
            let old = u16_at(data, room.table + 2 * k).unwrap_or(END);
            let new = match moved.get(&(old as usize)) {
                _ if old == END => END,
                Some(&new) if new < END as usize => new as u16,
                _ => {
                    fits = false;
                    END
                }
            };
            bytes.extend_from_slice(&new.to_le_bytes());
        }
        if !fits {
            // Passou do que um offset de 16 bits alcanca: o bloco fica como
            // no original.
            plan.report.no_room += applied;
            continue;
        }
        bytes.extend_from_slice(&text);
        bytes.resize(bytes.len().next_multiple_of(4), 0xFF);
        rebuilt.push(Rebuilt {
            bank: at,
            room,
            bytes,
            applied,
        });
    }

    // Quem cabe onde estava fica; o resto disputa o espaco livre, os maiores
    // primeiro, e o lugar de quem sai vira espaco livre pros outros.
    let mut free = padding(data);
    let (stay, mut leave): (Vec<Rebuilt>, Vec<Rebuilt>) = rebuilt
        .into_iter()
        .partition(|block| block.bytes.len() <= block.room.size());
    for block in stay {
        let mut bytes = block.bytes;
        bytes.resize(block.room.size(), 0xFF);
        plan.writes.push((block.room.table, bytes));
        plan.report.applied += block.applied;
    }
    leave.sort_by_key(|block| std::cmp::Reverse(block.bytes.len()));
    let mut placed = true;
    while placed && !leave.is_empty() {
        placed = false;
        let mut waiting = Vec::new();
        for block in leave {
            let Some(at) = allocate(&mut free, block.bytes.len(), block.bank) else {
                waiting.push(block);
                continue;
            };
            placed = true;
            let room = block.room;
            let (table, text) = (at - block.bank, at + 2 * room.slots - block.bank);
            for &pair in &room.pairs {
                let mut entry = (table as u32).to_le_bytes().to_vec();
                entry.extend_from_slice(&(text as u32).to_le_bytes());
                plan.writes.push((block.bank + 4 + 8 * pair, entry));
            }
            // O lugar antigo e limpo agora e pode ser de outro bloco depois:
            // a ordem das escritas garante que o bloco novo fica por cima.
            plan.writes.push((room.table, vec![0xFF; room.size()]));
            plan.writes.push((at, block.bytes));
            release(&mut free, room.table, room.size() & !3);
            plan.report.applied += block.applied;
            plan.report.relocated += block.applied;
        }
        leave = waiting;
    }
    plan.report.no_room += leave.iter().map(|block| block.applied).sum::<usize>();
    plan.rooms.sort_unstable();
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_is_handed_out_and_taken_back() {
        let mut free = vec![(0x100, 0x40), (0x400, 0x100)];
        // O primeiro que serve, depois do piso.
        assert_eq!(allocate(&mut free, 0x80, 0), Some(0x400));
        assert_eq!(allocate(&mut free, 0x20, 0x200), Some(0x480));
        assert_eq!(allocate(&mut free, 0x40, 0), Some(0x100));
        assert_eq!(allocate(&mut free, 0x1000, 0), None);
        assert_eq!(free, [(0x4A0, 0x60)]);
        // O que volta cola no vizinho.
        release(&mut free, 0x480, 0x20);
        release(&mut free, 0x100, 0x40);
        assert_eq!(free, [(0x100, 0x40), (0x480, 0x80)]);
    }

    #[test]
    fn only_long_runs_of_ff_are_free_space() {
        let mut data = vec![0u8; 3 * MIN_PADDING];
        data[0x100..0x200].fill(0xFF); // curto: pode ser dado
        data[MIN_PADDING..2 * MIN_PADDING + 8].fill(0xFF);
        let free = padding(&data);
        assert_eq!(free.len(), 1);
        let (at, len) = free[0];
        assert!(at >= MIN_PADDING + 16 && at % 4 == 0);
        assert!(at + len <= 2 * MIN_PADDING + 8 - 16);
        assert!(padding(&[]).is_empty());
    }

    #[test]
    fn the_longest_line_ignores_the_codes() {
        assert_eq!(longest_line("abc[FF01]defgh[FF03][FF00]ij"), 5);
        assert_eq!(longest_line("[FF01]"), 0);
        assert_eq!(longest_line("sem codigo"), 10);
    }

    #[test]
    fn garbage_is_not_a_bank() {
        assert!(find(&[]).is_empty());
        assert!(find(&[0xFF; 4096]).is_empty());
        assert!(find(&crate::synth::make_random(64 * 1024, 11)).is_empty());
        // Um diretorio que promete mais do que o arquivo tem.
        let mut lying = vec![0u8; 256];
        lying[..4].copy_from_slice(&16u32.to_le_bytes());
        lying[4..8].copy_from_slice(&68u32.to_le_bytes());
        lying[8..12].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
        assert!(find(&lying).is_empty());
    }
}

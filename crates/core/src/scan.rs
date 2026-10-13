//! Camada A da extracao (spec §10): scanner generico de strings.
//! Serve para DESCOBERTA e debug — nao garante reinsercao segura (isso e papel
//! dos adapters estruturados, Camada B+). Deterministico: mesmo arquivo + mesma
//! config => mesmas entries, na mesma ordem.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::{CoreError, Result};
use crate::tbl::TblTable;
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

// ponytail: scan carrega o arquivo inteiro em memoria; 64 MiB cobre qualquer
// cartucho (GBA max 32 MiB). Scan streaming entra com plataformas de disco.
pub const MAX_SCAN_FILE_SIZE: u64 = 64 * 1024 * 1024;

/// Teto de entries de uma extracao (scanner e adapters). Era 20 mil. Medido em
/// 12/10/2026 com 100 mil entries: JSON de 28 MB, 0,2 s pra gravar no banco e
/// 0,2 s pra reler — a interface desenha 500 linhas por vez, entao o gargalo
/// nao e este numero. O jogo real medido tem ~18 mil strings depois do filtro
/// `only_text` e 187 mil sem ele.
pub const MAX_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanEncoding {
    Ascii,
    Utf8,
    Utf16Le,
    Utf16Be,
    Table,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanConfig {
    pub encoding: ScanEncoding,
    /// Obrigatorio quando `encoding == Table`.
    pub tbl_path: Option<PathBuf>,
    /// Minimo de CARACTERES (nao bytes) para virar entry.
    pub min_chars: usize,
    /// Regiao de busca em bytes; None = arquivo inteiro.
    pub region_start: Option<u64>,
    pub region_end: Option<u64>,
    /// Corte de seguranca para a UI nao afogar.
    pub max_entries: usize,
    /// Descarta o que nao parece texto (`looks_like_text`) ANTES de contar
    /// `max_entries`; o total descartado sai em `ScanOutcome::discarded`.
    /// Desligado por padrao: o scanner cru e a ferramenta de descoberta.
    pub only_text: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        ScanConfig {
            encoding: ScanEncoding::Ascii,
            tbl_path: None,
            min_chars: 4,
            region_start: None,
            region_end: None,
            max_entries: MAX_ENTRIES,
            only_text: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub entries: Vec<TextEntry>,
    /// true = parou em max_entries; ha mais strings alem das retornadas.
    pub truncated: bool,
    pub scanned_bytes: u64,
    /// Sequencias que o filtro `only_text` descartou, ate onde a varredura foi.
    #[serde(default)]
    pub discarded: usize,
    /// A tabela que este scan leu (encoding `Table`). Em `scan_project` e a
    /// copia do projeto — a que vale dali em diante.
    #[serde(default)]
    pub tbl_path: Option<PathBuf>,
}

pub fn scan_file(path: &Path, config: &ScanConfig) -> Result<ScanOutcome> {
    let size = fs::metadata(path)
        .map_err(|e| CoreError::io(path, e))?
        .len();
    if size > MAX_SCAN_FILE_SIZE {
        return Err(CoreError::FileTooLarge {
            size,
            limit: MAX_SCAN_FILE_SIZE,
        });
    }
    let data = fs::read(path).map_err(|e| CoreError::io(path, e))?;
    let outcome = scan_bytes(&data, config)?;
    info!(
        path = %path.display(),
        encoding = ?config.encoding,
        entries = outcome.entries.len(),
        discarded = outcome.discarded,
        truncated = outcome.truncated,
        "scan concluido"
    );
    Ok(outcome)
}

/// Scan do arquivo de origem de um projeto. A tabela `.tbl` escolhida entra
/// no projeto antes (`tbl::adopt_table`) e e lida de la: a reinsercao regrava
/// pela mesma tabela que leu.
pub fn scan_project(project_dir: &Path, config: &ScanConfig) -> Result<ScanOutcome> {
    let mut config = config.clone();
    if config.encoding == ScanEncoding::Table {
        if let Some(tbl_path) = config.tbl_path.take() {
            config.tbl_path = Some(crate::tbl::adopt_table(project_dir, &tbl_path)?);
        }
    }
    scan_file(
        &crate::project::load_project(project_dir)?.source_path,
        &config,
    )
}

pub fn scan_bytes(data: &[u8], config: &ScanConfig) -> Result<ScanOutcome> {
    let len = data.len() as u64;
    let start = config.region_start.unwrap_or(0).min(len) as usize;
    let end = config.region_end.unwrap_or(len).min(len) as usize;
    if start > end {
        return Err(CoreError::Project(format!(
            "regiao invalida: inicio 0x{start:X} > fim 0x{end:X}"
        )));
    }
    let window = &data[start..end];

    let table;
    let runs = match config.encoding {
        ScanEncoding::Ascii => ascii_runs(window),
        ScanEncoding::Utf8 => utf8_runs(window),
        ScanEncoding::Utf16Le => utf16_runs(window, true),
        ScanEncoding::Utf16Be => utf16_runs(window, false),
        ScanEncoding::Table => {
            let tbl_path = config.tbl_path.as_deref().ok_or_else(|| {
                CoreError::Tbl("encoding 'table' exige o caminho de um arquivo .tbl".to_string())
            })?;
            table = TblTable::load(tbl_path)?;
            table_runs(window, &table)
        }
    };

    let text_encoding = |cfg: &ScanConfig| match cfg.encoding {
        ScanEncoding::Ascii => TextEncoding::Ascii,
        ScanEncoding::Utf8 => TextEncoding::Utf8,
        ScanEncoding::Utf16Le => TextEncoding::Utf16Le,
        ScanEncoding::Utf16Be => TextEncoding::Utf16Be,
        ScanEncoding::Table => TextEncoding::Table(
            cfg.tbl_path
                .as_deref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "tbl".to_string()),
        ),
    };

    let mut entries = Vec::new();
    let mut truncated = false;
    let mut discarded = 0;
    for run in runs {
        if run.chars < config.min_chars {
            continue;
        }
        // Antes do teto: ele conta so o que sobra do filtro.
        if config.only_text && !looks_like_text(&run.text) {
            discarded += 1;
            continue;
        }
        if entries.len() >= config.max_entries {
            truncated = true;
            break;
        }
        let abs = start as u64 + run.offset as u64;
        // ponytail: a marca e refeita a cada extracao enquanto a entry nao
        // tem traducao — desfazer e extrair de novo marca outra vez. Guardar
        // a escolha do usuario fora de `entries` (como `accent_swaps`) se
        // isso incomodar.
        let status = if looks_like_identifier(&run.text) {
            TranslationStatus::Internal
        } else {
            TranslationStatus::Untranslated
        };
        entries.push(TextEntry {
            id: format!("scan-{abs:08x}"),
            resource_path: None,
            offset: Some(abs),
            original_bytes: window[run.offset..run.offset + run.byte_len].to_vec(),
            source_text: run.text,
            translated_text: None,
            context: None,
            // Scanner generico nao garante limite de reinsercao (Camada B decide).
            max_bytes: None,
            encoding: text_encoding(config),
            status,
            metadata: serde_json::json!({
                "scanner": config.encoding,
                "chars": run.chars,
                "terminated": run.terminated,
            }),
        });
    }

    Ok(ScanOutcome {
        entries,
        truncated,
        scanned_bytes: (end - start) as u64,
        discarded,
        tbl_path: match config.encoding {
            ScanEncoding::Table => config.tbl_path.clone(),
            _ => None,
        },
    })
}

/// Parece texto de jogo, e nao tile, codigo ou tabela que por acaso caiu em
/// byte imprimivel? Regra unica do core: o filtro do scanner, a extracao
/// estruturada e o Raio-X usam esta.
///
/// Calibrada em 12/10/2026 num jogo real de GBA (16 MiB, so contagens): das
/// 187.162 sequencias ASCII de 4+ caracteres passam 16.711, e de 2.398 frases
/// longas com palavras comuns do ingles passam 2.397. Cada condicao derruba um
/// tipo de lixo medido la:
/// - 70% de letras e espacos: tile e tabela misturam digito e simbolo;
/// - letras variadas e nenhuma passando de metade: "UUUUDDDD", "ffffwfff";
/// - caixa coerente por palavra (MAIUSCULA, minuscula ou Capitalizada): byte
///   solto alterna caixa ("wgUfVw"), palavra nao;
/// - 1 vogal a cada 8 letras: "wwgfvwgf" nao tem nenhuma.
///
/// String curta (ate 4) so passa se for uma palavra limpa — "SAVE", "Yes",
/// "No", "HP". Menu de jogo e feito disso, e exigir 5+ caracteres (a regra
/// que o Raio-X usava) apagaria todas.
///
/// E heuristica e erra: "McCoy" quebra a caixa, "Lv.99" e "HP: 12/30" tem
/// simbolo demais, "Nooooo!" repete demais. Por isso quem descarta com ela
/// diz quantas descartou, e string com ponteiro em tabela nem passa por ela
/// (`keep_text`): ponteiro e evidencia mais forte que aparencia. Letra aqui e
/// a do Unicode (kana e kanji contam); caixa e vogal so pesam no alfabeto
/// latino.
pub fn looks_like_text(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let letters = chars.iter().filter(|c| c.is_alphabetic()).count();
    let spaces = chars.iter().filter(|c| **c == ' ').count();
    if n < 2 || (letters + spaces) * 10 < n * 7 || (n <= 4 && letters != n) {
        return false;
    }

    let mut sorted = chars.clone();
    sorted.sort_unstable();
    let (mut distinct, mut top) = (0, 0);
    for same in sorted.chunk_by(|a, b| a == b) {
        distinct += 1;
        top = top.max(same.len());
    }
    if distinct < (n - 1).clamp(2, 4) || (n >= 5 && top * 2 > n) {
        return false;
    }

    let odd_case = chars.split(|c| !c.is_alphabetic()).any(|word| {
        let lower_tail = word.iter().skip(1).all(|c| !c.is_uppercase());
        let no_lower = word.iter().all(|c| !c.is_lowercase());
        !lower_tail && !no_lower
    });
    let ascii = chars.iter().filter(|c| c.is_ascii_alphabetic()).count();
    let vowels = chars
        .iter()
        .filter(|c| "aeiouyAEIOUY".contains(**c))
        .count();
    !odd_case && (ascii < 4 || vowels * 8 >= ascii)
}

/// E nome interno do jogo (sala, script, recurso), e nao texto que aparece na
/// tela? O scanner marca a entry como `TranslationStatus::Internal`: ela fica
/// na lista (pode ter ponteiro em tabela, e o Raio-X conta com ela), mas nao
/// vai pra traducao automatica nem e regravada.
///
/// A regra, sem contar espaco nas pontas (preenchimento de campo fixo):
/// - so maiusculas ASCII, digitos e `_`, com pelo menos uma letra;
/// - e pelo menos um `_` ou um digito: "FASE4_31_01", "VILA_SET_1", "SALA12".
///   Sem isso e palavra de menu ("START", "OPTIONS"), e com espaco no meio e
///   frase ("NEW GAME", "GAME OVER") — texto, nos dois casos;
/// - minuscula so num pedaco entre `_` que tambem tem digito ("TORRE_0d_04",
///   "PONTE_14_R0f"): e numero em hexadecimal, nao palavra.
///
/// Calibrada em 12/10/2026 no mesmo jogo real de GBA de `looks_like_text` (so
/// contagens): das 17.936 strings extraidas, 2.326 batem — 1.596 com `_`
/// (1.564 com ponteiro em tabela) e 730 so com digito colado, que la sao
/// tile que passou no filtro, nao nome; das 1.590 com ponteiro ficam de fora
/// 25 — 21 sao texto (palavras de menu, nomes de idioma) e 4, lixo. Sem a
/// regra da minuscula e a do espaco nas pontas, 116 nomes com ponteiro
/// continuavam indo pra traducao.
///
/// E heuristica e erra pro outro lado tambem: "PLAYER1", "STAGE1" e "1UP"
/// sao texto de jogo e batem na regra. Por isso a marca aparece na interface,
/// contada, e se desfaz por entry (`validate::unmark_internal`).
pub fn looks_like_identifier(text: &str) -> bool {
    let name = text.trim_matches(' ');
    let joined = name.contains('_');
    name.split('_').all(|piece| {
        piece
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            || (joined
                && piece.chars().all(|c| c.is_ascii_alphanumeric())
                && piece.chars().any(|c| c.is_ascii_digit()))
    }) && name.chars().any(|c| c.is_ascii_alphabetic())
        && name.chars().any(|c| c == '_' || c.is_ascii_digit())
}

/// Tira de `entries` o que nao parece texto e devolve quantas sairam. Entry
/// com ponteiro em tabela fica sempre. E o filtro dos adapters que so acham
/// os ponteiros depois de varrer; quem nao tem ponteiro usa `only_text`.
pub fn keep_text(entries: &mut Vec<TextEntry>) -> usize {
    let before = entries.len();
    entries.retain(|e| !e.pointers().is_empty() || looks_like_text(&e.source_text));
    before - entries.len()
}

/// Ids das entries SOLTAS: as que ficam fora das regioes onde o arquivo guarda
/// o texto do jogo. Num cartucho, quase tudo o que passa em `looks_like_text`
/// fora do banco de texto e dado de grafico, de som ou de tabela — e traducao
/// gravada ali corrompe o jogo. Medido em 13/10/2026 num jogo real de GBA de
/// 16 MiB: de 15.610 entries fora dos nomes internos, 4.415 estavam fora do
/// banco de texto (308 KiB contiguos) e nenhuma delas era fala.
///
/// A regra: o arquivo e cortado em blocos e conta-se, em cada um, quantas
/// entries tem cara de frase (12+ caracteres, com espaco). Bloco com pelo
/// menos 1/8 do bloco mais cheio e regiao de texto; os vizinhos dela tambem
/// valem, porque um banco comeca e termina no meio de um bloco. Fica de fora:
/// entry com arquivo proprio (disco, sistema de arquivos), nome interno e
/// entry com ponteiro em tabela — ponteiro e evidencia mais forte que posicao.
///
/// So age quando o arquivo tem um banco nitido (100+ frases e 70% de todas as
/// frases dentro das regioes). Sem isso devolve vazio: jogo pequeno ou com o
/// texto espalhado fica como esta. E heuristica, calibrada num jogo so.
pub fn stray_ids(entries: &[TextEntry]) -> Vec<String> {
    const BANK_SENTENCES: usize = 100;

    let candidates: Vec<(&TextEntry, u64)> = entries
        .iter()
        .filter(|e| e.resource_path.is_none() && e.status != TranslationStatus::Internal)
        .filter_map(|e| e.offset.map(|offset| (e, offset)))
        .collect();
    let Some(top) = candidates.iter().map(|(_, offset)| *offset).max() else {
        return Vec::new();
    };
    // ponytail: tres tamanhos de bloco pelo tamanho do arquivo. Banco menor
    // que um bloco ainda aparece (o bloco dele e o mais cheio); dois bancos
    // pequenos e distantes num arquivo grande podem ficar abaixo do corte —
    // medir num jogo assim antes de trocar por agrupamento de verdade.
    let block: u64 = match top {
        t if t >= 4 << 20 => 64 << 10,
        t if t >= 512 << 10 => 16 << 10,
        _ => 4 << 10,
    };
    let mut sentences: HashMap<u64, usize> = HashMap::new();
    for (entry, offset) in &candidates {
        if is_sentence(&entry.source_text) {
            *sentences.entry(offset / block).or_default() += 1;
        }
    }
    let fullest = sentences.values().copied().max().unwrap_or(0);
    let dense: HashSet<u64> = sentences
        .iter()
        .filter(|(_, n)| **n >= (fullest / 8).max(SENTENCE_CHARS))
        .map(|(block, _)| *block)
        .collect();
    let inside: usize = dense.iter().map(|block| sentences[block]).sum();
    let all: usize = sentences.values().sum();
    if inside < BANK_SENTENCES || inside * 10 < all * 7 {
        return Vec::new();
    }
    let near_text = |b: u64| {
        dense.contains(&b)
            || dense.contains(&(b + 1))
            || b.checked_sub(1).is_some_and(|p| dense.contains(&p))
    };
    candidates
        .into_iter()
        .filter(|(entry, offset)| !near_text(offset / block) && entry.pointers().is_empty())
        .map(|(entry, _)| entry.id.clone())
        .collect()
}

const SENTENCE_CHARS: usize = 12;

/// Tem cara de frase: 12+ caracteres, com espaco no meio.
pub(crate) fn is_sentence(text: &str) -> bool {
    text.chars().count() >= SENTENCE_CHARS && text.trim().contains(' ')
}

/// Run bruto achado por um scanner, relativo ao inicio da janela.
struct Run {
    offset: usize,
    byte_len: usize,
    chars: usize,
    text: String,
    /// true se o byte seguinte ao run e 0x00 (terminador classico).
    terminated: bool,
}

fn is_ascii_printable(b: u8) -> bool {
    (0x20..0x7F).contains(&b)
}

/// Texto "plausivel" para scanners unicode: nada de controle, nada de
/// private-use (lixo comum em falso positivo), nada de replacement char e
/// nenhum noncharacter (U+FDD0..=U+FDEF e os xxFFFE/xxFFFF de cada plano —
/// padding 0xFFFF de ROM cai aqui).
fn is_text_char(c: char) -> bool {
    let cp = c as u32;
    !c.is_control()
        && c != '\u{FFFD}'
        && !('\u{E000}'..='\u{F8FF}').contains(&c)
        && !(0xFDD0..=0xFDEF).contains(&cp)
        && (cp & 0xFFFE) != 0xFFFE
}

/// So o texto dos runs ASCII — pra quem quer contar, nao reinserir.
pub(crate) fn ascii_texts(data: &[u8]) -> Vec<String> {
    ascii_runs(data).into_iter().map(|run| run.text).collect()
}

fn ascii_runs(data: &[u8]) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if !is_ascii_printable(data[i]) {
            i += 1;
            continue;
        }
        let begin = i;
        while i < data.len() && is_ascii_printable(data[i]) {
            i += 1;
        }
        let bytes = &data[begin..i];
        runs.push(Run {
            offset: begin,
            byte_len: bytes.len(),
            chars: bytes.len(),
            text: String::from_utf8_lossy(bytes).into_owned(),
            terminated: data.get(i) == Some(&0),
        });
    }
    runs
}

/// Decodifica um char UTF-8 valido em `data[i..]` (from_utf8 rejeita overlong
/// e surrogates por nos).
fn decode_utf8_char(data: &[u8], i: usize) -> Option<(char, usize)> {
    let first = *data.get(i)?;
    let len = match first {
        0x20..=0x7E => 1, // ASCII printable direto; controles nao comecam run
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return None,
    };
    let slice = data.get(i..i + len)?;
    let c = std::str::from_utf8(slice).ok()?.chars().next()?;
    is_text_char(c).then_some((c, len))
}

fn utf8_runs(data: &[u8]) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let Some((c, clen)) = decode_utf8_char(data, i) else {
            i += 1;
            continue;
        };
        let begin = i;
        let mut text = String::new();
        let mut chars = 0;
        let mut cur = (c, clen);
        loop {
            text.push(cur.0);
            chars += 1;
            i += cur.1;
            match decode_utf8_char(data, i) {
                Some(next) => cur = next,
                None => break,
            }
        }
        runs.push(Run {
            offset: begin,
            byte_len: i - begin,
            chars,
            text,
            terminated: data.get(i) == Some(&0),
        });
    }
    runs
}

fn utf16_runs(data: &[u8], le: bool) -> Vec<Run> {
    let unit = |i: usize| -> Option<u16> {
        let a = *data.get(i)?;
        let b = *data.get(i + 1)?;
        Some(if le {
            u16::from_le_bytes([a, b])
        } else {
            u16::from_be_bytes([a, b])
        })
    };
    // Decodifica um char (com par surrogate) em unidades de 2 bytes.
    let decode = |i: usize| -> Option<(char, usize)> {
        let u = unit(i)?;
        match u {
            0xD800..=0xDBFF => {
                let lo = unit(i + 2)?;
                if !(0xDC00..=0xDFFF).contains(&lo) {
                    return None;
                }
                let cp = 0x10000 + (((u as u32 - 0xD800) << 10) | (lo as u32 - 0xDC00));
                char::from_u32(cp)
                    .filter(|c| is_text_char(*c))
                    .map(|c| (c, 4))
            }
            0xDC00..=0xDFFF => None, // low surrogate solto
            _ => char::from_u32(u as u32)
                .filter(|c| is_text_char(*c))
                .map(|c| (c, 2)),
        }
    };

    let mut runs = Vec::new();
    let mut i = 0;
    // Alinhamento fixo em offsets pares — strings UTF-16 em ROMs sao alinhadas.
    while i + 1 < data.len() {
        let Some((c, clen)) = decode(i) else {
            i += 2;
            continue;
        };
        let begin = i;
        let mut text = String::new();
        let mut chars = 0;
        let mut cur = (c, clen);
        loop {
            text.push(cur.0);
            chars += 1;
            i += cur.1;
            match decode(i) {
                Some(next) => cur = next,
                None => break,
            }
        }
        runs.push(Run {
            offset: begin,
            byte_len: i - begin,
            chars,
            text,
            terminated: unit(i) == Some(0),
        });
    }
    runs
}

fn table_runs(data: &[u8], table: &TblTable) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let Some((klen, piece)) = table.lookup(&data[i..]) else {
            i += 1;
            continue;
        };
        let begin = i;
        let mut text = String::new();
        let mut cur = (klen, piece);
        loop {
            text.push_str(cur.1);
            i += cur.0;
            match table.lookup(&data[i..]) {
                Some(next) => cur = next,
                None => break,
            }
        }
        runs.push(Run {
            offset: begin,
            byte_len: i - begin,
            chars: text.chars().count(),
            text,
            terminated: data.get(i) == Some(&0),
        });
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_keeps_phrases_and_menu_words_and_drops_tile_like_runs() {
        for text in [
            "Welcome to the village",
            "GAME OVER",
            "You got the Flame Lantern!",
            "SAVE",
            "ITEM",
            "Yes",
            "No",
            "HP",
            "あいうえお",
        ] {
            assert!(looks_like_text(text), "{text}");
        }
        for text in [
            "3333DDDD3333",     // tile 4bpp imprimivel
            "\"\"\"\"\"\"\"\"", // um caractere so
            "UUUUUUUUfVUE",     // um caractere passa de metade
            "wgUfVwEd",         // caixa alternando dentro da palavra
            "wwgfvwgf",         // nenhuma vogal
            "x8Fp",             // curta com digito
            "DDDD",             // curta sem variedade
            "Hi!",              // curta com pontuacao
            "MAPA_07_A1",       // identificador: so fica se tiver ponteiro
            "A",
            "",
        ] {
            assert!(!looks_like_text(text), "{text}");
        }
    }

    #[test]
    fn identifier_needs_an_underscore_or_a_digit_and_no_inner_space() {
        for text in [
            "FASE4_31_01",
            "VILA_SET_1",
            "SALA12",         // digito colado, sem sublinhado
            "LV01",           //
            "TORRE_0d_04",    // hexadecimal em minuscula num pedaco com digito
            "PONTE_14_R0f",   //
            "FASE1_10_TOPO ", // espaco de preenchimento na ponta
        ] {
            assert!(looks_like_identifier(text), "{text}");
        }
        for text in [
            "NEW GAME",       // frase em maiusculas
            "GAME OVER",      //
            "START",          // palavra de menu
            "OPTIONS",        //
            "  FRENCH      ", // palavra de menu com preenchimento
            "Lv1",            // minuscula sem sublinhado e texto
            "The_old_bridge", // minuscula em pedaco sem digito
            "player_name",    //
            "HP {0}: 120",    //
            "SAVE 1",         // espaco no meio
            "1234",           // sem letra
            "____",           //
            "",
        ] {
            assert!(!looks_like_identifier(text), "{text}");
        }
    }

    #[test]
    fn scanner_marks_internal_names_and_leaves_menu_words_alone() {
        let mut data = vec![0u8; 0x80];
        for (at, text) in [(0x00, "VILA_SET_1"), (0x20, "NEW GAME"), (0x40, "START")] {
            data[at..at + text.len()].copy_from_slice(text.as_bytes());
        }
        let found: Vec<(String, TranslationStatus)> = scan_bytes(&data, &ScanConfig::default())
            .unwrap()
            .entries
            .into_iter()
            .map(|e| (e.source_text, e.status))
            .collect();
        assert_eq!(
            found,
            [
                ("VILA_SET_1".to_string(), TranslationStatus::Internal),
                ("NEW GAME".to_string(), TranslationStatus::Untranslated),
                ("START".to_string(), TranslationStatus::Untranslated),
            ]
        );
    }

    /// Arquivo de 1 MiB com um banco de 150 frases a partir de 512 KiB e
    /// sequencias curtas espalhadas pelo resto, como grafico que caiu em
    /// byte imprimivel.
    fn rom_with_a_text_bank() -> Vec<u8> {
        let mut data = vec![0u8; 1 << 20];
        let mut at = 0x80000;
        for i in 0..150 {
            let line = format!("The old bridge fell again, try number {i}.");
            data[at..at + line.len()].copy_from_slice(line.as_bytes());
            at += line.len() + 1;
        }
        // Sem digito nem sublinhado: com eles seria nome interno, que a regra
        // nem olha.
        for (i, at) in (0x1000..0x70000).step_by(0x3000).enumerate() {
            let junk = format!("Grid{}", (b'a' + (i % 26) as u8) as char);
            data[at..at + junk.len()].copy_from_slice(junk.as_bytes());
        }
        // Uma frase sozinha, longe do banco: tambem e solta.
        let lone = b"This line sits alone, far away.";
        data[0x20000..0x20000 + lone.len()].copy_from_slice(lone);
        data
    }

    #[test]
    fn entries_outside_the_text_bank_are_stray_and_the_bank_is_not() {
        let entries = scan_bytes(&rom_with_a_text_bank(), &ScanConfig::default())
            .unwrap()
            .entries;
        let stray = stray_ids(&entries);

        let in_bank = |e: &&TextEntry| e.offset.is_some_and(|o| o >= 0x80000);
        let bank: Vec<&TextEntry> = entries.iter().filter(in_bank).collect();
        assert_eq!(bank.len(), 150);
        assert!(bank.iter().all(|e| !stray.contains(&e.id)));
        // Tudo o que esta fora do banco saiu: o lixo curto e a frase sozinha.
        let outside = entries.len() - bank.len();
        assert!(outside > 30, "a fixture precisa de lixo espalhado");
        assert_eq!(stray.len(), outside);
    }

    #[test]
    fn pointer_internal_name_and_file_entries_are_never_stray() {
        let mut entries = scan_bytes(&rom_with_a_text_bank(), &ScanConfig::default())
            .unwrap()
            .entries;
        let far: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.offset.is_some_and(|o| o < 0x70000))
            .map(|(i, _)| i)
            .take(3)
            .collect();
        entries[far[0]].metadata = serde_json::json!({ "pointers": [0x40] });
        entries[far[1]].status = TranslationStatus::Internal;
        entries[far[2]].resource_path = Some("DATA/TEXT.BIN".to_string());
        assert!(
            !entries[far[0]].pointers().is_empty(),
            "o ponteiro tem que ser lido"
        );
        let kept: Vec<String> = far.iter().map(|i| entries[*i].id.clone()).collect();

        let stray = stray_ids(&entries);

        assert!(kept.iter().all(|id| !stray.contains(id)));
        assert!(!stray.is_empty());
    }

    #[test]
    fn file_without_a_clear_text_bank_has_no_stray_entries() {
        // Poucas frases, espalhadas: nao ha banco, a regra nao age.
        let mut data = vec![0u8; 1 << 20];
        for (i, at) in (0x1000..0xF0000).step_by(0x8000).enumerate() {
            let line = format!("A short line of text number {i}.");
            data[at..at + line.len()].copy_from_slice(line.as_bytes());
        }
        let entries = scan_bytes(&data, &ScanConfig::default()).unwrap().entries;
        assert!(entries.len() >= 20);
        assert!(stray_ids(&entries).is_empty());
    }
}

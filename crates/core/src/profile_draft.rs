//! Assistente do perfil do jogo (`romtranslate perfil-sugerir`): com uma
//! palavra que aparece na tela, propoe a tabela de letras (busca relativa),
//! o cabecalho, o terminador e o alinhamento mais comuns nas frases em volta
//! dela, e acha os ponteiros de 32 bits pra essas frases nas bases comuns —
//! cartucho de GBA em `0x08000000`, modulo de DS (ARM9, ARM7, overlay) no
//! endereco de carga. Escreve um `perfil.toml` de rascunho, com comentarios
//! dizendo o que conferir, e le o jogo com ele pra dizer quantas frases deu.
//!
//! E heuristica, feita pra quem estudou o jogo (pessoa ou IA) so confirmar:
//! codigo com parametro, quebra de linha e fim de pagina ficam listados, nao
//! declarados.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{CoreError, Result};
use crate::profile::{Pointers, Profile, Source};
use crate::tbl::{TblTable, TABLES_DIR};
use crate::types::Platform;

/// Id da tabela que o assistente escreve (`tables/perfil-letras.tbl`).
pub const TABLE_ID: &str = "perfil-letras";
/// Ate onde, antes e depois da palavra, procurar o comeco da frase dela e o
/// da seguinte.
const BACK: usize = 512;
const AHEAD: usize = 1024;
/// Ocorrencias da palavra usadas: o bastante pra votar.
const MAX_HITS: usize = 64;
/// Em volta das frases achadas, quanto o rascunho le pra contar o banco.
const AROUND: usize = 64 * 1024;
/// Fracao dos votos que um cabecalho ou terminador precisa ter.
const QUORUM_PERCENT: usize = 60;

/// O que o assistente propos e o que conferir.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// O perfil escrito: `perfil.toml`, ou `perfil-rascunho.toml` se o
    /// projeto ja tem um perfil (nunca e sobrescrito).
    pub path: PathBuf,
    pub table_path: PathBuf,
    /// `false` = a tabela ja existia e foi mantida (pode ter sido completada
    /// a mao).
    pub table_created: bool,
    /// `Some(n)` = letra = ASCII com `n` a menos (0 = ASCII).
    pub ascii_shift: Option<i32>,
    /// Ocorrencias da palavra no arquivo.
    pub hits: usize,
    pub module: String,
    pub memory: u32,
    pub origin: u64,
    pub header: String,
    pub terminator: String,
    pub align: usize,
    pub start: u64,
    pub end: u64,
    pub search: Vec<[u64; 2]>,
    /// Comecos de frase, em volta da palavra, com ponteiro.
    pub pointed: usize,
    /// O que o rascunho le do jogo.
    pub sentences: usize,
    pub discarded: usize,
    pub sample: Vec<String>,
    /// Bytes fora da tabela nas frases lidas, com quantas vezes aparecem.
    pub codes: Vec<(String, usize)>,
    /// O que conferir antes de traduzir.
    pub check: Vec<String>,
}

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("perfil-sugerir: {}", msg.into()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// `.tbl` do ASCII deslocado: cada caractere imprimivel com `shift` a menos.
fn shifted_ascii(shift: i32) -> String {
    let mut out = format!(
        "# Tabela do assistente de perfil: ASCII com 0x{shift:X} a menos (busca relativa).\n\
         # Confira acentos e pontuacao na fonte do jogo; acrescente o que faltar.\n"
    );
    for c in 0x20u8..=0x7E {
        if let Ok(b) = u8::try_from(c as i32 - shift) {
            let _ = writeln!(out, "{b:02X}={}", c as char);
        }
    }
    out
}

/// Os trechos do arquivo que o console carrega num endereco fixo:
/// (nome, inicio, fim, endereco).
fn modules(platform: Platform, data: &[u8]) -> Result<Vec<crate::adapters::nds::Module>> {
    match platform {
        Platform::Gba => Ok(vec![("cartucho".to_string(), 0, data.len(), 0x0800_0000)]),
        Platform::Nds => Ok(crate::adapters::nds::modules(data)),
        other => Err(err(format!(
            "o assistente conhece os ponteiros de GBA e de DS, nao de {}",
            other.display_name()
        ))),
    }
}

/// Os valores votados, do mais pro menos votado (empate: o menor valor).
fn ranked<T: Ord + Clone>(votes: &[T]) -> Vec<(T, usize)> {
    let mut counts: BTreeMap<T, usize> = BTreeMap::new();
    for v in votes {
        *counts.entry(v.clone()).or_default() += 1;
    }
    let mut out: Vec<(T, usize)> = counts.into_iter().collect();
    out.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    out
}

/// O valor mais votado, se ele tem quorum.
fn winner<T: Ord + Clone>(votes: &[T]) -> Option<(T, usize)> {
    let (value, n) = ranked(votes).into_iter().next()?;
    (n * 100 >= votes.len() * QUORUM_PERCENT).then_some((value, n))
}

/// Monta, grava e confere o rascunho do perfil do projeto a partir de
/// `word`.
pub fn suggest(project_dir: &Path, word: &str) -> Result<Draft> {
    let project = crate::project::load_project(project_dir)?;
    let data = crate::fileio::read_view(&project.source_path)?;
    let data: &[u8] = &data;
    let mods = modules(project.platform, data)?;
    let mut check = Vec::new();

    // 1. Tabela: a busca relativa; maiuscula e minuscula a 0x20 uma da outra
    //    e ASCII deslocado, e a tabela leva o ASCII imprimivel inteiro.
    let best = crate::relative::relative_search(data, word)?
        .into_iter()
        .next()
        .ok_or_else(|| {
            err(format!(
                "\"{word}\" nao aparece com as letras em ordem alfabetica: tente outra \
                 palavra (com maiuscula e minuscula, ex.: o nome do heroi)"
            ))
        })?;
    let ascii_shift = match (best.upper_base, best.lower_base) {
        (Some(u), Some(l)) if l.checked_sub(u) == Some(0x20) => Some(0x41 - u as i32),
        _ => None,
    };
    let table_text = match ascii_shift {
        Some(shift) => shifted_ascii(shift),
        None => {
            check.push(
                "a tabela tem so as letras (e o espaco, se deu pra inferir): complete numeros e \
                 pontuacao"
                    .to_string(),
            );
            crate::relative::build_tbl(data, best.upper_base, best.lower_base)?
        }
    };
    let table_path = project_dir.join(TABLES_DIR).join(format!("{TABLE_ID}.tbl"));
    let table_created = !table_path.exists();
    if table_created {
        let dir = project_dir.join(TABLES_DIR);
        fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
        fs::write(&table_path, &table_text).map_err(|e| CoreError::io(&table_path, e))?;
    }
    let table = TblTable::load(&table_path)?;

    // 2. Onde a palavra esta, lida pela tabela.
    let needle = table.encode(word)?;
    let hits: Vec<usize> = data
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle.as_slice())
        .map(|(at, _)| at)
        .take(MAX_HITS)
        .collect();
    let module_of = |at: usize| mods.iter().find(|m| (m.1..m.2).contains(&at));

    // 3. Ponteiros: o endereco de memoria de cada posicao em volta da
    //    palavra, procurado como palavra de 32 bits alinhada no arquivo todo.
    let mut wanted: HashMap<u32, Vec<usize>> = HashMap::new();
    for &at in &hits {
        let Some(m) = module_of(at) else { continue };
        for s in at.saturating_sub(BACK).max(m.1)..(at + AHEAD).min(m.2) {
            if let Some(address) = u32::try_from(s - m.1).ok().and_then(|r| m.3.checked_add(r)) {
                wanted.entry(address).or_default().push(s);
            }
        }
    }
    if wanted.is_empty() {
        return Err(err(format!(
            "\"{word}\" aparece {} vez(es), mas fora de qualquer trecho com endereco de \
             memoria conhecido (no DS, texto em arquivo do sistema de arquivos nao tem \
             ponteiro de memoria). Tabela em {}",
            hits.len(),
            table_path.display()
        )));
    }
    let mut pointers: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (k, word) in data.as_chunks::<4>().0.iter().enumerate() {
        let value = u32::from_le_bytes(*word);
        for &s in wanted.get(&value).into_iter().flatten() {
            pointers.entry(s).or_default().push(k * 4);
        }
    }

    if pointers.is_empty() {
        return Err(err(format!(
            "\"{word}\" aparece {} vez(es), mas nenhum ponteiro de 32 bits leva as frases em \
             volta dela (bases: GBA 0x08000000, modulos do DS pelo endereco de carga). O jogo \
             pode usar outro tipo de ponteiro ou o texto estar comprimido. Tabela em {}",
            hits.len(),
            table_path.display()
        )));
    }

    // 4. Cabecalho: entre todos os comecos com ponteiro em volta da palavra
    //    — frase e ponteiro de codigo que caiu perto —, o primeiro byte mais
    //    comum, que cresce enquanto o prefixo maior mantem 9 em 10 deles.
    //    Medido em 19/10/2026 num jogo de DS: 611 de 1.458 comecos eram frase.
    let all: Vec<usize> = pointers.keys().copied().collect();
    let top = |len: usize| {
        let votes: Vec<&[u8]> = all.iter().filter_map(|&s| data.get(s..s + len)).collect();
        ranked(&votes)
    };
    let mut header: Vec<u8> = Vec::new();
    let first = top(1);
    if let Some((byte, n)) = first.first() {
        let second = first.get(1).map_or(0, |(_, n)| *n);
        if *n >= 3.max(all.len() / 5) && *n >= 2 * second {
            header = byte.to_vec();
            let mut support = *n;
            for len in 2..=4 {
                match top(len).first() {
                    Some((prefix, n)) if n * 10 >= support * 9 && prefix.starts_with(&header) => {
                        header = prefix.to_vec();
                        support = *n;
                    }
                    _ => break,
                }
            }
        }
    }
    if header.is_empty() {
        check.push(
            "nenhum comeco comum nas frases com ponteiro: o rascunho vai sem cabecalho".to_string(),
        );
    }

    // 5. A frase de cada ocorrencia: o comeco com ponteiro e cabecalho mais
    //    perto antes dela; e o da frase seguinte.
    let heads: std::collections::BTreeSet<usize> = all
        .iter()
        .copied()
        .filter(|&s| data[s..].starts_with(&header))
        .collect();
    let mut phrases: Vec<(usize, usize, Option<usize>)> = Vec::new();
    for &at in &hits {
        let Some(&start) = heads
            .range(at.saturating_sub(BACK)..=at.saturating_sub(header.len()))
            .next_back()
        else {
            continue;
        };
        let next = heads.range(at + 1..at + AHEAD).next().copied();
        phrases.push((at, start, next));
    }
    if phrases.is_empty() {
        return Err(err(format!(
            "\"{word}\" aparece {} vez(es), mas nenhuma frase com ponteiro comeca antes dela. \
             Tabela em {}",
            hits.len(),
            table_path.display()
        )));
    }
    let mut starts: Vec<usize> = phrases
        .iter()
        .flat_map(|&(_, s, n)| std::iter::once(s).chain(n))
        .collect();
    starts.sort_unstable();
    starts.dedup();

    // 6. Terminador: o ultimo byte da frase antes da seguinte, tirado o
    //    preenchimento de zeros; letra ali e a string de C (fecha em 00).
    let letter = |b: u8| table.lookup(&[b]).is_some();
    let lasts: Vec<u8> = phrases
        .iter()
        .filter_map(|&(_, s, n)| {
            let span = data.get(s..n?)?;
            let i = span.iter().rposition(|&b| b != 0)?;
            Some(match span[i] {
                b if i + 1 < span.len() && letter(b) => 0,
                b if i + 1 < span.len() => b,
                _ => span[span.len() - 1],
            })
        })
        .collect();
    let terminator = match winner(&lasts) {
        Some((b, _)) => vec![b],
        _ => {
            check.push("terminador nao achado com certeza: o rascunho usa 00".to_string());
            vec![0]
        }
    };

    // 7. Alinhamento: o maior que 9 em 10 comecos respeitam.
    let align = [4, 2, 1]
        .into_iter()
        .find(|a| starts.iter().filter(|s| *s % a == 0).count() * 10 >= starts.len() * 9)
        .unwrap_or(1);

    // 8. O modulo das frases e onde os ponteiros delas estao.
    let mut votes: Vec<usize> = Vec::new();
    for &(_, s, _) in &phrases {
        if let Some(i) = mods.iter().position(|m| (m.1..m.2).contains(&s)) {
            votes.push(i);
        }
    }
    let home = &mods[winner(&votes).map_or(votes[0], |(i, _)| i)];
    // So dentro de modulo: palavra de um arquivo de dados que por acaso tem o
    // endereco de uma frase seria reescrita quando ela mudasse de lugar. No
    // jogo de DS de 19/10, 9 blocos de dados tinham um "ponteiro" assim.
    let mut search: Vec<[u64; 2]> = Vec::new();
    let mut loose = 0;
    for k in starts.iter().filter_map(|s| pointers.get(s)).flatten() {
        let Some(m) = module_of(*k) else {
            loose += 1;
            continue;
        };
        if !search.contains(&[m.1 as u64, m.2 as u64]) {
            search.push([m.1 as u64, m.2 as u64]);
        }
    }
    search.sort_unstable();
    if loose > 0 {
        check.push(format!(
            "{loose} ponteiro(s) fora dos modulos ficaram de fora de `procurar` (num arquivo de \
             dados costumam ser coincidencia)"
        ));
    }

    // 9. Le o jogo com o rascunho, em volta das frases achadas, e estreita a
    //    faixa pro que ele leu.
    let window = (
        starts[0].saturating_sub(AROUND).max(home.1),
        (starts[starts.len() - 1] + AROUND).min(home.2),
    );
    let mut source = Source {
        nome: home.0.clone(),
        tabela: TABLE_ID.to_string(),
        inicio: window.0 as u64,
        fim: window.1 as u64,
        cabecalho: hex(&header),
        terminador: hex(&terminator),
        alinhamento: align,
        parametros: BTreeMap::new(),
        quebra: None,
        pagina: None,
        ponteiros: Pointers {
            memoria: home.3,
            origem: home.1 as u64,
            procurar: search.clone(),
        },
    };
    let read = crate::profile::reread(
        project_dir,
        data,
        &Profile {
            texto: vec![source.clone()],
        },
    )?;
    let begin = read
        .entries
        .iter()
        .filter_map(|e| e.offset.map(|o| o as usize - header.len()))
        .min();
    let finish = read
        .entries
        .iter()
        .filter_map(|e| e.offset.map(|o| o as usize + e.original_bytes.len()))
        .max();
    if let (Some(begin), Some(finish)) = (begin, finish) {
        source.inicio = begin as u64;
        source.fim = (finish + terminator.len()).next_multiple_of(align) as u64;
        if begin <= window.0 + align || finish + AROUND / 16 >= window.1 {
            check.push(
                "o banco pode continuar alem da faixa: estenda inicio/fim ate o primeiro e o \
                 ultimo cabecalho com ponteiro e extraia de novo"
                    .to_string(),
            );
        }
    }
    let mut codes: BTreeMap<String, usize> = BTreeMap::new();
    for e in &read.entries {
        let mut rest = e.source_text.as_str();
        while let Some(open) = rest.find('[') {
            rest = &rest[open + 1..];
            if let Some(code) = rest
                .get(..2)
                .filter(|c| c.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                *codes.entry(code.to_string()).or_default() += 1;
            }
        }
    }
    let mut codes: Vec<(String, usize)> = codes.into_iter().collect();
    codes.sort_by_key(|(code, n)| (std::cmp::Reverse(*n), code.clone()));
    if !codes.is_empty() {
        check.push(
            "declare quebra (codigo que quebra a linha), pagina (o que espera o botao) e \
             parametros (codigo seguido de bytes que nao sao letra) entre os codigos achados"
                .to_string(),
        );
    }
    if read.entries.is_empty() {
        check.push("o rascunho nao leu nenhuma frase: confira cabecalho e terminador".to_string());
    }

    let mut draft = Draft {
        path: PathBuf::new(),
        table_path,
        table_created,
        ascii_shift,
        hits: hits.len(),
        module: source.nome.clone(),
        memory: home.3,
        origin: home.1 as u64,
        header: source.cabecalho.clone(),
        terminator: source.terminador.clone(),
        align,
        start: source.inicio,
        end: source.fim,
        search,
        pointed: starts.len(),
        sentences: read.entries.len(),
        discarded: read.discarded,
        // As frases com a palavra primeiro: e nelas que se ve se a leitura
        // esta certa.
        sample: read
            .entries
            .iter()
            .filter(|e| e.source_text.contains(word))
            .chain(
                read.entries
                    .iter()
                    .filter(|e| !e.source_text.contains(word)),
            )
            .take(5)
            .map(|e| e.source_text.chars().take(80).collect())
            .collect(),
        codes,
        check,
    };
    let name = if project_dir.join(crate::profile::FILE).exists() {
        "perfil-rascunho.toml"
    } else {
        crate::profile::FILE
    };
    draft.path = project_dir.join(name);
    fs::write(&draft.path, toml_of(&draft, word)).map_err(|e| CoreError::io(&draft.path, e))?;
    Ok(draft)
}

/// O rascunho em TOML, com o que conferir em comentario.
fn toml_of(d: &Draft, word: &str) -> String {
    let mut t = format!(
        "# Rascunho do perfil do jogo, montado por `romtranslate perfil-sugerir` com a\n\
         # palavra \"{word}\" ({} ocorrencia(s)). Confira cada valor no jogo antes de\n\
         # traduzir; o guia e o docs/PERFIL.md.\n\
         # Este rascunho le {} frase(s) ({} candidato(s) descartado(s)).\n",
        d.hits, d.sentences, d.discarded
    );
    for s in &d.sample {
        let _ = writeln!(t, "#   {s:?}");
    }
    for c in &d.check {
        let _ = writeln!(t, "# CONFERIR: {c}");
    }
    let table = match d.ascii_shift {
        Some(shift) => format!("ASCII com 0x{shift:X} a menos"),
        None => "so as letras da busca relativa".to_string(),
    };
    let _ = write!(
        t,
        "\n[[texto]]\n\
         nome = {:?}  # o trecho onde a palavra apareceu\n\
         tabela = {:?}  # tables/{}.tbl: {table}; confira acentos e pontuacao\n\
         inicio = 0x{:X}  # da primeira a ultima frase lida em volta da palavra\n\
         fim = 0x{:X}\n\
         cabecalho = {:?}  # o comeco que a maioria das frases com ponteiro tem\n\
         terminador = {:?}  # o ultimo byte antes do preenchimento que leva a frase seguinte\n\
         alinhamento = {}\n",
        d.module, TABLE_ID, TABLE_ID, d.start, d.end, d.header, d.terminator, d.align
    );
    if !d.codes.is_empty() {
        let list: Vec<String> = d
            .codes
            .iter()
            .take(10)
            .map(|(code, n)| format!("{code} x{n}"))
            .collect();
        let second = d.codes.get(1).map_or("..", |(code, _)| code.as_str());
        let _ = write!(
            t,
            "# Codigos fora da tabela nas frases lidas: {}.\n\
             # O mais comum costuma ser a quebra de linha e o segundo o fim da pagina\n\
             # (espera o botao). Confira no jogo antes de tirar o #:\n\
             # quebra = \"{}\"\n# pagina = \"{second}\"\n\
             # Codigo seguido sempre de bytes que nao sao letra leva parametro; declare\n\
             # quantos bytes, senao o parametro vira letra e a IA pode troca-lo:\n\
             # parametros = {{ XX = 1 }}\n",
            list.join(", "),
            d.codes[0].0
        );
    }
    let search: Vec<String> = d
        .search
        .iter()
        .map(|[a, b]| format!("[0x{a:X}, 0x{b:X}]"))
        .collect();
    let _ = write!(
        t,
        "\n[texto.ponteiros]\n\
         memoria = 0x{:08X}  # endereco onde o trecho e carregado\n\
         origem = 0x{:X}  # posicao do trecho no arquivo\n\
         procurar = [{}]  # onde os ponteiros das frases apareceram\n",
        d.memory,
        d.origin,
        search.join(", ")
    );
    t
}

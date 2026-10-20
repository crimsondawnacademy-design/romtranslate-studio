//! Loader de tabelas de caracteres `.tbl` (formato classico de romhacking):
//! uma entrada por linha, `HEX=texto`, com chaves de 1 a 16 bytes (as longas
//! sao comando de jogo com os parametros dele).
//! Linhas vazias e comentarios (`#` ou `;`) sao ignorados.
//!
//! A tabela vale nos dois sentidos: `lookup` le bytes do jogo, `encode`
//! escreve a traducao de volta. A copia que o projeto usa mora em
//! `<projeto>/tables/<id>.tbl` (`ProjectTables`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use icu_normalizer::DecomposingNormalizerBorrowed;

use crate::error::{CoreError, Result};
use crate::types::{TextEncoding, TextEntry};

/// Subdiretorio do projeto com as tabelas de caracteres dele.
pub const TABLES_DIR: &str = "tables";
/// Acima disto a lista de caracteres da tabela nao vai inteira pro prompt.
const PROMPT_CHARSET_MAX: usize = 400;
/// Os sinais que vao pro prompt junto do ASCII numa tabela grande demais pra
/// ir inteira: marcas de fala e enfeites que a traducao tem que manter.
const KEPT_SIGNS: &str = "◆◇☆★○●◎□■△▲▽▼※♪…「」『』";

/// A forma de largura inteira de um caractere ASCII imprimivel: "a" -> "ａ",
/// "!" -> "！" (o bloco U+FF01..U+FF5E e o ASCII deslocado). O espaco nao
/// entra: cada tabela tem o dela.
fn wide_form(ch: char) -> Option<char> {
    ('!'..='~')
        .contains(&ch)
        .then(|| char::from_u32(ch as u32 + 0xFEE0))
        .flatten()
}

/// O que pode ir no lugar de um caractere que a tabela nao tem: a forma de
/// largura inteira e, nas aspas, a aspa tipografica — a tabela japonesa tem
/// "”" e "’", mas nao a reta de largura inteira.
fn stand_ins(ch: char) -> impl Iterator<Item = char> {
    let quote = match ch {
        '"' => Some('”'),
        '\'' => Some('’'),
        _ => None,
    };
    wide_form(ch).into_iter().chain(quote)
}

/// A linha que diz quantas letras do texto traduzido do projeto couberam em
/// quantos codigos com os pares da tabela: `# pares: 1432 letras em 1000
/// codigos`. Gravada na extracao (`crate::pairs::with_rate`), lida no parse.
pub const PAIR_RATE: &str = "# pares: ";

#[derive(Debug, Clone)]
pub struct TblTable {
    entries: HashMap<Vec<u8>, String>,
    max_key_len: usize,
    /// Caminho inverso: texto -> bytes. Dois bytes com o mesmo texto: vale a
    /// MENOR chave (a mais curta; no empate, a de menor valor).
    reverse: HashMap<String, Vec<u8>>,
    /// Maior texto de uma entrada, em caracteres.
    max_text_chars: usize,
    /// (letras, codigos) da linha `PAIR_RATE`, se a tabela tem.
    pair_rate: Option<(usize, usize)>,
}

/// `text` e um par de letras ("de", "a "), e nao um caractere ou um codigo
/// de controle (`[FIM]`).
pub fn is_pair(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next(), chars.next()), (Some(a), Some(_), None) if a != '[')
}

/// O comando com parametro (`[FF26:0003:0003]`): a letra que vem logo
/// depois dele pode ser o ultimo parametro (`kuten`, o nome esticado), e um
/// par ali entregaria ao comando o codigo do par.
fn takes_params(text: &str) -> bool {
    text.starts_with('[') && text.contains(':')
}

/// O trecho de texto que comeca em `rest` (ate o proximo colchete) e
/// alinhado com espacos: tem 2 seguidos, como as opcoes de um menu
/// ("[FF06:0002]   Sim   Não", 1.126 frases no jogo medido em 20/10/2026).
/// Ali cada letra vai sozinha, e cada opcao fica na coluna em que a
/// traducao a pos.
fn aligned(rest: &str) -> bool {
    rest[..rest.find('[').unwrap_or(rest.len())].contains("  ")
}

impl TblTable {
    pub fn parse(text: &str) -> Result<Self> {
        let mut entries = HashMap::new();
        let mut max_key_len = 0;
        let mut pair_rate = None;
        for (idx, raw) in text.lines().enumerate() {
            let line = raw.trim_end_matches('\r');
            if let Some(rate) = line.strip_prefix(PAIR_RATE) {
                let numbers: Vec<usize> = rate
                    .split_whitespace()
                    .filter_map(|word| word.parse().ok())
                    .collect();
                if let [letters, codes] = numbers[..] {
                    pair_rate = (codes > 0 && letters >= codes).then_some((letters, codes));
                }
            }
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let n = idx + 1;
            let Some((hex, value)) = line.split_once('=') else {
                return Err(CoreError::Tbl(format!("linha {n}: falta '=' ({line})")));
            };
            let key = decode_hex(hex).ok_or_else(|| {
                CoreError::Tbl(format!(
                    "linha {n}: chave hex invalida \"{hex}\" (esperado de 2 a 32 digitos hex, aos pares)"
                ))
            })?;
            max_key_len = max_key_len.max(key.len());
            // Chave duplicada: a ultima vence (comum em .tbl reais).
            entries.insert(key, value.to_string());
        }
        if entries.is_empty() {
            return Err(CoreError::Tbl("tabela vazia".to_string()));
        }
        // Montado depois do parse: o HashMap nao tem ordem, a regra da menor
        // chave e que torna a escolha a mesma em toda execucao. Entrada de
        // texto vazio (`00=`) so existe na leitura.
        let mut reverse: HashMap<String, Vec<u8>> = HashMap::new();
        for (key, text) in entries.iter().filter(|(_, text)| !text.is_empty()) {
            let smaller = |old: &Vec<u8>| (key.len(), key) < (old.len(), old);
            if reverse.get(text).is_none_or(smaller) {
                reverse.insert(text.clone(), key.clone());
            }
        }
        let max_text_chars = reverse.keys().map(|t| t.chars().count()).max().unwrap_or(0);
        Ok(TblTable {
            entries,
            max_key_len,
            reverse,
            max_text_chars,
            pair_rate,
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
        Self::parse(&text)
    }

    /// Match mais longo no inicio de `bytes` (greedy longest-match).
    pub fn lookup(&self, bytes: &[u8]) -> Option<(usize, &str)> {
        let cap = self.max_key_len.min(bytes.len());
        for len in (1..=cap).rev() {
            if let Some(text) = self.entries.get(&bytes[..len]) {
                return Some((len, text.as_str()));
            }
        }
        None
    }

    /// Texto -> bytes, pelo casamento mais longo sobre o texto (uma entrada
    /// pode valer mais de um caractere: "[FIM]", "th"). Caractere que a
    /// tabela nao tem e erro que diz qual e onde — nunca um byte inventado.
    // ponytail: guloso, sem voltar atras. Numa tabela em que so "ab" e "bc"
    // existem (sem "c"), "abc" da erro mesmo cabendo como "a"+"bc". Tabela
    // real traz os caracteres avulsos; programacao dinamica se aparecer uma
    // que nao traga.
    ///
    /// Com pares de letras (todos de 2 caracteres) e as letras avulsas na
    /// tabela, o guloso ja usa o menor numero de codigos: escolher pares sem
    /// sobreposicao e escolher intervalos de mesmo tamanho, e pegar o que
    /// termina primeiro (o da esquerda) e o otimo. Programacao dinamica nao
    /// ganharia um codigo. Logo depois de um comando com parametro, a letra
    /// vai sozinha (`takes_params`), e no trecho alinhado com espacos, todas
    /// (`aligned`).
    ///
    /// Tabela japonesa nao tem a letra "a": tem "ａ", a de largura inteira.
    /// O caractere ASCII que falta e gravado nessa forma (`wide_form`), e e
    /// assim que ele sera lido de volta.
    pub fn encode(&self, text: &str) -> Result<Vec<u8>> {
        let (out, written) = self.encode_unchecked(text)?;
        // Chave de 2+ bytes que comeca por outra chave: os bytes certos, lado
        // a lado, podem ser lidos como outro texto.
        let read_back = self.decode(&out);
        if read_back.as_deref() != Some(written.as_str()) {
            return Err(CoreError::Tbl(format!(
                "tabela ambigua: \"{written}\" gravado seria lido de volta como \"{}\"",
                read_back.unwrap_or_default()
            )));
        }
        Ok(out)
    }

    /// `text` lido pela tabela `self` depois de gravado pela tabela `older`:
    /// os mesmos bytes, com os codigos na forma em que esta tabela os le
    /// (`[FF21][FFF0]` vira `[FF21:FFF0]` quando a leitura de agora sabe que
    /// o comando leva parametro). `None` = `older` nao grava o texto ou esta
    /// tabela nao le os bytes.
    pub fn reread(&self, older: &TblTable, text: &str) -> Option<String> {
        let (bytes, _) = older.encode_unchecked(text).ok()?;
        self.decode(&bytes)
    }

    /// Os bytes de `text` e o texto como ele foi escrito (o caractere que so
    /// existe em largura inteira, nessa forma), sem conferir a leitura de
    /// volta.
    fn encode_unchecked(&self, text: &str) -> Result<(Vec<u8>, String)> {
        self.encode_counting(text)
            .map(|(out, written, _)| (out, written))
    }

    /// `encode_unchecked` e quantas entradas da tabela o texto ocupou.
    fn encode_counting(&self, text: &str) -> Result<(Vec<u8>, String, usize)> {
        let mut out = Vec::new();
        // O texto como a tabela vai le-lo de volta.
        let mut written = String::with_capacity(text.len());
        let mut rest = text;
        let mut cells = 0;
        // Par vale aqui? Nao depois de comando com parametro nem em trecho
        // alinhado (`aligned`), que vai ate o proximo codigo.
        let (mut after_params, mut in_aligned) = (false, aligned(text));
        while let Some(ch) = rest.chars().next() {
            cells += 1;
            if let Some((end, bytes)) = self.longest_text(rest, !after_params && !in_aligned) {
                out.extend_from_slice(bytes);
                written.push_str(&rest[..end]);
                let piece = &rest[..end];
                rest = &rest[end..];
                after_params = takes_params(piece);
                if piece.starts_with('[') && piece.chars().count() > 1 {
                    in_aligned = aligned(rest);
                }
                continue;
            }
            after_params = false;
            let wide =
                stand_ins(ch).find_map(|w| Some((w, self.reverse.get(w.to_string().as_str())?)));
            let Some((wide, bytes)) = wide else {
                let position = text[..text.len() - rest.len()].chars().count() + 1;
                return Err(CoreError::Tbl(format!(
                    "o caractere {ch:?} (posicao {position}) nao existe na tabela"
                )));
            };
            out.extend_from_slice(bytes);
            written.push(wide);
            rest = &rest[ch.len_utf8()..];
        }
        Ok((out, written, cells))
    }

    /// A tabela escreve `ch`, como ele e ou por um substituto (`stand_ins`).
    fn writes(&self, ch: char) -> bool {
        let has = |c: char| self.reverse.contains_key(c.to_string().as_str());
        has(ch) || stand_ins(ch).any(has)
    }

    /// `text` e uma entrada da tabela inteira (um codigo de controle como
    /// `[FF03]`, por exemplo), e nao letras avulsas?
    pub fn has_entry(&self, text: &str) -> bool {
        self.reverse.contains_key(text)
    }

    /// Quantos codigos (entradas da tabela) `text` ocupa gravado: a largura
    /// dele na tela, numa fonte de largura fixa. `None` = nao da pra gravar.
    pub fn cells(&self, text: &str) -> Option<usize> {
        self.encode_counting(text).ok().map(|(_, _, cells)| cells)
    }

    /// A tabela tem pares de letras (`is_pair`).
    pub fn has_pairs(&self) -> bool {
        self.reverse.keys().any(|text| is_pair(text))
    }

    /// A maior entrada da tabela no comeco de `rest`: onde ela termina e os
    /// bytes dela. Sem `pairs`, par de letras nao vale.
    fn longest_text(&self, rest: &str, pairs: bool) -> Option<(usize, &Vec<u8>)> {
        // Onde terminam os prefixos de 1 a `max_text_chars` caracteres.
        let ends: Vec<usize> = rest
            .char_indices()
            .skip(1)
            .map(|(i, _)| i)
            .chain([rest.len()])
            .take(self.max_text_chars)
            .collect();
        ends.iter()
            .rev()
            .filter(|&&end| pairs || !is_pair(&rest[..end]))
            .find_map(|&end| self.reverse.get(&rest[..end]).map(|bytes| (end, bytes)))
    }

    /// Os caracteres que a tabela escreve sozinhos, em ordem — a lista que
    /// vai pro prompt de traducao. Entrada de varios caracteres ("[FIM]",
    /// "th") fica de fora: codigo de controle o tradutor copia do original,
    /// e dupla de letras e so compressao.
    ///
    /// Tabela com mais de `PROMPT_CHARSET_MAX` caracteres (a japonesa
    /// inteira tem quase 7 mil) nao cabe num prompt: ai a lista e o ASCII
    /// imprimivel que ela escreve, como ele e ou em largura inteira.
    // ponytail: vale pra traduzir pra idioma de alfabeto latino. Traducao
    // PRA japones com uma tabela dessas ficaria sem os kana na lista.
    pub fn charset(&self) -> String {
        let mut chars: Vec<char> = self
            .reverse
            .keys()
            .filter_map(|text| {
                let mut chars = text.chars();
                chars.next().filter(|_| chars.next().is_none())
            })
            .collect();
        if chars.len() > PROMPT_CHARSET_MAX {
            // Com o ASCII vao as figuras e os sinais que jogo usa pra marcar
            // fala e enfeitar texto, se a tabela os tem: fora da lista, o
            // modelo os tira da traducao (no piloto de 13/10/2026 sumiu o
            // losango que abre cada fala).
            // E as letras acentuadas que a tabela ganhou (desenhadas na fonte
            // do jogo, `images::letters`).
            let mut accented: Vec<char> = chars
                .iter()
                .copied()
                .filter(|c| base_letter(*c).is_some())
                .collect();
            accented.sort_unstable();
            return (' '..='~')
                .filter(|c| self.writes(*c))
                .chain(accented)
                .chain(KEPT_SIGNS.chars().filter(|c| chars.contains(c)))
                .collect();
        }
        chars.sort_unstable();
        chars.into_iter().collect()
    }

    /// Quantos caracteres uma traducao pode ter com a garantia de caber em
    /// `slot` bytes — o numero que vai pro prompt, porque o modelo conta
    /// caracteres e o espaco e em bytes da tabela. Conta pela entrada mais
    /// cara (bytes por caractere), entao nunca passa do que cabe: tabela de
    /// 1 byte por caractere devolve `slot`; dupla de letras num byte so
    /// ("th") nao aumenta o numero. Quem decide se a traducao cabe e
    /// `encode`, na validacao.
    // ponytail: piso pela entrada mais cara da tabela INTEIRA. Com letras de
    // 1 byte e kanji de 2 na mesma tabela, o prompt diz a metade do que cabe
    // em letras; medir so os caracteres do idioma de destino se aparecer um
    // jogo assim.
    ///
    /// Tabela com pares de letras e a conta medida no texto ja traduzido
    /// (`PAIR_RATE`): o numero e o do texto tipico, letras por codigo como
    /// nele. Ai ele pode prometer mais do que um texto sem nenhum par cabe —
    /// e a validacao que confere, e a repergunta (`pipeline`) encurta pelo
    /// que a resposta anterior ocupou.
    pub fn chars_that_fit(&self, slot: usize) -> usize {
        let worst = self
            .reverse
            .iter()
            .map(|(text, key)| slot.saturating_mul(text.chars().count()) / key.len())
            .min()
            .unwrap_or(0);
        self.letters_in(worst)
    }

    /// Quantas letras do texto tipico cabem em `cells` codigos: `cells`, ou,
    /// com pares de letras e a conta medida (`PAIR_RATE`), na proporcao dela.
    /// O teto de uma linha da caixa de texto no pedido a IA.
    pub fn letters_in(&self, cells: usize) -> usize {
        match self.pair_rate {
            Some((letters, codes)) => cells.saturating_mul(letters) / codes,
            None => cells,
        }
    }

    /// `chars_that_fit` pra uma traducao obrigada a copiar `kept` do original
    /// (codigos de controle, placeholders). O modelo conta "[FIM]" como 5
    /// caracteres e ele custa 1 byte: sem esta conta, "HELLO[FIM]" (6 bytes)
    /// viraria um teto de 6 caracteres pra um original de 10, e so o codigo
    /// gastaria 5. Cada item de `kept` entra pelos bytes que custa e pelos
    /// caracteres que tem; o resto do espaco segue a conta pela entrada mais
    /// cara. O que a tabela nao escreve fica fora e continua gastando o teto:
    /// pra menos, nunca pra mais.
    pub fn chars_that_fit_keeping(&self, slot: usize, kept: &[String]) -> usize {
        let (bytes, chars) = kept
            .iter()
            .filter_map(|token| Some((self.encode(token).ok()?.len(), token.chars().count())))
            .fold((0, 0), |(bytes, chars), (b, c)| (bytes + b, chars + c));
        self.chars_that_fit(slot.saturating_sub(bytes)) + chars
    }

    /// O teto da repergunta depois de `previous` nao caber em `slot`:
    /// `chars_that_fit_keeping` e, com pares de letras (onde o teto e o do
    /// texto tipico e a resposta pode ter ficado abaixo dele), no maximo as
    /// letras de `previous` na proporcao do espaco, fora os codigos copiados.
    pub fn chars_that_fit_after(&self, slot: usize, kept: &[String], previous: &str) -> usize {
        let room = self.chars_that_fit_keeping(slot, kept);
        let Some(len) = self
            .encode(previous)
            .ok()
            .map(|bytes| bytes.len())
            .filter(|&len| len > slot && self.has_pairs())
        else {
            return room;
        };
        let (code_bytes, code_chars) = kept
            .iter()
            .filter_map(|token| Some((self.encode(token).ok()?.len(), token.chars().count())))
            .fold((0, 0), |(b, c), (bytes, chars)| (b + bytes, c + chars));
        let letters = previous.chars().count().saturating_sub(code_chars);
        let fit = letters * slot.saturating_sub(code_bytes) / len.saturating_sub(code_bytes).max(1);
        room.min(fit + code_chars)
    }

    /// `text` com cada caractere que a tabela nao escreve trocado pelo
    /// equivalente simples dele (`plain_equivalent`: 'ç' -> 'c', 'É' -> 'E',
    /// '”' -> '"') — so quando a tabela escreve o equivalente. `None` = nada a
    /// trocar. Caractere sem troca fica como veio e a validacao acusa: aqui
    /// nada some nem vira outro simbolo.
    // ponytail: so caractere precomposto. Resposta com o acento solto ("e" +
    // U+0301) nao e mexida e cai no erro de validacao; compor antes (NFC) se
    // algum provider responder assim.
    pub fn drop_missing_accents(&self, text: &str) -> Option<String> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(ch) = rest.chars().next() {
            // O que a tabela escreve passa inteiro, entrada longa inclusive.
            if let Some((end, _)) = self.longest_text(rest, true) {
                out.push_str(&rest[..end]);
                rest = &rest[end..];
                continue;
            }
            // Caractere que so existe em largura inteira fica como veio: e
            // `encode` que o grava nessa forma.
            if self.writes(ch) {
                out.push(ch);
                rest = &rest[ch.len_utf8()..];
                continue;
            }
            let plain = plain_equivalent(ch).filter(|plain| plain.chars().all(|c| self.writes(c)));
            match plain {
                Some(plain) => out.push_str(&plain),
                None => out.push(ch),
            }
            rest = &rest[ch.len_utf8()..];
        }
        (out != text).then_some(out)
    }

    /// Le `bytes` inteiro pela tabela. `None` = sobrou byte sem entrada.
    fn decode(&self, bytes: &[u8]) -> Option<String> {
        let mut text = String::new();
        let mut i = 0;
        while i < bytes.len() {
            let (len, piece) = self.lookup(&bytes[i..])?;
            text.push_str(piece);
            i += len;
        }
        Some(text)
    }

    /// Os bytes que vao no espaco de `slot` bytes de uma string: a traducao
    /// e o preenchimento da sobra — 0x00 se a original era terminada; senao
    /// (largura fixa) o espaco DA TABELA, e sem ele a traducao tem que ocupar
    /// o espaco exato. Traducao maior que `slot` volta sem preenchimento:
    /// quem chama decide se e erro ou relocacao.
    pub fn encode_slot(&self, text: &str, slot: usize, terminated: bool) -> Result<Vec<u8>> {
        self.encode_slot_with(text, slot, terminated.then_some(&[0u8][..]))
    }

    /// `encode_slot` com a sobra completada por `fill` repetido (o 0x00 da
    /// string terminada, o codigo de fim de um campo de largura fixa); sem
    /// `fill`, pelo espaco da tabela.
    pub fn encode_slot_with(
        &self,
        text: &str,
        slot: usize,
        fill: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let mut bytes = self.encode(text)?;
        let Some(room) = slot.checked_sub(bytes.len()).filter(|&room| room > 0) else {
            return Ok(bytes);
        };
        if let Some(fill) = fill.filter(|fill| !fill.is_empty()) {
            if !room.is_multiple_of(fill.len()) {
                return Err(CoreError::Tbl(format!(
                    "campo de largura fixa: sobram {room} bytes, que o preenchimento de {} \
                     bytes nao completa",
                    fill.len()
                )));
            }
            bytes.extend(fill.repeat(room / fill.len()));
            return Ok(bytes);
        }
        match self.reverse.get(" ") {
            Some(space) if room.is_multiple_of(space.len()) => {
                bytes.extend(space.repeat(room / space.len()));
                Ok(bytes)
            }
            space => Err(CoreError::Tbl(format!(
                "string de largura fixa: a traducao ocupa {} bytes e precisa ocupar \
                 exatamente {slot} — {}",
                bytes.len(),
                match space {
                    Some(space) => format!(
                        "o espaco da tabela tem {} bytes e nao completa a sobra de {room}",
                        space.len()
                    ),
                    None => "a tabela nao tem espaco pra completar a sobra".to_string(),
                }
            ))),
        }
    }

    /// Cada entrada da tabela: a chave e o texto.
    pub fn entries(&self) -> impl Iterator<Item = (&[u8], &str)> {
        self.entries
            .iter()
            .map(|(key, text)| (key.as_slice(), text.as_str()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Tabelas de um projeto, carregadas sob demanda de
/// `<projeto>/tables/<id>.tbl`. A entry so guarda o id
/// (`TextEncoding::Table(id)` = nome do arquivo sem extensao).
pub struct ProjectTables {
    dir: PathBuf,
    loaded: HashMap<String, TblTable>,
}

impl ProjectTables {
    pub fn new(project_dir: &Path) -> Self {
        ProjectTables {
            dir: project_dir.join(TABLES_DIR),
            loaded: HashMap::new(),
        }
    }

    /// A tabela de id `id`. Arquivo ausente e erro que diz onde ele devia
    /// estar — sem a tabela nao da pra conferir nem regravar o texto.
    pub fn get(&mut self, id: &str) -> Result<&TblTable> {
        if !self.loaded.contains_key(id) {
            // O id vem do banco do projeto: nao pode sair de `tables/`.
            if Path::new(id).file_name().and_then(|n| n.to_str()) != Some(id) {
                return Err(CoreError::Tbl(format!("id de tabela invalido: \"{id}\"")));
            }
            let path = self.dir.join(format!("{id}.tbl"));
            if !path.is_file() {
                return Err(CoreError::Tbl(format!(
                    "a tabela \"{id}\" nao esta no projeto (esperada em {}) — escaneie de \
                     novo com ela ou copie o arquivo pra la",
                    path.display()
                )));
            }
            self.loaded.insert(id.to_string(), TblTable::load(&path)?);
        }
        Ok(&self.loaded[id])
    }

    /// A tabela da entry; `None` se o encoding dela nao e de tabela.
    pub fn for_entry(&mut self, entry: &TextEntry) -> Result<Option<&TblTable>> {
        match &entry.encoding {
            TextEncoding::Table(id) => self.get(id).map(Some),
            _ => Ok(None),
        }
    }
}

/// Garante que a tabela em `source` esta em `<projeto>/tables/<nome>.tbl` e
/// devolve esse caminho: o scan le dali, e a validacao e a reinsercao tambem
/// — leitura e escrita pela mesma tabela, e o projeto nao depende de um
/// arquivo solto no disco. Copia que ja existe com outro conteudo nao e
/// sobrescrita: pode ter sido completada a mao.
pub fn adopt_table(project_dir: &Path, source: &Path) -> Result<PathBuf> {
    let text = fs::read_to_string(source).map_err(|e| CoreError::io(source, e))?;
    TblTable::parse(&text)?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| CoreError::Tbl(format!("nome de tabela invalido: {}", source.display())))?;
    let dir = project_dir.join(TABLES_DIR);
    let dest = dir.join(format!("{stem}.tbl"));
    match fs::read_to_string(&dest) {
        Ok(current) if current == text => {}
        Ok(_) => {
            return Err(CoreError::Tbl(format!(
                "o projeto ja tem uma tabela \"{stem}\" com outro conteudo em {} — edite essa \
                 copia ou escolha um arquivo com outro nome",
                dest.display()
            )))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
            fs::write(&dest, &text).map_err(|e| CoreError::io(&dest, e))?;
        }
        // Existe mas nao da pra ler: nao e nosso pra sobrescrever.
        Err(e) => return Err(CoreError::io(&dest, e)),
    }
    Ok(dest)
}

/// Grava no projeto a tabela `id` que a extracao montou sozinha. A que ja
/// existe e trocada pela de agora, com as linhas que a pessoa mudou
/// (`hand_lines`) no fim, valendo por cima: a leitura aprende o jogo melhor a
/// cada versao, e a tabela velha lia os comandos de outro jeito que as frases
/// (medido em 14/10/2026: 113 falas acusadas de "tabela ambigua" porque o
/// comando de nome de item tinha parametro numa e nao na outra).
pub fn ensure_table(project_dir: &Path, id: &str, text: &str) -> Result<()> {
    TblTable::parse(text)?;
    let dir = project_dir.join(TABLES_DIR);
    let dest = dir.join(format!("{id}.tbl"));
    let mut out = text.to_string();
    match fs::read_to_string(&dest) {
        Ok(old) => {
            let kept = hand_lines(&old, text);
            if !kept.is_empty() {
                out.push_str("# Linhas mudadas a mao, mantidas da tabela anterior:\n");
                for line in kept {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            if old == out {
                return Ok(());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(CoreError::io(&dest, e)),
    }
    fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    fs::write(&dest, out).map_err(|e| CoreError::io(&dest, e))
}

/// As linhas de `old` que a pessoa escreveu: nem estao na tabela gerada agora
/// (`generated`), nem tem o valor que o gerador da a uma chave que nao reconhece
/// (`[` + as unidades de 16 bits da chave + `]`: comando, comando com
/// parametro, desenho proprio), nem o caractere da ordem JIS padrao naquele
/// codigo (a tabela de antes de a extracao ler a ordem da fonte do jogo,
/// `kuten::font_order`), nem uma letra acentuada que a gerada de agora poe em
/// outro codigo (`images::letters::choose`), nem um par de letras ou o que o
/// codigo de um par da gerada de agora tinha antes (`crate::pairs`) — essas
/// sao de uma geracao velha.
fn hand_lines<'a>(old: &'a str, generated: &str) -> Vec<&'a str> {
    let fresh: std::collections::HashSet<&str> = generated.lines().collect();
    let paired: std::collections::HashSet<&str> = generated
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(_, value)| is_pair(value))
        .map(|(hex, _)| hex)
        .collect();
    let drawn: std::collections::HashSet<&str> = generated
        .lines()
        .filter_map(|line| line.split_once('=').map(|(_, value)| value))
        .filter(|value| {
            let mut chars = value.chars();
            chars.next().and_then(base_letter).is_some() && chars.next().is_none()
        })
        .collect();
    let generator_value = |hex: &str| {
        let bytes = decode_hex(hex)?;
        let units: Vec<String> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| format!("{:04X}", u16::from_le_bytes(*pair)))
            .collect();
        (bytes.len() % 2 == 0).then(|| format!("[{}]", units.join(":")))
    };
    let standard = |hex: &str| {
        let pair: [u8; 2] = decode_hex(hex)?.try_into().ok()?;
        crate::kuten::decode(u16::from_le_bytes(pair)).map(String::from)
    };
    old.lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with(';'))
        .filter(|line| !fresh.contains(line))
        .filter(|line| {
            line.split_once('=').is_some_and(|(hex, value)| {
                generator_value(hex).as_deref() != Some(value)
                    && standard(hex).as_deref() != Some(value)
                    && !drawn.contains(value)
                    && !is_pair(value)
                    && !paired.contains(hex)
            })
        })
        .collect()
}

/// O que se grava no lugar de um caractere que a fonte do jogo nao tem,
/// quando existe um equivalente que nao troca a palavra: a letra sem acento
/// (`base_letter`) e o sinal ASCII de aspas, travessao e reticencias
/// tipograficos. Sempre ASCII. `None` = nao ha equivalente (e o que vale pra
/// todo caractere ASCII).
pub(crate) fn plain_equivalent(ch: char) -> Option<String> {
    let sign = match ch {
        '\u{2018}' | '\u{2019}' => "'",
        '\u{201C}' | '\u{201D}' => "\"",
        '\u{2013}' | '\u{2014}' => "-",
        '\u{2026}' => "...",
        _ => return base_letter(ch).map(String::from),
    };
    Some(sign.to_string())
}

/// A letra sem acento de uma letra latina acentuada, pela decomposicao
/// canonica do Unicode: letra base + marcas U+0300..U+036F. So base ASCII —
/// 'й' -> 'и' e 'が' -> 'か' tambem sao decomposicoes, mas dao outra letra.
pub(crate) fn base_letter(ch: char) -> Option<char> {
    const NFD: DecomposingNormalizerBorrowed<'static> = DecomposingNormalizerBorrowed::new_nfd();
    let mut buf = [0; 4];
    let decomposed = NFD.normalize(ch.encode_utf8(&mut buf));
    let mut parts = decomposed.chars();
    let base = parts.next()?;
    (base != ch
        && base.is_ascii_alphabetic()
        && parts.all(|mark| ('\u{300}'..='\u{36F}').contains(&mark)))
    .then_some(base)
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    let hex = hex.trim();
    if hex.is_empty() || !hex.len().is_multiple_of(2) || hex.len() > 32 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_extraction_rewrites_the_table_and_keeps_the_lines_changed_by_hand() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-tbl-{nanos}"));
        // A tabela de uma leitura velha: um comando com parametro que a de
        // agora le de outro jeito, um desenho sem nome e um que a pessoa
        // batizou.
        let old = "# gerada\n1B01=あ\n21FF=[FF21]\n21FF5D00=[FF21:005D]\nAD00=[00AD]\nAE00=父\n";
        ensure_table(&dir, "jis16", old).unwrap();
        let new = "# gerada de novo\n1B01=あ\n21FF=[FF21]\n21FFF0FF=[FF21:FFF0]\nAD00=[00AD]\nAE00=[00AE]\n";
        ensure_table(&dir, "jis16", new).unwrap();
        let written = fs::read_to_string(dir.join(TABLES_DIR).join("jis16.tbl")).unwrap();
        assert!(written.starts_with(new), "{written}");
        assert!(
            !written.contains("[FF21:005D]"),
            "linha de geracao velha sai"
        );
        assert!(written.ends_with("AE00=父\n"), "{written}");
        let table = TblTable::parse(&written).unwrap();
        assert_eq!(table.lookup(&[0xAE, 0x00]), Some((2, "父")));
        // De novo com a mesma tabela: o arquivo nao muda.
        ensure_table(&dir, "jis16", new).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(TABLES_DIR).join("jis16.tbl")).unwrap(),
            written
        );
        // A ordem da fonte do jogo troca o kanji de um codigo: o da ordem
        // padrao, que estava no arquivo, e geracao velha e sai.
        let code: u16 = 0x0E3C;
        let standard = crate::kuten::decode(code).unwrap();
        let key = format!("{:02X}{:02X}", code & 0xFF, code >> 8);
        ensure_table(&dir, "jis16", &format!("{new}{key}={standard}\n")).unwrap();
        ensure_table(&dir, "jis16", &format!("{new}{key}=中\n")).unwrap();
        let written = fs::read_to_string(dir.join(TABLES_DIR).join("jis16.tbl")).unwrap();
        assert!(!written.contains(&format!("{key}={standard}")), "{written}");
        // A letra acentuada que a extracao de agora poe em outro codigo: a do
        // codigo velho e geracao velha e sai. Letra que a de agora nao poe em
        // lugar nenhum fica (pode ser da pessoa).
        ensure_table(&dir, "jis16", &format!("{new}0E0F=á\n")).unwrap();
        ensure_table(&dir, "jis16", &format!("{new}0D0F=á\n9900=ñ\n")).unwrap();
        ensure_table(&dir, "jis16", &format!("{new}0C0F=á\n")).unwrap();
        let written = fs::read_to_string(dir.join(TABLES_DIR).join("jis16.tbl")).unwrap();
        assert!(!written.contains("0D0F=á") && !written.contains("0E0F=á"));
        assert!(written.contains("\n9900=ñ\n"), "{written}");
        // O par de letras e da geracao: a extracao que tira o par (ou o poe
        // em outro codigo) nao o deixa como linha da pessoa, nem o kanji que
        // o codigo do par tinha antes.
        ensure_table(&dir, "jis16", &format!("{new}0E0F=弌\n")).unwrap();
        ensure_table(&dir, "jis16", &format!("{new}0E0F=de\n")).unwrap();
        let written = fs::read_to_string(dir.join(TABLES_DIR).join("jis16.tbl")).unwrap();
        assert!(!written.contains("=弌") && written.contains("\n0E0F=de\n"));
        ensure_table(&dir, "jis16", new).unwrap();
        let written = fs::read_to_string(dir.join(TABLES_DIR).join("jis16.tbl")).unwrap();
        assert!(!written.contains("=de"), "{written}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_big_table_lists_the_accented_letters_it_gained() {
        let mut text: String = (0..500u16)
            .map(|k| {
                format!(
                    "{:04X}={}\n",
                    0x1000 + k,
                    char::from_u32(0x4E00 + k as u32).unwrap()
                )
            })
            .collect();
        text.push_str("2100=ａ\n2200=á\n2300=Ç\n");
        let charset = TblTable::parse(&text).unwrap().charset();
        assert!(charset.contains("aÇá"), "{charset}");
        assert!(!charset.contains('一'), "o kanji nao vai pro pedido");
    }

    #[test]
    fn a_translation_is_reread_in_the_form_of_the_new_table() {
        let older = TblTable::parse("5D00=◇\n1B01=あ\n21FF=[FF21]\n").unwrap();
        let newer =
            TblTable::parse("5D00=◇\n1B01=あ\n21FF=[FF21]\n21FF5D00=[FF21:005D]\n").unwrap();
        assert_eq!(
            newer.reread(&older, "◇[FF21]◇あ").as_deref(),
            Some("◇[FF21:005D]あ")
        );
        // Escrito do jeito velho, o comando com o parametro colado acusa a
        // tabela nova de ambigua; relido, grava os mesmos bytes sem erro.
        assert!(newer.encode("[FF21]◇").is_err());
        assert_eq!(
            newer.encode("[FF21:005D]").unwrap(),
            [0x21, 0xFF, 0x5D, 0x00]
        );
        // O que a tabela velha nao grava fica como esta.
        assert_eq!(newer.reread(&older, "x"), None);
    }

    #[test]
    fn quotes_the_table_lacks_go_in_as_typographic_ones() {
        // Tabela como a japonesa: aspas tipograficas, sem a reta.
        let table = TblTable::parse("01=”\n02=’\n03=a\n04=[FF03]\n").unwrap();
        assert_eq!(table.encode("a\"a'").unwrap(), [3, 1, 3, 2]);
        assert!(table.has_entry("[FF03]") && !table.has_entry("<a>"));
    }

    #[test]
    fn encode_takes_the_longest_match_and_reads_back() {
        // Chave de 2 bytes, entrada de varios caracteres e uma dupla ("th")
        // que vale um byte so.
        let table = TblTable::parse("80=t\n81=h\n82=e\n90=th\nFF20= \nFE=[FIM]\n").unwrap();
        assert_eq!(table.encode("the").unwrap(), [0x90, 0x82]);
        assert_eq!(
            table.encode("he t").unwrap(),
            [0x81, 0x82, 0xFF, 0x20, 0x80]
        );
        assert_eq!(table.encode("e[FIM]").unwrap(), [0x82, 0xFE]);
        assert_eq!(table.encode("").unwrap(), [] as [u8; 0]);
        for text in ["the", "he t", "t[FIM]h"] {
            assert_eq!(table.decode(&table.encode(text).unwrap()).unwrap(), text);
        }
    }

    /// Tabela como a do japones de 16 bits com pares: letras avulsas, o
    /// espaco, comandos e os pares, 2 bytes cada.
    fn paired() -> TblTable {
        let mut text = String::from("0100= \n02FF=[FF02]\n23FF0200=[FF23:0002]\n");
        for (k, c) in "abcdeor.!".chars().enumerate() {
            text.push_str(&format!("{:02X}01={c}\n", 0x10 + k));
        }
        for (k, pair) in ["de", "a ", " o", "or", "ra", "ab", "bc", "cd"]
            .iter()
            .enumerate()
        {
            text.push_str(&format!("{:02X}02={pair}\n", 0x10 + k));
        }
        TblTable::parse(&text).unwrap()
    }

    #[test]
    fn pairs_are_written_and_read_back_and_count_one_cell() {
        let table = paired();
        assert!(table.has_pairs() && is_pair("de") && is_pair("a ") && !is_pair("[FIM]"));
        let bytes = table.encode("ora de").unwrap();
        // "or" "a " "de": 3 codigos pra 6 letras.
        assert_eq!(bytes, [0x13, 0x02, 0x11, 0x02, 0x10, 0x02]);
        assert_eq!(table.decode(&bytes).as_deref(), Some("ora de"));
        assert_eq!(table.cells("ora de"), Some(3));
        assert_eq!(table.cells("ora dex"), None);
        // Depois de comando com parametro a letra vai sozinha: ela pode ser
        // o ultimo parametro dele.
        assert_eq!(table.cells("[FF23:0002]de"), Some(3));
        assert_eq!(table.cells("[FF02]de"), Some(2));
        // Trecho alinhado com espacos (as opcoes de um menu): sem pares ate o
        // proximo codigo, e as colunas ficam onde a traducao as pos.
        assert_eq!(table.cells("[FF02]  ora  de"), Some(1 + 9));
        assert_eq!(table.cells("  de[FF02]de"), Some(4 + 1 + 1));
    }

    /// O guloso (o par mais a esquerda) ja usa o menor numero de codigos com
    /// pares de 2 letras: igual a programacao dinamica, em textos sorteados.
    #[test]
    fn the_greedy_encoding_uses_the_fewest_codes() {
        let table = paired();
        let letters: Vec<char> = "abcdor ".chars().collect();
        let fewest = |text: &[char]| -> usize {
            let mut best = vec![0usize; text.len() + 1];
            for i in (0..text.len()).rev() {
                best[i] = 1 + best[i + 1];
                let two: String = text[i..(i + 2).min(text.len())].iter().collect();
                if two.chars().count() == 2 && table.has_entry(&two) {
                    best[i] = best[i].min(1 + best[i + 2]);
                }
            }
            best[0]
        };
        let mut seed = 7u32;
        for _ in 0..2000 {
            let text: Vec<char> = (0..12)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    letters[(seed >> 16) as usize % letters.len()]
                })
                .collect();
            let joined: String = text.iter().collect();
            if joined.contains("  ") {
                continue;
            }
            assert_eq!(table.cells(&joined), Some(fewest(&text)), "{joined:?}");
        }
    }

    /// A conta medida no texto traduzido (`PAIR_RATE`) poe no teto as letras
    /// que os pares deixam caber; sem ela, o teto de sempre.
    #[test]
    fn the_measured_rate_raises_the_ceiling() {
        let plain = paired();
        assert_eq!(plain.chars_that_fit(20), 10);
        assert_eq!(plain.letters_in(10), 10);
        let text = "# pares: 150 letras em 100 codigos (texto traduzido do projeto)\n0100= \n1001=a\n1002=de\n";
        let rated = TblTable::parse(text).unwrap();
        assert_eq!(rated.chars_that_fit(20), 15);
        assert_eq!(rated.letters_in(10), 15);
        // Conta sem sentido (menos letras que codigos) nao vale.
        let odd = TblTable::parse("# pares: 50 letras em 100 codigos\n1001=a\n").unwrap();
        assert_eq!(odd.chars_that_fit(20), 10);
    }

    /// A repergunta com pares: o teto e o das letras da resposta que nao
    /// coube, na proporcao do espaco; sem pares, o de sempre.
    #[test]
    fn the_shorter_retry_scales_the_answer_that_did_not_fit() {
        let rated = format!(
            "{PAIR_RATE}200 letras em 100 codigos\n0100= \n1001=a\n1101=b\n1002=ab\n02FF=[FF02]\n"
        );
        let table = TblTable::parse(&rated).unwrap();
        let kept = vec!["[FF02]".to_string()];
        // Teto do texto tipico: (10 - 2) / 2 * 2 + 6 = 14.
        assert_eq!(table.chars_that_fit_keeping(10, &kept), 14);
        // "aaaaaaaa" nao tem par: 8 codigos, 16 bytes + 2 do codigo. As 8
        // letras em 18 bytes viram 4 em 10.
        assert_eq!(
            table.chars_that_fit_after(10, &kept, "aaaaaaaa[FF02]"),
            4 + 6
        );
        // Resposta que cabe nao muda o teto.
        assert_eq!(table.chars_that_fit_after(10, &kept, "abab[FF02]"), 14);
        // Sem pares, o teto de sempre, mesmo com o codigo longo.
        let plain = TblTable::parse("0100= \n1001=a\n02FF=[FF02]\n").unwrap();
        assert_eq!(
            plain.chars_that_fit_after(10, &kept, "aaaaaaaa[FF02]"),
            4 + 6
        );
        assert_eq!(plain.chars_that_fit_after(4, &kept, "a[FF02]"), 1 + 6);
    }

    #[test]
    fn missing_character_is_named_with_its_position() {
        let table = TblTable::parse("80=A\n81=B\n").unwrap();
        let err = table.encode("ABã").unwrap_err().to_string();
        assert!(err.contains("'ã'") && err.contains("posicao 3"), "{err}");
        let err = table.encode("!AB").unwrap_err().to_string();
        assert!(err.contains("'!'") && err.contains("posicao 1"), "{err}");
        // Parte de uma entrada longa nao vale: "[FI" nao e "[FIM]".
        let table = TblTable::parse("80=A\nFE=[FIM]\n").unwrap();
        assert!(table
            .encode("A[FI")
            .unwrap_err()
            .to_string()
            .contains("'['"));
    }

    #[test]
    fn same_text_in_two_keys_always_picks_the_smallest_key() {
        // A ordem das linhas nao muda a escolha: chave mais curta, depois a
        // de menor valor.
        for text in ["C1=A\n80=A\nFF01=A\n", "FF01=A\n80=A\nC1=A\n"] {
            assert_eq!(
                TblTable::parse(text).unwrap().encode("AA").unwrap(),
                [0x80, 0x80]
            );
        }
    }

    #[test]
    fn ambiguous_table_is_refused_instead_of_writing_another_text() {
        // 80 81 lado a lado seriam lidos como "X", nao como "AB".
        let table = TblTable::parse("80=A\n81=B\n8081=X\n").unwrap();
        let err = table.encode("AB").unwrap_err().to_string();
        assert!(err.contains("ambigua") && err.contains("\"X\""), "{err}");
        assert_eq!(table.encode("BA").unwrap(), [0x81, 0x80]);
    }

    #[test]
    fn charset_lists_only_what_one_entry_of_one_character_writes() {
        // Fora da lista: entrada longa, dupla de letras e texto vazio. Letra
        // repetida em duas chaves entra uma vez so.
        let table =
            TblTable::parse("82=b\n81=a\nC1=a\n40= \n90=th\nFE=[FIM]\n00=\n83=é\n").unwrap();
        assert_eq!(table.charset(), " abé");
    }

    #[test]
    fn chars_that_fit_never_promises_more_than_the_slot_holds() {
        // Um byte por caractere: caracteres e bytes sao o mesmo numero.
        let plain = TblTable::parse("80=A\n81=B\n40= \n").unwrap();
        assert_eq!(plain.chars_that_fit(10), 10);
        // Com dupla num byte so ou codigo de controle cabe mais do que o
        // numero diz, nunca menos.
        let pairs = TblTable::parse("80=t\n81=h\n90=th\nFE=[FIM]\n").unwrap();
        assert_eq!(pairs.chars_that_fit(10), 10);
        // Um caractere de 2 bytes basta pra valer a conta dele.
        let wide = TblTable::parse("80=A\n81=B\nFF20= \n").unwrap();
        assert_eq!(wide.chars_that_fit(10), 5);
        assert_eq!(wide.chars_that_fit(11), 5);
        // Entrada de 3 bytes pra 2 caracteres: 1,5 byte por caractere.
        let odd = TblTable::parse("80=A\nC0C1C2=AB\n").unwrap();
        assert_eq!(odd.chars_that_fit(10), 6);
        // O que o numero promete, o codificador cumpre — no pior texto.
        for (table, worst) in [(&plain, "A"), (&wide, " "), (&odd, "AB")] {
            for slot in 0..12 {
                let fit = table.chars_that_fit(slot);
                let text: String = worst.chars().cycle().take(fit).collect();
                assert!(
                    table.encode(&text).unwrap().len() <= slot,
                    "{text:?} em {slot}"
                );
            }
        }
        // So entrada de texto vazio: nada cabe.
        assert_eq!(TblTable::parse("00=\n").unwrap().chars_that_fit(10), 0);
    }

    #[test]
    fn copied_control_codes_count_by_what_they_cost() {
        let table = TblTable::parse("80=A\n81=B\n40= \nFE=[FIM]\nFD=[NOME]\n").unwrap();
        let kept =
            |tokens: &[&str]| -> Vec<String> { tokens.iter().map(|t| t.to_string()).collect() };
        // Sem nada pra copiar e a conta de sempre.
        assert_eq!(
            table.chars_that_fit_keeping(6, &[]),
            table.chars_that_fit(6)
        );
        // "ABABA[FIM]": 6 bytes, 10 caracteres — o teto e o proprio original.
        assert_eq!(table.chars_that_fit_keeping(6, &kept(&["[FIM]"])), 10);
        assert_eq!(
            table.chars_that_fit_keeping(7, &kept(&["[NOME]", "[FIM]"])),
            16
        );
        // Placeholder que a tabela escreve letra a letra custa o que tem; o
        // que ela nao escreve ("{0}") fica fora da conta.
        assert_eq!(table.chars_that_fit_keeping(6, &kept(&["AB"])), 6);
        assert_eq!(table.chars_that_fit_keeping(6, &kept(&["{0}"])), 6);

        // Caractere de 2 bytes na tabela: o codigo nao paga por ele, e uma
        // string que e so o codigo nao vira teto zero.
        let wide = TblTable::parse("80=A\n81=B\nFF21=!\nFE=[FIM]\n").unwrap();
        assert_eq!(wide.chars_that_fit(1), 0);
        assert_eq!(wide.chars_that_fit_keeping(1, &kept(&["[FIM]"])), 5);
        assert_eq!(wide.chars_that_fit_keeping(6, &kept(&["[FIM]"])), 7);
        // O que o numero promete, o codificador cumpre no pior texto.
        for slot in 1..12 {
            let fit = wide.chars_that_fit_keeping(slot, &kept(&["[FIM]"]));
            let text = "!".repeat(fit - 5) + "[FIM]";
            assert!(
                wide.encode(&text).unwrap().len() <= slot,
                "{text:?} em {slot}"
            );
        }
    }

    #[test]
    fn missing_accent_becomes_the_base_letter_only_when_the_table_has_it() {
        let table = TblTable::parse("80=A\n81=C\n82=O\nA0=a\nA1=c\nA2=o\n40= \n").unwrap();
        assert_eq!(
            table.drop_missing_accents("AÇÃO ação").as_deref(),
            Some("ACAO acao")
        );
        // Nada a trocar: `None`, e o texto segue como veio.
        assert_eq!(table.drop_missing_accents("ACAO"), None);
        // Base fora da tabela ('e', 'n'), pontuacao e letra sem decomposicao
        // ('ß', 'ø') ficam como vieram — quem acusa e a validacao.
        assert_eq!(table.drop_missing_accents("é ñ ! ß ø"), None);
        // O que tem troca e trocado mesmo com um '!' sem saida ao lado.
        assert_eq!(
            table.drop_missing_accents("ação!").as_deref(),
            Some("acao!")
        );
        // Maiuscula nao vira minuscula nem o contrario.
        let upper = TblTable::parse("80=A\n81=C\n").unwrap();
        assert_eq!(upper.drop_missing_accents("ção"), None);
        assert_eq!(upper.drop_missing_accents("ÇÃ").as_deref(), Some("CA"));
        // Acento empilhado (vietnamita) tambem cai na base.
        assert_eq!(table.drop_missing_accents("ấ").as_deref(), Some("a"));
        // Decomposicao que da outra letra nao e troca de acento.
        let other = TblTable::parse("80=и\n81=か\n").unwrap();
        assert_eq!(other.drop_missing_accents("йが"), None);
    }

    #[test]
    fn typographic_signs_become_the_ascii_sign_only_when_the_table_has_it() {
        let table = TblTable::parse("80=a\n81=\"\n82=-\n83=.\n").unwrap();
        // Aspas, travessao e reticencias: o sinal ASCII, que a tabela tem.
        assert_eq!(
            table.drop_missing_accents("“a” — a…").as_deref(),
            Some("\"a\" - a...")
        );
        // Apostrofo ela nao tem: fica como veio, e a validacao acusa.
        assert_eq!(table.drop_missing_accents("a’a"), None);
        // Sem o ponto, as reticencias ficam; o travessao, que tem saida, sai.
        let bare = TblTable::parse("80=a\n82=-\n").unwrap();
        assert_eq!(bare.drop_missing_accents("a–a…").as_deref(), Some("a-a…"));
        // ASCII nunca tem equivalente: nada a trocar nele.
        assert!((' '..='~').all(|ch| plain_equivalent(ch).is_none()));
        assert_eq!(plain_equivalent('ã').as_deref(), Some("a"));
        assert_eq!(plain_equivalent('‘').as_deref(), Some("'"));
        assert_eq!(plain_equivalent('º'), None);
    }

    #[test]
    fn accent_the_table_has_is_kept_even_inside_a_long_entry() {
        // A tabela escreve 'ã' sozinho e "çõ" numa entrada so: nenhum dos
        // dois e trocado. So o 'é', que ela nao tem.
        let table = TblTable::parse("80=a\n81=c\n82=o\n83=e\n84=s\n90=ã\n91=çõ\n").unwrap();
        assert_eq!(
            table.drop_missing_accents("ações é ã").as_deref(),
            Some("ações e ã")
        );
    }

    #[test]
    fn slot_is_filled_with_terminator_or_the_table_space() {
        let table = TblTable::parse("80=A\n81=B\n40= \n").unwrap();
        assert_eq!(
            table.encode_slot("AB", 4, true).unwrap(),
            [0x80, 0x81, 0, 0]
        );
        assert_eq!(
            table.encode_slot("AB", 4, false).unwrap(),
            [0x80, 0x81, 0x40, 0x40]
        );
        assert_eq!(table.encode_slot("AB", 2, false).unwrap(), [0x80, 0x81]);
        // Maior que o espaco: volta sem preenchimento, quem chama decide.
        assert_eq!(table.encode_slot("ABA", 2, false).unwrap().len(), 3);

        // Sem espaco na tabela, largura fixa so aceita o tamanho exato.
        let bare = TblTable::parse("80=A\n81=B\n").unwrap();
        assert_eq!(bare.encode_slot("AB", 2, false).unwrap(), [0x80, 0x81]);
        assert_eq!(bare.encode_slot("AB", 4, true).unwrap(), [0x80, 0x81, 0, 0]);
        let err = bare.encode_slot("AB", 4, false).unwrap_err().to_string();
        assert!(err.contains("exatamente 4"), "{err}");
        // Espaco de 2 bytes nao completa sobra impar.
        let wide = TblTable::parse("80=A\nFF20= \n").unwrap();
        assert_eq!(wide.encode_slot("A", 3, false).unwrap(), [0x80, 0xFF, 0x20]);
        assert!(wide.encode_slot("A", 4, false).is_err());
    }
}

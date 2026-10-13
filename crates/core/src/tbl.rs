//! Loader de tabelas de caracteres `.tbl` (formato classico de romhacking):
//! uma entrada por linha, `HEX=texto`, com chaves de 1 a 4 bytes.
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

#[derive(Debug, Clone)]
pub struct TblTable {
    entries: HashMap<Vec<u8>, String>,
    max_key_len: usize,
    /// Caminho inverso: texto -> bytes. Dois bytes com o mesmo texto: vale a
    /// MENOR chave (a mais curta; no empate, a de menor valor).
    reverse: HashMap<String, Vec<u8>>,
    /// Maior texto de uma entrada, em caracteres.
    max_text_chars: usize,
}

impl TblTable {
    pub fn parse(text: &str) -> Result<Self> {
        let mut entries = HashMap::new();
        let mut max_key_len = 0;
        for (idx, raw) in text.lines().enumerate() {
            let line = raw.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let n = idx + 1;
            let Some((hex, value)) = line.split_once('=') else {
                return Err(CoreError::Tbl(format!("linha {n}: falta '=' ({line})")));
            };
            let key = decode_hex(hex).ok_or_else(|| {
                CoreError::Tbl(format!(
                    "linha {n}: chave hex invalida \"{hex}\" (esperado 2-8 digitos hex)"
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
    /// Tabela japonesa nao tem a letra "a": tem "ａ", a de largura inteira.
    /// O caractere ASCII que falta e gravado nessa forma (`wide_form`), e e
    /// assim que ele sera lido de volta.
    pub fn encode(&self, text: &str) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        // O texto como a tabela vai le-lo de volta.
        let mut written = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(ch) = rest.chars().next() {
            if let Some((end, bytes)) = self.longest_text(rest) {
                out.extend_from_slice(bytes);
                written.push_str(&rest[..end]);
                rest = &rest[end..];
                continue;
            }
            let wide =
                wide_form(ch).and_then(|w| Some((w, self.reverse.get(w.to_string().as_str())?)));
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

    /// A tabela escreve `ch`, como ele e ou na forma de largura inteira.
    fn writes(&self, ch: char) -> bool {
        let has = |c: char| self.reverse.contains_key(c.to_string().as_str());
        has(ch) || wide_form(ch).is_some_and(has)
    }

    /// A maior entrada da tabela no comeco de `rest`: onde ela termina e os
    /// bytes dela.
    fn longest_text(&self, rest: &str) -> Option<(usize, &Vec<u8>)> {
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
            return (' '..='~')
                .filter(|c| self.writes(*c))
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
    pub fn chars_that_fit(&self, slot: usize) -> usize {
        self.reverse
            .iter()
            .map(|(text, key)| slot.saturating_mul(text.chars().count()) / key.len())
            .min()
            .unwrap_or(0)
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
            if let Some((end, _)) = self.longest_text(rest) {
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
        let mut bytes = self.encode(text)?;
        let Some(room) = slot.checked_sub(bytes.len()).filter(|&room| room > 0) else {
            return Ok(bytes);
        };
        if terminated {
            bytes.resize(slot, 0x00);
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

/// Grava a tabela `id` no projeto se ele ainda nao tem uma com esse nome: e
/// a tabela que a extracao montou sozinha. A que ja existe fica como esta —
/// pode ter sido completada a mao.
pub fn ensure_table(project_dir: &Path, id: &str, text: &str) -> Result<()> {
    let dir = project_dir.join(TABLES_DIR);
    let dest = dir.join(format!("{id}.tbl"));
    if dest.exists() {
        return Ok(());
    }
    TblTable::parse(text)?;
    fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    fs::write(&dest, text).map_err(|e| CoreError::io(&dest, e))
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
fn base_letter(ch: char) -> Option<char> {
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
    if hex.is_empty() || !hex.len().is_multiple_of(2) || hex.len() > 8 {
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

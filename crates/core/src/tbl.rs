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

use crate::error::{CoreError, Result};
use crate::types::{TextEncoding, TextEntry};

/// Subdiretorio do projeto com as tabelas de caracteres dele.
pub const TABLES_DIR: &str = "tables";

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
    pub fn encode(&self, text: &str) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            // Onde terminam os prefixos de 1 a `max_text_chars` caracteres.
            let ends: Vec<usize> = rest
                .char_indices()
                .skip(1)
                .map(|(i, _)| i)
                .chain([rest.len()])
                .take(self.max_text_chars)
                .collect();
            let hit = ends
                .iter()
                .rev()
                .find_map(|&end| self.reverse.get(&rest[..end]).map(|bytes| (end, bytes)));
            let Some((end, bytes)) = hit else {
                let ch = rest.chars().next().unwrap_or_default();
                let position = text[..text.len() - rest.len()].chars().count() + 1;
                return Err(CoreError::Tbl(format!(
                    "o caractere {ch:?} (posicao {position}) nao existe na tabela"
                )));
            };
            out.extend_from_slice(bytes);
            rest = &rest[end..];
        }
        // Chave de 2+ bytes que comeca por outra chave: os bytes certos, lado
        // a lado, podem ser lidos como outro texto.
        let read_back = self.decode(&out);
        if read_back.as_deref() != Some(text) {
            return Err(CoreError::Tbl(format!(
                "tabela ambigua: \"{text}\" gravado seria lido de volta como \"{}\"",
                read_back.unwrap_or_default()
            )));
        }
        Ok(out)
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

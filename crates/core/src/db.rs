//! Banco SQLite do projeto (`<dir>.rtsproj/translations.sqlite`): entries
//! extraidas, translation memory e glossario (spec §13, §20). Um DB por
//! projeto; TM global cross-projeto fica para depois.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::error::{CoreError, Result};
use crate::provider::GlossaryTerm;
use crate::scan::looks_like_text;
use crate::types::{TextEncoding, TextEntry, TranslationStatus};

pub const DB_FILE: &str = "translations.sqlite";

pub struct ProjectDb {
    conn: Connection,
    project_dir: PathBuf,
}

impl ProjectDb {
    /// Abre (criando se preciso) o banco do projeto e garante o schema.
    pub fn open(project_dir: &Path) -> Result<Self> {
        let path = project_dir.join(DB_FILE);
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id TEXT PRIMARY KEY,
                resource_path TEXT,
                offset INTEGER,
                original_bytes BLOB NOT NULL,
                source_text TEXT NOT NULL,
                translated_text TEXT,
                context TEXT,
                max_bytes INTEGER,
                encoding TEXT NOT NULL,
                status TEXT NOT NULL,
                metadata TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tm (
                normalized TEXT NOT NULL,
                source_language TEXT NOT NULL,
                target_language TEXT NOT NULL,
                translation TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (normalized, source_language, target_language)
            );
            CREATE TABLE IF NOT EXISTS glossary (
                term TEXT PRIMARY KEY,
                translation TEXT,
                no_translate INTEGER NOT NULL DEFAULT 0,
                case_sensitive INTEGER NOT NULL DEFAULT 0,
                note TEXT
            );
            CREATE TABLE IF NOT EXISTS provider_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                translated INTEGER NOT NULL DEFAULT 0,
                tm_hits INTEGER NOT NULL DEFAULT 0,
                failed INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS accent_swaps (
                entry_id TEXT PRIMARY KEY,
                answer TEXT NOT NULL,
                written TEXT NOT NULL
            );",
        )?;
        Ok(ProjectDb {
            conn,
            project_dir: project_dir.to_path_buf(),
        })
    }

    /// Diretorio do projeto deste banco (as tabelas `.tbl` moram ao lado).
    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    /// Upsert de entries preservando traducao ja existente quando a nova vem vazia
    /// (re-scan nao apaga trabalho feito).
    pub fn upsert_entries(&mut self, entries: &[TextEntry]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO entries
                 (id, resource_path, offset, original_bytes, source_text, translated_text,
                  context, max_bytes, encoding, status, metadata)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
                 ON CONFLICT(id) DO UPDATE SET
                   resource_path=excluded.resource_path,
                   offset=excluded.offset,
                   original_bytes=excluded.original_bytes,
                   source_text=excluded.source_text,
                   context=excluded.context,
                   max_bytes=excluded.max_bytes,
                   encoding=excluded.encoding,
                   metadata=excluded.metadata,
                   translated_text=COALESCE(excluded.translated_text, entries.translated_text),
                   status=CASE
                     WHEN excluded.translated_text IS NULL AND entries.translated_text IS NOT NULL
                     THEN entries.status ELSE excluded.status END",
            )?;
            for e in entries {
                stmt.execute(params![
                    e.id,
                    e.resource_path,
                    e.offset.map(|o| o as i64),
                    e.original_bytes,
                    e.source_text,
                    e.translated_text,
                    e.context,
                    e.max_bytes.map(|m| m as i64),
                    json_str(&e.encoding)?,
                    json_str(&e.status)?,
                    e.metadata.to_string(),
                ])?;
            }
        }
        tx.commit()?;
        Ok(entries.len())
    }

    /// Apaga as entries antigas de lixo que a extracao estruturada deixou de
    /// devolver e diz quantas sairam. So sai a entry que cumpre TUDO: sem
    /// traducao, fora de `extracted`, sem ponteiro em tabela, nao lida por
    /// tabela de caracteres (essa veio de um scan com `.tbl` que o usuario
    /// pediu, e a regra e fraca pra ela) e que nao passa em
    /// `scan::looks_like_text`.
    pub fn remove_stale_junk(&mut self, extracted: &[TextEntry]) -> Result<usize> {
        let extracted: HashSet<&str> = extracted.iter().map(|e| e.id.as_str()).collect();
        // Mesma conexao: a leitura abaixo ja roda dentro da transacao.
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let mut removed = 0;
        {
            let mut delete =
                tx.prepare("DELETE FROM entries WHERE id=?1 AND translated_text IS NULL")?;
            for e in self.load_entries()? {
                if e.translated_text.is_none()
                    && !extracted.contains(e.id.as_str())
                    && e.pointers().is_empty()
                    && !matches!(e.encoding, TextEncoding::Table(_))
                    && !looks_like_text(&e.source_text)
                {
                    removed += delete.execute([&e.id])?;
                }
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Tira do projeto a leitura antiga que a extracao nova trocou por outra:
    /// a entry que nao voltou, no mesmo encoding de uma que voltou e em cima
    /// dos mesmos bytes — tenha traducao ou nao. Sem isto, depois de uma
    /// regra de leitura nova as duas disputavam os bytes e a reinsercao
    /// parava. Entry de outro encoding (o Escanear com uma tabela da pessoa)
    /// fica. Devolve quantas saíram.
    pub fn remove_overlapped(&mut self, extracted: &[TextEntry]) -> Result<usize> {
        let ids: HashSet<&str> = extracted.iter().map(|e| e.id.as_str()).collect();
        let mut spans: Vec<(u64, u64, &TextEncoding)> = extracted
            .iter()
            .filter_map(|e| {
                Some((
                    e.offset?,
                    e.offset? + e.original_bytes.len() as u64,
                    &e.encoding,
                ))
            })
            .collect();
        spans.sort_unstable_by_key(|span| span.0);
        let overlaps = |entry: &TextEntry| {
            let Some(start) = entry.offset else {
                return false;
            };
            let end = start + entry.original_bytes.len() as u64;
            let i = spans.partition_point(|span| span.0 < end);
            spans[..i]
                .iter()
                .rev()
                .take_while(|span| span.0 + 64 * 1024 > start)
                .any(|span| span.1 > start && span.2 == &entry.encoding)
        };
        let stale: Vec<String> = self
            .load_entries()?
            .into_iter()
            .filter(|e| !ids.contains(e.id.as_str()) && overlaps(e))
            .map(|e| e.id)
            .collect();
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let mut removed = 0;
        {
            let mut delete = tx.prepare("DELETE FROM entries WHERE id=?1")?;
            for id in &stale {
                removed += delete.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Apaga as entries de `ids` e diz quantas sairam.
    pub fn remove_entries(&mut self, ids: &[String]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut removed = 0;
        {
            let mut delete = tx.prepare("DELETE FROM entries WHERE id = ?1")?;
            for id in ids {
                removed += delete.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    pub fn load_entries(&self) -> Result<Vec<TextEntry>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ENTRY_COLUMNS} FROM entries ORDER BY offset IS NULL, offset, id"
        ))?;
        let rows = stmt.query_map([], entry_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn update_translation(
        &mut self,
        id: &str,
        translation: &str,
        status: TranslationStatus,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE entries SET translated_text=?2, status=?3 WHERE id=?1",
            params![id, translation, json_str(&status)?],
        )?;
        Ok(())
    }

    pub fn set_status(&mut self, id: &str, status: TranslationStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE entries SET status=?2 WHERE id=?1",
            params![id, json_str(&status)?],
        )?;
        Ok(())
    }

    /// Tira da traducao as entries soltas (`scan::stray_ids`): viram
    /// `Internal`, que nao vai pro provider nem e regravado, e a traducao
    /// automatica que alguma ja tinha recebido e descartada — ela seria
    /// gravada por cima de dado do jogo. Entry revisada por uma pessoa fica
    /// como esta. Devolve quantas mudaram.
    pub fn mark_stray(&mut self, ids: &[String]) -> Result<usize> {
        let internal = json_str(&TranslationStatus::Internal)?;
        let reviewed = json_str(&TranslationStatus::Reviewed)?;
        let tx = self.conn.transaction()?;
        let mut changed = 0;
        {
            let mut stmt = tx.prepare(
                "UPDATE entries SET status=?2, translated_text=NULL
                 WHERE id=?1 AND status NOT IN (?2, ?3)",
            )?;
            for id in ids {
                changed += stmt.execute(params![id, internal, reviewed])?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// A traducao automatica de `id` veio como `answer` e foi gravada como
    /// `written`: acentos que a tabela do jogo nao tem viraram a letra base.
    /// Fica fora de `entries.metadata` porque um novo scan troca a metadata.
    pub fn record_accent_swap(&mut self, id: &str, answer: &str, written: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO accent_swaps (entry_id, answer, written) VALUES (?1,?2,?3)",
            params![id, answer, written],
        )?;
        Ok(())
    }

    /// Trocas de acento registradas: id da entry -> (resposta, gravado).
    pub fn accent_swaps(&self) -> Result<HashMap<String, (String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT entry_id, answer, written FROM accent_swaps")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?;
        let mut out = HashMap::new();
        for row in rows {
            let (id, swap) = row?;
            out.insert(id, swap);
        }
        Ok(out)
    }

    /// A entry de id `id`. Busca so a linha dela: carregar o banco inteiro
    /// pra achar uma frase custou 10 minutos ao gravar 11 mil traducoes de
    /// uma vez pela CLI (13/10/2026).
    pub fn get_entry(&self, id: &str) -> Result<Option<TextEntry>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {ENTRY_COLUMNS} FROM entries WHERE id=?1"))?;
        let mut rows = stmt.query_map([id], entry_from_row)?;
        Ok(rows.next().transpose()?)
    }

    // ---- Translation memory ----

    pub fn tm_lookup(
        &self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
    ) -> Result<Option<String>> {
        tm_lookup_conn(&self.conn, source_text, source_language, target_language)
    }

    pub fn tm_store(
        &mut self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
        translation: &str,
    ) -> Result<()> {
        tm_store_conn(
            &self.conn,
            source_text,
            source_language,
            target_language,
            translation,
        )
    }

    // ---- Glossario ----

    pub fn glossary_list(&self) -> Result<Vec<GlossaryTerm>> {
        let mut stmt = self.conn.prepare(
            "SELECT term, translation, no_translate, case_sensitive, note
             FROM glossary ORDER BY term COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(GlossaryTerm {
                term: row.get(0)?,
                translation: row.get(1)?,
                no_translate: row.get::<_, i64>(2)? != 0,
                case_sensitive: row.get::<_, i64>(3)? != 0,
                note: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn glossary_upsert(&mut self, term: &GlossaryTerm) -> Result<()> {
        if term.term.trim().is_empty() {
            return Err(CoreError::Project("termo do glossario vazio".to_string()));
        }
        self.conn.execute(
            "INSERT INTO glossary (term, translation, no_translate, case_sensitive, note)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(term) DO UPDATE SET
               translation=excluded.translation,
               no_translate=excluded.no_translate,
               case_sensitive=excluded.case_sensitive,
               note=excluded.note",
            params![
                term.term.trim(),
                term.translation,
                term.no_translate as i64,
                term.case_sensitive as i64,
                term.note,
            ],
        )?;
        Ok(())
    }

    pub fn glossary_delete(&mut self, term: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM glossary WHERE term=?1", params![term])?;
        Ok(())
    }

    // ---- Runs ----

    pub fn record_run(
        &mut self,
        provider: &str,
        model: &str,
        started_at: &str,
        translated: usize,
        tm_hits: usize,
        failed: usize,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO provider_runs
             (provider, model, started_at, finished_at, translated, tm_hits, failed)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                provider,
                model,
                started_at,
                Utc::now().to_rfc3339(),
                translated as i64,
                tm_hits as i64,
                failed as i64,
            ],
        )?;
        Ok(())
    }
}

fn tm_lookup_conn(
    conn: &Connection,
    source_text: &str,
    source_language: &str,
    target_language: &str,
) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT translation FROM tm
             WHERE normalized=?1 AND source_language=?2 AND target_language=?3",
            params![
                normalize_source(source_text),
                source_language,
                target_language
            ],
            |row| row.get(0),
        )
        .optional()?)
}

fn tm_store_conn(
    conn: &Connection,
    source_text: &str,
    source_language: &str,
    target_language: &str,
    translation: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO tm (normalized, source_language, target_language, translation, updated_at)
         VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(normalized, source_language, target_language)
         DO UPDATE SET translation=excluded.translation, updated_at=excluded.updated_at",
        params![
            normalize_source(source_text),
            source_language,
            target_language,
            translation,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Translation memory GLOBAL (cross-projeto), no config dir do app.
/// Consultada em cascata DEPOIS da TM do projeto; alimentada por toda
/// traducao nova (maquina ou manual). Mesma chave/normalizacao da TM local.
pub struct GlobalTm {
    conn: Connection,
}

impl GlobalTm {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Mesma DDL da tabela `tm` do ProjectDb.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tm (
                normalized TEXT NOT NULL,
                source_language TEXT NOT NULL,
                target_language TEXT NOT NULL,
                translation TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (normalized, source_language, target_language)
            );",
        )?;
        Ok(GlobalTm { conn })
    }

    pub fn tm_lookup(
        &self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
    ) -> Result<Option<String>> {
        tm_lookup_conn(&self.conn, source_text, source_language, target_language)
    }

    pub fn tm_store(
        &mut self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
        translation: &str,
    ) -> Result<()> {
        tm_store_conn(
            &self.conn,
            source_text,
            source_language,
            target_language,
            translation,
        )
    }
}

/// As colunas de uma entry, na ordem que `entry_from_row` le.
const ENTRY_COLUMNS: &str = "id, resource_path, offset, original_bytes, source_text, \
     translated_text, context, max_bytes, encoding, status, metadata";

fn entry_from_row(row: &rusqlite::Row) -> rusqlite::Result<TextEntry> {
    Ok(TextEntry {
        id: row.get(0)?,
        resource_path: row.get(1)?,
        offset: row.get::<_, Option<i64>>(2)?.map(|o| o as u64),
        original_bytes: row.get(3)?,
        source_text: row.get(4)?,
        translated_text: row.get(5)?,
        context: row.get(6)?,
        max_bytes: row.get::<_, Option<i64>>(7)?.map(|m| m as usize),
        encoding: json_val(&row.get::<_, String>(8)?),
        status: json_val(&row.get::<_, String>(9)?),
        metadata: serde_json::from_str(&row.get::<_, String>(10)?)
            .unwrap_or(serde_json::Value::Null),
    })
}

/// Chave da TM (spec §13): texto normalizado (trim + espacos colapsados),
/// case preservado — em jogo, "SAVE" e "Save" sao strings diferentes.
pub fn normalize_source(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn json_str<T: serde::Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

/// Deserializa enum gravado como JSON; default seguro se algo estiver corrompido.
fn json_val<T: serde::de::DeserializeOwned + Default>(s: &str) -> T {
    serde_json::from_str(s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_whitespace_keeps_case() {
        assert_eq!(normalize_source("  Hello   World \n"), "Hello World");
        assert_eq!(normalize_source("SAVE"), "SAVE");
        assert_ne!(normalize_source("SAVE"), normalize_source("save"));
    }
}

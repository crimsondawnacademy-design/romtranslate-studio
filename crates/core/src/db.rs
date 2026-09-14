//! Banco SQLite do projeto (`<dir>.rtsproj/translations.sqlite`): entries
//! extraidas, translation memory e glossario (spec §13, §20). Um DB por
//! projeto; TM global cross-projeto fica para depois.

use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{CoreError, Result};
use crate::provider::GlossaryTerm;
use crate::types::{TextEntry, TranslationStatus};

pub const DB_FILE: &str = "translations.sqlite";

pub struct ProjectDb {
    conn: Connection,
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
            );",
        )?;
        Ok(ProjectDb { conn })
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

    pub fn load_entries(&self) -> Result<Vec<TextEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, resource_path, offset, original_bytes, source_text, translated_text,
                    context, max_bytes, encoding, status, metadata
             FROM entries ORDER BY offset IS NULL, offset, id",
        )?;
        let rows = stmt.query_map([], |row| {
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
        })?;
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

    pub fn get_entry(&self, id: &str) -> Result<Option<TextEntry>> {
        Ok(self.load_entries()?.into_iter().find(|e| e.id == id))
        // ponytail: filtra em memoria (poucos milhares de rows); query dedicada
        // se o load completo aparecer em profile.
    }

    // ---- Translation memory ----

    pub fn tm_lookup(
        &self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .conn
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

    pub fn tm_store(
        &mut self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
        translation: &str,
    ) -> Result<()> {
        self.conn.execute(
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

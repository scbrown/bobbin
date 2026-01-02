use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

use crate::types::{Chunk, FileCoupling, FileMetadata, IndexStats, SearchResult};

/// Metadata and FTS storage using SQLite
pub struct MetadataStore {
    conn: Connection,
}

impl MetadataStore {
    /// Open or create a metadata store at the given path
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database: {}", path.display()))?;

        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(r#"
            -- Indexed files
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                language TEXT,
                mtime INTEGER,
                hash TEXT,
                indexed_at INTEGER
            );

            -- Semantic chunks
            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                file_id INTEGER REFERENCES files(id) ON DELETE CASCADE,
                chunk_type TEXT,
                name TEXT,
                start_line INTEGER,
                end_line INTEGER,
                content TEXT,
                vector_id TEXT
            );

            -- Full-text search
            CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                content, name,
                content='chunks',
                content_rowid='rowid'
            );

            -- Triggers to keep FTS in sync
            CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
                INSERT INTO chunks_fts(rowid, content, name)
                VALUES (new.rowid, new.content, new.name);
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, content, name)
                VALUES ('delete', old.rowid, old.content, old.name);
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, content, name)
                VALUES ('delete', old.rowid, old.content, old.name);
                INSERT INTO chunks_fts(rowid, content, name)
                VALUES (new.rowid, new.content, new.name);
            END;

            -- Temporal coupling
            CREATE TABLE IF NOT EXISTS coupling (
                file_a INTEGER REFERENCES files(id) ON DELETE CASCADE,
                file_b INTEGER REFERENCES files(id) ON DELETE CASCADE,
                score REAL,
                co_changes INTEGER,
                last_co_change INTEGER,
                PRIMARY KEY (file_a, file_b)
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_id);
            CREATE INDEX IF NOT EXISTS idx_coupling_score ON coupling(score DESC);
        "#)?;

        Ok(())
    }

    /// Insert or update a file's metadata
    pub fn upsert_file(&self, metadata: &FileMetadata) -> Result<i64> {
        self.conn.execute(
            r#"INSERT INTO files (path, language, mtime, hash, indexed_at)
               VALUES (?1, ?2, ?3, ?4, ?5)
               ON CONFLICT(path) DO UPDATE SET
                   language = excluded.language,
                   mtime = excluded.mtime,
                   hash = excluded.hash,
                   indexed_at = excluded.indexed_at"#,
            (
                &metadata.path,
                &metadata.language,
                metadata.mtime,
                &metadata.hash,
                metadata.indexed_at,
            ),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert a chunk
    pub fn insert_chunk(&self, chunk: &Chunk, file_id: i64) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO chunks (id, file_id, chunk_type, name, start_line, end_line, content, vector_id)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
            (
                &chunk.id,
                file_id,
                chunk.chunk_type.to_string(),
                &chunk.name,
                chunk.start_line,
                chunk.end_line,
                &chunk.content,
                &chunk.id, // vector_id same as chunk id
            ),
        )?;
        Ok(())
    }

    /// Delete chunks for a file
    pub fn delete_file_chunks(&self, file_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM chunks WHERE file_id IN (SELECT id FROM files WHERE path = ?1)",
            [file_path],
        )?;
        Ok(())
    }

    /// Delete a file and its chunks
    pub fn delete_file(&self, file_path: &str) -> Result<()> {
        self.conn.execute("DELETE FROM files WHERE path = ?1", [file_path])?;
        Ok(())
    }

    /// Search using FTS
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT c.id, c.chunk_type, c.name, c.start_line, c.end_line, c.content,
                      f.path, f.language, bm25(chunks_fts) as score
               FROM chunks_fts
               JOIN chunks c ON chunks_fts.rowid = c.rowid
               JOIN files f ON c.file_id = f.id
               WHERE chunks_fts MATCH ?1
               ORDER BY score
               LIMIT ?2"#,
        )?;

        let results = stmt
            .query_map([query, &limit.to_string()], |row| {
                Ok(SearchResult {
                    chunk: Chunk {
                        id: row.get(0)?,
                        chunk_type: parse_chunk_type(&row.get::<_, String>(1)?),
                        name: row.get(2)?,
                        start_line: row.get(3)?,
                        end_line: row.get(4)?,
                        content: row.get(5)?,
                        file_path: row.get(6)?,
                        language: row.get(7)?,
                    },
                    score: row.get(8)?,
                    match_type: Some(crate::types::MatchType::Keyword),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Get file coupling data
    pub fn get_coupling(&self, file_path: &str, limit: usize) -> Result<Vec<FileCoupling>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT f1.path, f2.path, c.score, c.co_changes, c.last_co_change
               FROM coupling c
               JOIN files f1 ON c.file_a = f1.id
               JOIN files f2 ON c.file_b = f2.id
               WHERE f1.path = ?1 OR f2.path = ?1
               ORDER BY c.score DESC
               LIMIT ?2"#,
        )?;

        let results = stmt
            .query_map([file_path, &limit.to_string()], |row| {
                Ok(FileCoupling {
                    file_a: row.get(0)?,
                    file_b: row.get(1)?,
                    score: row.get(2)?,
                    co_changes: row.get(3)?,
                    last_co_change: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Update coupling data
    pub fn upsert_coupling(&self, coupling: &FileCoupling) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO coupling (file_a, file_b, score, co_changes, last_co_change)
               VALUES (
                   (SELECT id FROM files WHERE path = ?1),
                   (SELECT id FROM files WHERE path = ?2),
                   ?3, ?4, ?5
               )
               ON CONFLICT(file_a, file_b) DO UPDATE SET
                   score = excluded.score,
                   co_changes = excluded.co_changes,
                   last_co_change = excluded.last_co_change"#,
            (
                &coupling.file_a,
                &coupling.file_b,
                coupling.score,
                coupling.co_changes,
                coupling.last_co_change,
            ),
        )?;
        Ok(())
    }

    /// Get index statistics
    pub fn get_stats(&self) -> Result<IndexStats> {
        let total_files: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files",
            [],
            |row| row.get(0),
        )?;

        let total_chunks: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM chunks",
            [],
            |row| row.get(0),
        )?;

        let last_indexed: Option<i64> = self.conn.query_row(
            "SELECT MAX(indexed_at) FROM files",
            [],
            |row| row.get(0),
        )?;

        // Get per-language stats
        let mut stmt = self.conn.prepare(
            "SELECT language, COUNT(*) FROM files WHERE language IS NOT NULL GROUP BY language"
        )?;
        let languages = stmt
            .query_map([], |row| {
                Ok(crate::types::LanguageStats {
                    language: row.get(0)?,
                    file_count: row.get(1)?,
                    chunk_count: 0, // TODO: join with chunks
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(IndexStats {
            total_files,
            total_chunks,
            total_embeddings: total_chunks, // Same as chunks for now
            languages,
            last_indexed,
            index_size_bytes: 0, // TODO: Get actual file size
        })
    }
}

fn parse_chunk_type(s: &str) -> crate::types::ChunkType {
    match s {
        "function" => crate::types::ChunkType::Function,
        "method" => crate::types::ChunkType::Method,
        "class" => crate::types::ChunkType::Class,
        "struct" => crate::types::ChunkType::Struct,
        "enum" => crate::types::ChunkType::Enum,
        "interface" => crate::types::ChunkType::Interface,
        "module" => crate::types::ChunkType::Module,
        "impl" => crate::types::ChunkType::Impl,
        "trait" => crate::types::ChunkType::Trait,
        _ => crate::types::ChunkType::Other,
    }
}

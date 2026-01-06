use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
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

        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON", [])
            .context("Failed to enable foreign keys")?;

        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
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

            -- Global metadata
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_id);
            CREATE INDEX IF NOT EXISTS idx_coupling_score ON coupling(score DESC);
        "#,
        )?;

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
    // TODO(bobbin-6vq): For granular chunk insertion
    #[allow(dead_code)]
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
        self.conn
            .execute("DELETE FROM files WHERE path = ?1", [file_path])?;
        Ok(())
    }

    /// Get file metadata by path
    pub fn get_file(&self, file_path: &str) -> Result<Option<FileMetadata>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, language, mtime, hash, indexed_at FROM files WHERE path = ?1")?;

        let result = stmt
            .query_row([file_path], |row| {
                Ok(FileMetadata {
                    path: row.get(0)?,
                    language: row.get(1)?,
                    mtime: row.get(2)?,
                    hash: row.get(3)?,
                    indexed_at: row.get(4)?,
                })
            })
            .optional()?;

        Ok(result)
    }

    /// Get all indexed files
    pub fn get_all_files(&self) -> Result<Vec<FileMetadata>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, language, mtime, hash, indexed_at FROM files ORDER BY path")?;

        let results = stmt
            .query_map([], |row| {
                Ok(FileMetadata {
                    path: row.get(0)?,
                    language: row.get(1)?,
                    mtime: row.get(2)?,
                    hash: row.get(3)?,
                    indexed_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Check if a file needs reindexing based on hash or mtime
    pub fn needs_reindex(
        &self,
        file_path: &str,
        current_hash: &str,
        current_mtime: i64,
    ) -> Result<bool> {
        match self.get_file(file_path)? {
            None => Ok(true), // File not indexed yet
            Some(metadata) => {
                // Reindex if hash changed, or if mtime changed and hash differs
                if metadata.hash != current_hash {
                    Ok(true)
                } else if metadata.mtime != current_mtime {
                    // mtime changed but hash same - update mtime but no reindex needed
                    Ok(false)
                } else {
                    Ok(false)
                }
            }
        }
    }

    /// Get file ID by path
    // TODO(bobbin-6vq): For file lookup operations
    #[allow(dead_code)]
    pub fn get_file_id(&self, file_path: &str) -> Result<Option<i64>> {
        let result = self
            .conn
            .query_row("SELECT id FROM files WHERE path = ?1", [file_path], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(result)
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
               SELECT f1.id, f2.id, ?3, ?4, ?5
               FROM files f1, files f2
               WHERE f1.path = ?1 AND f2.path = ?2
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
        let total_files: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;

        let total_chunks: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;

        let last_indexed: Option<i64> =
            self.conn
                .query_row("SELECT MAX(indexed_at) FROM files", [], |row| row.get(0))?;

        // Get per-language stats with chunk counts
        let mut stmt = self.conn.prepare(
            r#"SELECT f.language, COUNT(DISTINCT f.id), COUNT(c.id)
               FROM files f
               LEFT JOIN chunks c ON f.id = c.file_id
               WHERE f.language IS NOT NULL
               GROUP BY f.language"#,
        )?;
        let languages = stmt
            .query_map([], |row| {
                Ok(crate::types::LanguageStats {
                    language: row.get(0)?,
                    file_count: row.get(1)?,
                    chunk_count: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Get database file size
        let index_size_bytes: u64 = self
            .conn
            .query_row(
                "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(IndexStats {
            total_files,
            total_chunks,
            total_embeddings: total_chunks, // Same as chunks for now
            languages,
            last_indexed,
            index_size_bytes,
        })
    }

    /// Begin a transaction for batch operations
    pub fn begin_transaction(&self) -> Result<()> {
        self.conn.execute("BEGIN TRANSACTION", [])?;
        Ok(())
    }

    /// Commit a transaction
    pub fn commit(&self) -> Result<()> {
        self.conn.execute("COMMIT", [])?;
        Ok(())
    }

    /// Rollback a transaction
    // TODO(bobbin-6vq): For transactional error recovery
    #[allow(dead_code)]
    pub fn rollback(&self) -> Result<()> {
        self.conn.execute("ROLLBACK", [])?;
        Ok(())
    }

    /// Insert multiple chunks in a batch (call within a transaction for best performance)
    pub fn insert_chunks(&self, chunks: &[Chunk], file_id: i64) -> Result<()> {
        let mut stmt = self.conn.prepare(
            r#"INSERT INTO chunks (id, file_id, chunk_type, name, start_line, end_line, content, vector_id)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
        )?;

        for chunk in chunks {
            stmt.execute((
                &chunk.id,
                file_id,
                chunk.chunk_type.to_string(),
                &chunk.name,
                chunk.start_line,
                chunk.end_line,
                &chunk.content,
                &chunk.id,
            ))?;
        }
        Ok(())
    }

    /// Get chunks for a specific file
    // TODO(bobbin-6vq): For file-level chunk retrieval
    #[allow(dead_code)]
    pub fn get_file_chunks(&self, file_path: &str) -> Result<Vec<Chunk>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT c.id, c.chunk_type, c.name, c.start_line, c.end_line, c.content, f.path, f.language
               FROM chunks c
               JOIN files f ON c.file_id = f.id
               WHERE f.path = ?1
               ORDER BY c.start_line"#,
        )?;

        let results = stmt
            .query_map([file_path], |row| {
                Ok(Chunk {
                    id: row.get(0)?,
                    chunk_type: parse_chunk_type(&row.get::<_, String>(1)?),
                    name: row.get(2)?,
                    start_line: row.get(3)?,
                    end_line: row.get(4)?,
                    content: row.get(5)?,
                    file_path: row.get(6)?,
                    language: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Get the database path
    // TODO(bobbin-6vq): For debugging and status display
    #[allow(dead_code)]
    pub fn path(&self) -> Option<String> {
        self.conn.path().map(|p| p.to_owned())
    }

    /// Get global metadata value
    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let result = stmt.query_row([key], |row| row.get(0)).optional()?;
        Ok(result)
    }

    /// Set global metadata value
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            [key, value],
        )?;
        Ok(())
    }

    /// Clear all index data (files, chunks, coupling) but keep global metadata
    pub fn clear_index(&self) -> Result<()> {
        // Deleting from files cascades to chunks and coupling due to foreign keys
        // if ON DELETE CASCADE is set.
        // Schema says:
        // file_id INTEGER REFERENCES files(id) ON DELETE CASCADE
        // file_a INTEGER REFERENCES files(id) ON DELETE CASCADE
        // So just deleting files is enough.
        self.conn.execute("DELETE FROM files", [])?;
        Ok(())
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
        "doc" => crate::types::ChunkType::Doc,
        _ => crate::types::ChunkType::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChunkType;
    use tempfile::tempdir;

    fn create_test_store() -> (MetadataStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = MetadataStore::open(&db_path).unwrap();
        (store, dir)
    }

    fn create_test_file_metadata(path: &str) -> FileMetadata {
        FileMetadata {
            path: path.to_string(),
            language: Some("rust".to_string()),
            mtime: 1234567890,
            hash: "abc123".to_string(),
            indexed_at: 1234567890,
        }
    }

    fn create_test_chunk(id: &str, file_path: &str) -> Chunk {
        Chunk {
            id: id.to_string(),
            file_path: file_path.to_string(),
            chunk_type: ChunkType::Function,
            name: Some("test_function".to_string()),
            start_line: 1,
            end_line: 10,
            content: "fn test_function() { }".to_string(),
            language: "rust".to_string(),
        }
    }

    #[test]
    fn test_open_creates_schema() {
        let (store, _dir) = create_test_store();

        // Schema should be created - verify by checking stats
        let stats = store.get_stats().unwrap();
        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.total_chunks, 0);
    }

    #[test]
    fn test_upsert_and_get_file() {
        let (store, _dir) = create_test_store();
        let metadata = create_test_file_metadata("src/main.rs");

        // Insert file
        let file_id = store.upsert_file(&metadata).unwrap();
        assert!(file_id > 0);

        // Retrieve file
        let retrieved = store.get_file("src/main.rs").unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.path, "src/main.rs");
        assert_eq!(retrieved.language, Some("rust".to_string()));
        assert_eq!(retrieved.hash, "abc123");
    }

    #[test]
    fn test_upsert_file_updates_existing() {
        let (store, _dir) = create_test_store();
        let mut metadata = create_test_file_metadata("src/main.rs");

        // Insert file
        store.upsert_file(&metadata).unwrap();

        // Update file
        metadata.hash = "def456".to_string();
        metadata.mtime = 9999999999;
        store.upsert_file(&metadata).unwrap();

        // Verify update
        let retrieved = store.get_file("src/main.rs").unwrap().unwrap();
        assert_eq!(retrieved.hash, "def456");
        assert_eq!(retrieved.mtime, 9999999999);
    }

    #[test]
    fn test_get_all_files() {
        let (store, _dir) = create_test_store();

        store
            .upsert_file(&create_test_file_metadata("src/a.rs"))
            .unwrap();
        store
            .upsert_file(&create_test_file_metadata("src/b.rs"))
            .unwrap();
        store
            .upsert_file(&create_test_file_metadata("src/c.rs"))
            .unwrap();

        let files = store.get_all_files().unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/a.rs");
        assert_eq!(files[1].path, "src/b.rs");
        assert_eq!(files[2].path, "src/c.rs");
    }

    #[test]
    fn test_needs_reindex() {
        let (store, _dir) = create_test_store();
        let metadata = create_test_file_metadata("src/main.rs");
        store.upsert_file(&metadata).unwrap();

        // Same hash and mtime - no reindex needed
        assert!(!store
            .needs_reindex("src/main.rs", "abc123", 1234567890)
            .unwrap());

        // Different hash - needs reindex
        assert!(store
            .needs_reindex("src/main.rs", "different_hash", 1234567890)
            .unwrap());

        // Different mtime but same hash - no reindex needed
        assert!(!store
            .needs_reindex("src/main.rs", "abc123", 9999999999)
            .unwrap());

        // File not indexed - needs reindex
        assert!(store
            .needs_reindex("src/new_file.rs", "any_hash", 0)
            .unwrap());
    }

    #[test]
    fn test_insert_and_get_chunks() {
        let (store, _dir) = create_test_store();
        let metadata = create_test_file_metadata("src/main.rs");
        let file_id = store.upsert_file(&metadata).unwrap();

        let chunk = create_test_chunk("chunk1", "src/main.rs");
        store.insert_chunk(&chunk, file_id).unwrap();

        let chunks = store.get_file_chunks("src/main.rs").unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].id, "chunk1");
        assert_eq!(chunks[0].name, Some("test_function".to_string()));
    }

    #[test]
    fn test_insert_chunks_batch() {
        let (store, _dir) = create_test_store();
        let metadata = create_test_file_metadata("src/main.rs");
        let file_id = store.upsert_file(&metadata).unwrap();

        let chunks = vec![
            create_test_chunk("chunk1", "src/main.rs"),
            create_test_chunk("chunk2", "src/main.rs"),
            create_test_chunk("chunk3", "src/main.rs"),
        ];

        store.begin_transaction().unwrap();
        store.insert_chunks(&chunks, file_id).unwrap();
        store.commit().unwrap();

        let retrieved = store.get_file_chunks("src/main.rs").unwrap();
        assert_eq!(retrieved.len(), 3);
    }

    #[test]
    fn test_delete_file_chunks() {
        let (store, _dir) = create_test_store();
        let metadata = create_test_file_metadata("src/main.rs");
        let file_id = store.upsert_file(&metadata).unwrap();

        store
            .insert_chunk(&create_test_chunk("chunk1", "src/main.rs"), file_id)
            .unwrap();
        store
            .insert_chunk(&create_test_chunk("chunk2", "src/main.rs"), file_id)
            .unwrap();

        // Verify chunks exist
        assert_eq!(store.get_file_chunks("src/main.rs").unwrap().len(), 2);

        // Delete chunks
        store.delete_file_chunks("src/main.rs").unwrap();

        // Verify chunks deleted
        assert_eq!(store.get_file_chunks("src/main.rs").unwrap().len(), 0);

        // File should still exist
        assert!(store.get_file("src/main.rs").unwrap().is_some());
    }

    #[test]
    fn test_delete_file() {
        let (store, _dir) = create_test_store();
        let metadata = create_test_file_metadata("src/main.rs");
        let file_id = store.upsert_file(&metadata).unwrap();

        store
            .insert_chunk(&create_test_chunk("chunk1", "src/main.rs"), file_id)
            .unwrap();

        // Delete file (should cascade to chunks due to foreign key)
        store.delete_file("src/main.rs").unwrap();

        // Verify file deleted
        assert!(store.get_file("src/main.rs").unwrap().is_none());
    }

    #[test]
    fn test_search_fts() {
        let (store, _dir) = create_test_store();
        let metadata = create_test_file_metadata("src/main.rs");
        let file_id = store.upsert_file(&metadata).unwrap();

        let mut chunk = create_test_chunk("chunk1", "src/main.rs");
        chunk.content =
            "fn calculate_total(items: Vec<Item>) -> i32 { items.iter().sum() }".to_string();
        chunk.name = Some("calculate_total".to_string());
        store.insert_chunk(&chunk, file_id).unwrap();

        // Search by function name
        let results = store.search_fts("calculate_total", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.id, "chunk1");

        // Search by content
        let results = store.search_fts("items", 10).unwrap();
        assert_eq!(results.len(), 1);

        // Search with no results
        let results = store.search_fts("nonexistent", 10).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_get_stats() {
        let (store, _dir) = create_test_store();

        // Add files with different languages
        let mut rust_file = create_test_file_metadata("src/main.rs");
        rust_file.language = Some("rust".to_string());
        let file_id1 = store.upsert_file(&rust_file).unwrap();

        let mut python_file = create_test_file_metadata("src/script.py");
        python_file.language = Some("python".to_string());
        let file_id2 = store.upsert_file(&python_file).unwrap();

        // Add chunks
        store
            .insert_chunk(&create_test_chunk("chunk1", "src/main.rs"), file_id1)
            .unwrap();
        store
            .insert_chunk(&create_test_chunk("chunk2", "src/main.rs"), file_id1)
            .unwrap();
        store
            .insert_chunk(&create_test_chunk("chunk3", "src/script.py"), file_id2)
            .unwrap();

        let stats = store.get_stats().unwrap();
        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.total_chunks, 3);
        assert_eq!(stats.languages.len(), 2);

        // Find rust stats
        let rust_stats = stats
            .languages
            .iter()
            .find(|l| l.language == "rust")
            .unwrap();
        assert_eq!(rust_stats.file_count, 1);
        assert_eq!(rust_stats.chunk_count, 2);

        // Find python stats
        let python_stats = stats
            .languages
            .iter()
            .find(|l| l.language == "python")
            .unwrap();
        assert_eq!(python_stats.file_count, 1);
        assert_eq!(python_stats.chunk_count, 1);
    }

    #[test]
    fn test_coupling() {
        let (store, _dir) = create_test_store();

        // Create files first (required for foreign key)
        store
            .upsert_file(&create_test_file_metadata("src/a.rs"))
            .unwrap();
        store
            .upsert_file(&create_test_file_metadata("src/b.rs"))
            .unwrap();

        let coupling = FileCoupling {
            file_a: "src/a.rs".to_string(),
            file_b: "src/b.rs".to_string(),
            score: 0.85,
            co_changes: 10,
            last_co_change: 1234567890,
        };

        store.upsert_coupling(&coupling).unwrap();

        let retrieved = store.get_coupling("src/a.rs", 10).unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].score, 0.85);
        assert_eq!(retrieved[0].co_changes, 10);
    }

    #[test]
    fn test_transaction_rollback() {
        let (store, _dir) = create_test_store();
        let metadata = create_test_file_metadata("src/main.rs");
        let file_id = store.upsert_file(&metadata).unwrap();

        store.begin_transaction().unwrap();
        store
            .insert_chunk(&create_test_chunk("chunk1", "src/main.rs"), file_id)
            .unwrap();
        store.rollback().unwrap();

        // Chunk should not be persisted
        let chunks = store.get_file_chunks("src/main.rs").unwrap();
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_get_file_id() {
        let (store, _dir) = create_test_store();
        let metadata = create_test_file_metadata("src/main.rs");
        store.upsert_file(&metadata).unwrap();

        let file_id = store.get_file_id("src/main.rs").unwrap();
        assert!(file_id.is_some());

        let no_file_id = store.get_file_id("nonexistent.rs").unwrap();
        assert!(no_file_id.is_none());
    }
}

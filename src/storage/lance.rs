use anyhow::{Context, Result};
use arrow::array::{
    Array, ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator,
    StringArray, UInt32Array,
};
use arrow::datatypes::{DataType, Field, FieldRef, Schema, SchemaRef};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, Table};
use std::path::Path;
use std::sync::Arc;

use crate::types::{Chunk, ChunkType, MatchType, SearchResult};

/// Table name for vector storage
const TABLE_NAME: &str = "vectors";

/// Embedding dimension (all-MiniLM-L6-v2)
const EMBEDDING_DIM: i32 = 384;

/// Vector storage using LanceDB
pub struct VectorStore {
    conn: Connection,
    table: Option<Table>,
}

impl VectorStore {
    /// Open or create a vector store at the given path
    pub async fn open(path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let conn = connect(path.to_str().unwrap())
            .execute()
            .await
            .with_context(|| format!("Failed to open LanceDB at: {}", path.display()))?;

        // Check if table exists
        let tables = conn
            .table_names()
            .execute()
            .await
            .context("Failed to list tables")?;

        let table = if tables.contains(&TABLE_NAME.to_string()) {
            Some(
                conn.open_table(TABLE_NAME)
                    .execute()
                    .await
                    .context("Failed to open vectors table")?,
            )
        } else {
            None
        };

        Ok(Self { conn, table })
    }

    /// Get the inner field for the vector FixedSizeList
    fn vector_field() -> FieldRef {
        Arc::new(Field::new("item", DataType::Float32, true))
    }

    /// Get the Arrow schema for vector records
    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(Self::vector_field(), EMBEDDING_DIM),
                false,
            ),
            Field::new("file_path", DataType::Utf8, false),
            Field::new("chunk_name", DataType::Utf8, true),
            Field::new("chunk_type", DataType::Utf8, false),
            Field::new("start_line", DataType::UInt32, false),
            Field::new("end_line", DataType::UInt32, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("language", DataType::Utf8, false),
        ])
    }

    /// Convert chunks and embeddings to a RecordBatch
    fn to_record_batch(chunks: &[Chunk], embeddings: &[Vec<f32>]) -> Result<RecordBatch> {
        let schema = Arc::new(Self::schema());

        // Collect all data
        let ids: Vec<&str> = chunks.iter().map(|c| c.id.as_str()).collect();
        let file_paths: Vec<&str> = chunks.iter().map(|c| c.file_path.as_str()).collect();
        let chunk_names: Vec<Option<&str>> = chunks.iter().map(|c| c.name.as_deref()).collect();
        let chunk_types: Vec<&str> = chunks
            .iter()
            .map(|c| chunk_type_to_str(&c.chunk_type))
            .collect();
        let start_lines: Vec<u32> = chunks.iter().map(|c| c.start_line).collect();
        let end_lines: Vec<u32> = chunks.iter().map(|c| c.end_line).collect();
        let contents: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let languages: Vec<&str> = chunks.iter().map(|c| c.language.as_str()).collect();

        // Flatten embeddings for FixedSizeList
        let flat_embeddings: Vec<f32> = embeddings.iter().flatten().copied().collect();
        let embedding_values: ArrayRef = Arc::new(Float32Array::from(flat_embeddings));
        let vector_array = FixedSizeListArray::try_new(
            Self::vector_field(),
            EMBEDDING_DIM,
            embedding_values,
            None,
        )
        .context("Failed to create vector array")?;

        let columns: Vec<ArrayRef> = vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(vector_array),
            Arc::new(StringArray::from(file_paths)),
            Arc::new(StringArray::from(chunk_names)),
            Arc::new(StringArray::from(chunk_types)),
            Arc::new(UInt32Array::from(start_lines)),
            Arc::new(UInt32Array::from(end_lines)),
            Arc::new(StringArray::from(contents)),
            Arc::new(StringArray::from(languages)),
        ];

        RecordBatch::try_new(schema, columns).context("Failed to create record batch")
    }

    /// Create a RecordBatchIterator from a batch
    fn batch_to_reader(
        batch: RecordBatch,
        schema: SchemaRef,
    ) -> RecordBatchIterator<
        impl Iterator<Item = std::result::Result<RecordBatch, arrow::error::ArrowError>>,
    > {
        RecordBatchIterator::new(std::iter::once(Ok(batch)), schema)
    }

    /// Insert chunks with their embeddings
    pub async fn insert(&mut self, chunks: &[Chunk], embeddings: &[Vec<f32>]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        if chunks.len() != embeddings.len() {
            anyhow::bail!(
                "Chunks and embeddings must have same length: {} vs {}",
                chunks.len(),
                embeddings.len()
            );
        }

        let schema = Arc::new(Self::schema());
        let batch = Self::to_record_batch(chunks, embeddings)?;

        match &self.table {
            Some(table) => {
                // Delete existing records with same IDs first (upsert behavior)
                let ids: Vec<String> = chunks.iter().map(|c| c.id.clone()).collect();
                self.delete(&ids).await?;

                // Add new records
                let reader = Self::batch_to_reader(batch, schema);
                table
                    .add(reader)
                    .execute()
                    .await
                    .context("Failed to add vectors")?;
            }
            None => {
                // Create table with initial data
                let reader = Self::batch_to_reader(batch, schema);
                let table = self
                    .conn
                    .create_table(TABLE_NAME, reader)
                    .execute()
                    .await
                    .context("Failed to create vectors table")?;
                self.table = Some(table);
            }
        }

        Ok(())
    }

    /// Search for similar vectors using approximate nearest neighbor search
    pub async fn search(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(vec![]), // No data yet
        };

        let results = table
            .vector_search(query_embedding.to_vec())
            .context("Failed to create vector search")?
            .limit(limit)
            .execute()
            .await
            .context("Failed to execute vector search")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect search results")?;

        let mut search_results = Vec::new();

        for batch in batches {
            let ids = batch
                .column_by_name("id")
                .context("Missing id column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("id column has wrong type")?;

            let file_paths = batch
                .column_by_name("file_path")
                .context("Missing file_path column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("file_path column has wrong type")?;

            let chunk_names = batch
                .column_by_name("chunk_name")
                .context("Missing chunk_name column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("chunk_name column has wrong type")?;

            let chunk_types = batch
                .column_by_name("chunk_type")
                .context("Missing chunk_type column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("chunk_type column has wrong type")?;

            let start_lines = batch
                .column_by_name("start_line")
                .context("Missing start_line column")?
                .as_any()
                .downcast_ref::<UInt32Array>()
                .context("start_line column has wrong type")?;

            let end_lines = batch
                .column_by_name("end_line")
                .context("Missing end_line column")?
                .as_any()
                .downcast_ref::<UInt32Array>()
                .context("end_line column has wrong type")?;

            let contents = batch
                .column_by_name("content")
                .context("Missing content column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("content column has wrong type")?;

            let languages = batch
                .column_by_name("language")
                .context("Missing language column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("language column has wrong type")?;

            // LanceDB adds _distance column for search results
            let distances = batch
                .column_by_name("_distance")
                .context("Missing _distance column")?
                .as_any()
                .downcast_ref::<Float32Array>()
                .context("_distance column has wrong type")?;

            for i in 0..batch.num_rows() {
                let chunk = Chunk {
                    id: ids.value(i).to_string(),
                    file_path: file_paths.value(i).to_string(),
                    chunk_type: str_to_chunk_type(chunk_types.value(i)),
                    name: if chunk_names.is_null(i) {
                        None
                    } else {
                        Some(chunk_names.value(i).to_string())
                    },
                    start_line: start_lines.value(i),
                    end_line: end_lines.value(i),
                    content: contents.value(i).to_string(),
                    language: languages.value(i).to_string(),
                };

                // Convert distance to similarity score (1 - distance for L2)
                // Lower distance = more similar = higher score
                let distance = distances.value(i);
                let score = 1.0 / (1.0 + distance);

                search_results.push(SearchResult {
                    chunk,
                    score,
                    match_type: Some(MatchType::Semantic),
                });
            }
        }

        Ok(search_results)
    }

    /// Delete vectors by chunk IDs
    pub async fn delete(&self, chunk_ids: &[String]) -> Result<()> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(()), // No data yet
        };

        if chunk_ids.is_empty() {
            return Ok(());
        }

        // Build a filter expression for deletion
        // LanceDB uses SQL-like filter expressions
        let escaped_ids: Vec<String> = chunk_ids
            .iter()
            .map(|id| format!("'{}'", id.replace('\'', "''")))
            .collect();
        let filter = format!("id IN ({})", escaped_ids.join(", "));

        table
            .delete(&filter)
            .await
            .context("Failed to delete vectors")?;

        Ok(())
    }

    /// Get total vector count
    pub async fn count(&self) -> Result<u64> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(0),
        };

        let count = table
            .count_rows(None)
            .await
            .context("Failed to count vectors")?;

        Ok(count as u64)
    }

    /// Delete all vectors for files matching the given paths
    pub async fn delete_by_file(&self, file_paths: &[String]) -> Result<()> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(()),
        };

        if file_paths.is_empty() {
            return Ok(());
        }

        let escaped_paths: Vec<String> = file_paths
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "''")))
            .collect();
        let filter = format!("file_path IN ({})", escaped_paths.join(", "));

        table
            .delete(&filter)
            .await
            .context("Failed to delete vectors by file")?;

        Ok(())
    }
}

/// Convert ChunkType to string for storage
fn chunk_type_to_str(ct: &ChunkType) -> &'static str {
    match ct {
        ChunkType::Function => "function",
        ChunkType::Method => "method",
        ChunkType::Class => "class",
        ChunkType::Struct => "struct",
        ChunkType::Enum => "enum",
        ChunkType::Interface => "interface",
        ChunkType::Module => "module",
        ChunkType::Impl => "impl",
        ChunkType::Trait => "trait",
        ChunkType::Doc => "doc",
        ChunkType::Other => "other",
    }
}

/// Parse string back to ChunkType
fn str_to_chunk_type(s: &str) -> ChunkType {
    match s {
        "function" => ChunkType::Function,
        "method" => ChunkType::Method,
        "class" => ChunkType::Class,
        "struct" => ChunkType::Struct,
        "enum" => ChunkType::Enum,
        "interface" => ChunkType::Interface,
        "module" => ChunkType::Module,
        "impl" => ChunkType::Impl,
        "trait" => ChunkType::Trait,
        "doc" => ChunkType::Doc,
        _ => ChunkType::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_chunk(id: &str, name: &str) -> Chunk {
        Chunk {
            id: id.to_string(),
            file_path: "src/main.rs".to_string(),
            chunk_type: ChunkType::Function,
            name: Some(name.to_string()),
            start_line: 1,
            end_line: 10,
            content: format!("fn {}() {{ }}", name),
            language: "rust".to_string(),
        }
    }

    fn sample_embedding() -> Vec<f32> {
        // Create a normalized 384-dim embedding
        let mut emb: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        emb.iter_mut().for_each(|x| *x /= norm);
        emb
    }

    #[tokio::test]
    async fn test_open_creates_store() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let store = VectorStore::open(&path).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_insert_and_count() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![
            sample_chunk("chunk1", "main"),
            sample_chunk("chunk2", "helper"),
        ];
        let embeddings = vec![sample_embedding(), sample_embedding()];

        store.insert(&chunks, &embeddings).await.unwrap();

        assert_eq!(store.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_search_returns_results() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![sample_chunk("chunk1", "process_data")];
        let embeddings = vec![sample_embedding()];

        store.insert(&chunks, &embeddings).await.unwrap();

        let results = store.search(&sample_embedding(), 10).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.id, "chunk1");
        assert_eq!(results[0].chunk.name, Some("process_data".to_string()));
        assert!(results[0].score > 0.0);
        assert_eq!(results[0].match_type, Some(MatchType::Semantic));
    }

    #[tokio::test]
    async fn test_delete_removes_vectors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![
            sample_chunk("chunk1", "main"),
            sample_chunk("chunk2", "helper"),
        ];
        let embeddings = vec![sample_embedding(), sample_embedding()];

        store.insert(&chunks, &embeddings).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);

        store.delete(&["chunk1".to_string()]).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_delete_by_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![
            Chunk {
                id: "chunk1".to_string(),
                file_path: "src/a.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("func_a".to_string()),
                start_line: 1,
                end_line: 5,
                content: "fn func_a() {}".to_string(),
                language: "rust".to_string(),
            },
            Chunk {
                id: "chunk2".to_string(),
                file_path: "src/b.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("func_b".to_string()),
                start_line: 1,
                end_line: 5,
                content: "fn func_b() {}".to_string(),
                language: "rust".to_string(),
            },
        ];
        let embeddings = vec![sample_embedding(), sample_embedding()];

        store.insert(&chunks, &embeddings).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);

        store
            .delete_by_file(&["src/a.rs".to_string()])
            .await
            .unwrap();
        assert_eq!(store.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_upsert_behavior() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        // Insert initial chunk
        let chunks = vec![sample_chunk("chunk1", "original")];
        let embeddings = vec![sample_embedding()];
        store.insert(&chunks, &embeddings).await.unwrap();

        // Insert with same ID should replace
        let chunks = vec![sample_chunk("chunk1", "updated")];
        store.insert(&chunks, &embeddings).await.unwrap();

        assert_eq!(store.count().await.unwrap(), 1);

        let results = store.search(&sample_embedding(), 10).await.unwrap();
        assert_eq!(results[0].chunk.name, Some("updated".to_string()));
    }

    #[tokio::test]
    async fn test_empty_operations() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        // Empty insert should be fine
        store.insert(&[], &[]).await.unwrap();

        // Search on empty store should return empty
        let results = store.search(&sample_embedding(), 10).await.unwrap();
        assert!(results.is_empty());

        // Delete on empty store should be fine
        store.delete(&["nonexistent".to_string()]).await.unwrap();
    }

    #[tokio::test]
    async fn test_reopen_persists_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        // Insert data
        {
            let mut store = VectorStore::open(&path).await.unwrap();
            let chunks = vec![sample_chunk("chunk1", "persistent")];
            let embeddings = vec![sample_embedding()];
            store.insert(&chunks, &embeddings).await.unwrap();
        }

        // Reopen and verify
        {
            let store = VectorStore::open(&path).await.unwrap();
            assert_eq!(store.count().await.unwrap(), 1);

            let results = store.search(&sample_embedding(), 10).await.unwrap();
            assert_eq!(results[0].chunk.name, Some("persistent".to_string()));
        }
    }
}

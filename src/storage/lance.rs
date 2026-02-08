use anyhow::{Context, Result};
use arrow::array::{
    Array, ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator,
    StringArray, UInt32Array,
};
use arrow::datatypes::{DataType, Field, FieldRef, Schema, SchemaRef};
use futures::TryStreamExt;
use lance_index::scalar::FullTextSearchQuery;
use lancedb::index::scalar::FtsIndexBuilder;
use lancedb::index::Index;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, Table};
use std::path::Path;
use std::sync::Arc;

use crate::types::{Chunk, ChunkType, FileMetadata, IndexStats, LanguageStats, MatchType, SearchResult};

/// Table name for chunk storage
const TABLE_NAME: &str = "chunks";

/// Default embedding dimension (for backward compatibility)
const DEFAULT_EMBEDDING_DIM: i32 = 384;

/// Extract a named string column from a RecordBatch, returning a Result instead of panicking.
fn string_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .with_context(|| format!("missing column '{name}' in RecordBatch"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("column '{name}' is not a StringArray"))
}

/// Unified chunk storage using LanceDB (vectors + metadata + FTS)
pub struct VectorStore {
    conn: Connection,
    table: Option<Table>,
    /// Embedding dimension used by this store
    embedding_dim: i32,
    /// Whether FTS index has been created for this session
    fts_indexed: bool,
}

impl VectorStore {
    /// Open or create a vector store at the given path
    pub async fn open(path: &Path) -> Result<Self> {
        Self::open_with_dim(path, DEFAULT_EMBEDDING_DIM).await
    }

    /// Open or create a vector store with a specific embedding dimension
    pub async fn open_with_dim(path: &Path, embedding_dim: i32) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let path_str = path
            .to_str()
            .context("non-UTF8 path for LanceDB")?;
        let conn = connect(path_str)
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
                    .context("Failed to open chunks table")?,
            )
        } else {
            None
        };

        Ok(Self {
            conn,
            table,
            embedding_dim,
            fts_indexed: false,
        })
    }

    /// Get the embedding dimension of this store
    pub fn embedding_dim(&self) -> i32 {
        self.embedding_dim
    }

    /// Get the inner field for the vector FixedSizeList
    fn vector_field() -> FieldRef {
        Arc::new(Field::new("item", DataType::Float32, true))
    }

    /// Get the Arrow schema for chunk records
    fn schema(&self) -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(Self::vector_field(), self.embedding_dim),
                false,
            ),
            Field::new("repo", DataType::Utf8, false),
            Field::new("file_path", DataType::Utf8, false),
            Field::new("file_hash", DataType::Utf8, false),
            Field::new("language", DataType::Utf8, false),
            Field::new("chunk_type", DataType::Utf8, false),
            Field::new("chunk_name", DataType::Utf8, true),
            Field::new("start_line", DataType::UInt32, false),
            Field::new("end_line", DataType::UInt32, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("full_context", DataType::Utf8, true),
            Field::new("indexed_at", DataType::Utf8, false),
        ])
    }

    /// Convert chunks and embeddings to a RecordBatch
    ///
    /// `full_contexts` contains the context-enriched text used for embedding.
    /// `None` entries mean the chunk was embedded using its content directly.
    fn to_record_batch(
        &self,
        chunks: &[Chunk],
        embeddings: &[Vec<f32>],
        full_contexts: &[Option<String>],
        repo: &str,
        file_hash: &str,
        indexed_at: &str,
    ) -> Result<RecordBatch> {
        let schema = Arc::new(self.schema());

        let ids: Vec<&str> = chunks.iter().map(|c| c.id.as_str()).collect();
        let repos: Vec<&str> = chunks.iter().map(|_| repo).collect();
        let file_paths: Vec<&str> = chunks.iter().map(|c| c.file_path.as_str()).collect();
        let file_hashes: Vec<&str> = chunks.iter().map(|_| file_hash).collect();
        let languages: Vec<&str> = chunks.iter().map(|c| c.language.as_str()).collect();
        let chunk_types: Vec<&str> = chunks
            .iter()
            .map(|c| chunk_type_to_str(&c.chunk_type))
            .collect();
        let chunk_names: Vec<Option<&str>> = chunks.iter().map(|c| c.name.as_deref()).collect();
        let start_lines: Vec<u32> = chunks.iter().map(|c| c.start_line).collect();
        let end_lines: Vec<u32> = chunks.iter().map(|c| c.end_line).collect();
        let contents: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let full_context_refs: Vec<Option<&str>> = full_contexts
            .iter()
            .map(|c| c.as_deref())
            .collect();
        let indexed_ats: Vec<&str> = chunks.iter().map(|_| indexed_at).collect();

        // Flatten embeddings for FixedSizeList
        let flat_embeddings: Vec<f32> = embeddings.iter().flatten().copied().collect();
        let embedding_values: ArrayRef = Arc::new(Float32Array::from(flat_embeddings));
        let vector_array = FixedSizeListArray::try_new(
            Self::vector_field(),
            self.embedding_dim,
            embedding_values,
            None,
        )
        .context("Failed to create vector array")?;

        let columns: Vec<ArrayRef> = vec![
            Arc::new(StringArray::from(ids)),
            Arc::new(vector_array),
            Arc::new(StringArray::from(repos)),
            Arc::new(StringArray::from(file_paths)),
            Arc::new(StringArray::from(file_hashes)),
            Arc::new(StringArray::from(languages)),
            Arc::new(StringArray::from(chunk_types)),
            Arc::new(StringArray::from(chunk_names)),
            Arc::new(UInt32Array::from(start_lines)),
            Arc::new(UInt32Array::from(end_lines)),
            Arc::new(StringArray::from(contents)),
            Arc::new(StringArray::from(full_context_refs)),
            Arc::new(StringArray::from(indexed_ats)),
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
    ///
    /// `full_contexts` contains the context-enriched text used for embedding.
    /// `None` entries mean the chunk was embedded using its content directly.
    pub async fn insert(
        &mut self,
        chunks: &[Chunk],
        embeddings: &[Vec<f32>],
        full_contexts: &[Option<String>],
        repo: &str,
        file_hash: &str,
        indexed_at: &str,
    ) -> Result<()> {
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

        if chunks.len() != full_contexts.len() {
            anyhow::bail!(
                "Chunks and full_contexts must have same length: {} vs {}",
                chunks.len(),
                full_contexts.len()
            );
        }

        let schema = Arc::new(self.schema());
        let batch = self.to_record_batch(chunks, embeddings, full_contexts, repo, file_hash, indexed_at)?;

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
                    .context("Failed to add chunks")?;
            }
            None => {
                // Create table with initial data
                let reader = Self::batch_to_reader(batch, schema);
                let table = self
                    .conn
                    .create_table(TABLE_NAME, reader)
                    .execute()
                    .await
                    .context("Failed to create chunks table")?;
                self.table = Some(table);
            }
        }

        // Invalidate FTS index since data changed
        self.fts_indexed = false;

        Ok(())
    }

    /// Ensure FTS index exists on the content column.
    ///
    /// LanceDB 0.17 does not support multi-column (composite) FTS indexes,
    /// so we index only the `content` column which contains the actual code text.
    pub async fn ensure_fts_index(&mut self) -> Result<()> {
        if self.fts_indexed {
            return Ok(());
        }

        let table = match &self.table {
            Some(t) => t,
            None => return Ok(()),
        };

        table
            .create_index(
                &["content"],
                Index::FTS(FtsIndexBuilder::default()),
            )
            .execute()
            .await
            .context("Failed to create FTS index")?;

        self.fts_indexed = true;
        Ok(())
    }

    /// Search for similar vectors using approximate nearest neighbor search
    /// Optionally filter by repo name
    pub async fn search(&self, query_embedding: &[f32], limit: usize, repo: Option<&str>) -> Result<Vec<SearchResult>> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        let mut query = table
            .vector_search(query_embedding.to_vec())
            .context("Failed to create vector search")?;

        if let Some(repo_name) = repo {
            query = query.only_if(format!("repo = '{}'", repo_name.replace('\'', "''")));
        }

        let results = query
            .limit(limit)
            .execute()
            .await
            .context("Failed to execute vector search")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect search results")?;

        Self::batches_to_results(&batches, MatchType::Semantic)
    }

    /// Full-text search on content and chunk_name
    /// Optionally filter by repo name
    pub async fn search_fts(&mut self, query: &str, limit: usize, repo: Option<&str>) -> Result<Vec<SearchResult>> {
        // Ensure FTS index exists (must be called before borrowing self.table)
        self.ensure_fts_index().await?;

        let table = match &self.table {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        let mut q = table
            .query()
            .full_text_search(FullTextSearchQuery::new(query.to_string()));

        if let Some(repo_name) = repo {
            q = q.only_if(format!("repo = '{}'", repo_name.replace('\'', "''")));
        }

        let results = q
            .limit(limit)
            .execute()
            .await
            .context("Failed to execute FTS search")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect FTS results")?;

        Self::batches_to_fts_results(&batches)
    }

    /// Convert RecordBatches to SearchResults (for vector search with _distance)
    fn batches_to_results(batches: &[RecordBatch], match_type: MatchType) -> Result<Vec<SearchResult>> {
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

                let distance = distances.value(i);
                let score = 1.0 / (1.0 + distance);

                search_results.push(SearchResult {
                    chunk,
                    score,
                    match_type: Some(match_type),
                });
            }
        }

        Ok(search_results)
    }

    /// Convert RecordBatches to SearchResults (for FTS, using _score)
    fn batches_to_fts_results(batches: &[RecordBatch]) -> Result<Vec<SearchResult>> {
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

            // FTS returns _score (BM25 relevance score)
            let scores = batch
                .column_by_name("_score")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

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

                let score = scores.map(|s| s.value(i)).unwrap_or(1.0);

                search_results.push(SearchResult {
                    chunk,
                    score,
                    match_type: Some(MatchType::Keyword),
                });
            }
        }

        Ok(search_results)
    }

    /// Get the stored embedding vector for a chunk by its ID
    pub async fn get_chunk_embedding(&self, chunk_id: &str) -> Result<Option<Vec<f32>>> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(None),
        };

        let filter = format!("id = '{}'", chunk_id.replace('\'', "''"));

        let results = table
            .query()
            .only_if(filter)
            .select(lancedb::query::Select::Columns(vec!["vector".to_string()]))
            .limit(1)
            .execute()
            .await
            .context("Failed to query chunk embedding")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect chunk embedding")?;

        for batch in &batches {
            if batch.num_rows() == 0 {
                continue;
            }

            let vectors = batch
                .column_by_name("vector")
                .context("Missing vector column")?
                .as_any()
                .downcast_ref::<FixedSizeListArray>()
                .context("vector column has wrong type")?;

            let value_arr = vectors.value(0);
            let values = value_arr
                .as_any()
                .downcast_ref::<Float32Array>()
                .context("vector values have wrong type")?;

            return Ok(Some(values.values().to_vec()));
        }

        Ok(None)
    }

    /// Get a single chunk by its ID
    pub async fn get_chunk_by_id(&self, chunk_id: &str) -> Result<Option<Chunk>> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(None),
        };

        let filter = format!("id = '{}'", chunk_id.replace('\'', "''"));

        let results = table
            .query()
            .only_if(filter)
            .execute()
            .await
            .context("Failed to query chunk by ID")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect chunk by ID")?;

        for batch in &batches {
            if batch.num_rows() == 0 {
                continue;
            }

            let ids = batch.column_by_name("id").context("Missing id column")?
                .as_any().downcast_ref::<StringArray>().context("id column has wrong type")?;
            let file_paths = batch.column_by_name("file_path").context("Missing file_path column")?
                .as_any().downcast_ref::<StringArray>().context("file_path column has wrong type")?;
            let chunk_names = batch.column_by_name("chunk_name").context("Missing chunk_name column")?
                .as_any().downcast_ref::<StringArray>().context("chunk_name column has wrong type")?;
            let chunk_types = batch.column_by_name("chunk_type").context("Missing chunk_type column")?
                .as_any().downcast_ref::<StringArray>().context("chunk_type column has wrong type")?;
            let start_lines = batch.column_by_name("start_line").context("Missing start_line column")?
                .as_any().downcast_ref::<UInt32Array>().context("start_line column has wrong type")?;
            let end_lines = batch.column_by_name("end_line").context("Missing end_line column")?
                .as_any().downcast_ref::<UInt32Array>().context("end_line column has wrong type")?;
            let contents = batch.column_by_name("content").context("Missing content column")?
                .as_any().downcast_ref::<StringArray>().context("content column has wrong type")?;
            let languages = batch.column_by_name("language").context("Missing language column")?
                .as_any().downcast_ref::<StringArray>().context("language column has wrong type")?;

            return Ok(Some(Chunk {
                id: ids.value(0).to_string(),
                file_path: file_paths.value(0).to_string(),
                chunk_type: str_to_chunk_type(chunk_types.value(0)),
                name: if chunk_names.is_null(0) {
                    None
                } else {
                    Some(chunk_names.value(0).to_string())
                },
                start_line: start_lines.value(0),
                end_line: end_lines.value(0),
                content: contents.value(0).to_string(),
                language: languages.value(0).to_string(),
            }));
        }

        Ok(None)
    }

    /// Delete vectors by chunk IDs
    pub async fn delete(&self, chunk_ids: &[String]) -> Result<()> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(()),
        };

        if chunk_ids.is_empty() {
            return Ok(());
        }

        let escaped_ids: Vec<String> = chunk_ids
            .iter()
            .map(|id| format!("'{}'", id.replace('\'', "''")))
            .collect();
        let filter = format!("id IN ({})", escaped_ids.join(", "));

        table
            .delete(&filter)
            .await
            .context("Failed to delete chunks")?;

        Ok(())
    }

    /// Get total chunk count
    pub async fn count(&self) -> Result<u64> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(0),
        };

        let count = table
            .count_rows(None)
            .await
            .context("Failed to count chunks")?;

        Ok(count as u64)
    }

    /// Delete all chunks for files matching the given paths
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
            .context("Failed to delete chunks by file")?;

        Ok(())
    }

    /// Check if a file needs reindexing by comparing hash
    pub async fn needs_reindex(&self, file_path: &str, current_hash: &str) -> Result<bool> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(true), // No data yet, needs indexing
        };

        // Query for any chunk from this file and check the file_hash
        let filter = format!("file_path = '{}'", file_path.replace('\'', "''"));
        let results = table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await
            .context("Failed to query file hash")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect file hash results")?;

        if batches.is_empty() || batches[0].num_rows() == 0 {
            return Ok(true); // File not indexed yet
        }

        let file_hashes = batches[0]
            .column_by_name("file_hash")
            .context("Missing file_hash column")?
            .as_any()
            .downcast_ref::<StringArray>()
            .context("file_hash column has wrong type")?;

        let stored_hash = file_hashes.value(0);
        Ok(stored_hash != current_hash)
    }

    /// Get all indexed file paths, optionally filtered by repo
    pub async fn get_all_file_paths(&self, repo: Option<&str>) -> Result<Vec<String>> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        let mut q = table
            .query()
            .select(lancedb::query::Select::Columns(vec!["file_path".to_string()]));

        if let Some(repo_name) = repo {
            q = q.only_if(format!("repo = '{}'", repo_name.replace('\'', "''")));
        }

        let results = q
            .execute()
            .await
            .context("Failed to query file paths")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect file paths")?;

        let mut paths = std::collections::HashSet::new();
        for batch in &batches {
            let file_paths = batch
                .column_by_name("file_path")
                .context("Missing file_path column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("file_path column has wrong type")?;

            for i in 0..batch.num_rows() {
                paths.insert(file_paths.value(i).to_string());
            }
        }

        Ok(paths.into_iter().collect())
    }

    /// Get all unique repo names in the index
    pub async fn get_all_repos(&self) -> Result<Vec<String>> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        let results = table
            .query()
            .select(lancedb::query::Select::Columns(vec!["repo".to_string()]))
            .execute()
            .await
            .context("Failed to query repos")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect repos")?;

        let mut repos = std::collections::HashSet::new();
        for batch in &batches {
            let repo_col = batch
                .column_by_name("repo")
                .context("Missing repo column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("repo column has wrong type")?;

            for i in 0..batch.num_rows() {
                repos.insert(repo_col.value(i).to_string());
            }
        }

        let mut repo_list: Vec<String> = repos.into_iter().collect();
        repo_list.sort();
        Ok(repo_list)
    }

    /// Get file metadata from a chunk record
    pub async fn get_file(&self, file_path: &str) -> Result<Option<FileMetadata>> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(None),
        };

        let filter = format!("file_path = '{}'", file_path.replace('\'', "''"));
        let results = table
            .query()
            .only_if(filter)
            .select(lancedb::query::Select::Columns(vec![
                "file_path".to_string(),
                "language".to_string(),
                "file_hash".to_string(),
                "indexed_at".to_string(),
            ]))
            .limit(1)
            .execute()
            .await
            .context("Failed to query file metadata")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect file metadata")?;

        if batches.is_empty() || batches[0].num_rows() == 0 {
            return Ok(None);
        }

        let batch = &batches[0];
        let file_paths = string_column(batch, "file_path")?;
        let languages = string_column(batch, "language")?;
        let file_hashes = string_column(batch, "file_hash")?;
        let indexed_ats = string_column(batch, "indexed_at")?;

        let indexed_at_str = indexed_ats.value(0);
        let indexed_at = indexed_at_str.parse::<i64>().unwrap_or(0);

        Ok(Some(FileMetadata {
            path: file_paths.value(0).to_string(),
            language: Some(languages.value(0).to_string()),
            mtime: 0, // Not stored in LanceDB (derived from filesystem)
            hash: file_hashes.value(0).to_string(),
            indexed_at,
        }))
    }

    /// Get all chunks for a specific file path, ordered by start_line
    pub async fn get_chunks_for_file(
        &self,
        file_path: &str,
        repo: Option<&str>,
    ) -> Result<Vec<Chunk>> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        let mut filter = format!("file_path = '{}'", file_path.replace('\'', "''"));
        if let Some(repo_name) = repo {
            filter.push_str(&format!(" AND repo = '{}'", repo_name.replace('\'', "''")));
        }

        let results = table
            .query()
            .only_if(filter)
            .execute()
            .await
            .context("Failed to query chunks for file")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect chunks for file")?;

        let mut chunks = Vec::new();
        for batch in &batches {
            let ids = batch.column_by_name("id").context("Missing id column")?
                .as_any().downcast_ref::<StringArray>().context("id column has wrong type")?;
            let file_paths = batch.column_by_name("file_path").context("Missing file_path column")?
                .as_any().downcast_ref::<StringArray>().context("file_path column has wrong type")?;
            let chunk_names = batch.column_by_name("chunk_name").context("Missing chunk_name column")?
                .as_any().downcast_ref::<StringArray>().context("chunk_name column has wrong type")?;
            let chunk_types = batch.column_by_name("chunk_type").context("Missing chunk_type column")?
                .as_any().downcast_ref::<StringArray>().context("chunk_type column has wrong type")?;
            let start_lines = batch.column_by_name("start_line").context("Missing start_line column")?
                .as_any().downcast_ref::<UInt32Array>().context("start_line column has wrong type")?;
            let end_lines = batch.column_by_name("end_line").context("Missing end_line column")?
                .as_any().downcast_ref::<UInt32Array>().context("end_line column has wrong type")?;
            let contents = batch.column_by_name("content").context("Missing content column")?
                .as_any().downcast_ref::<StringArray>().context("content column has wrong type")?;
            let languages = batch.column_by_name("language").context("Missing language column")?
                .as_any().downcast_ref::<StringArray>().context("language column has wrong type")?;

            for i in 0..batch.num_rows() {
                chunks.push(Chunk {
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
                });
            }
        }

        chunks.sort_by_key(|c| c.start_line);
        Ok(chunks)
    }

    /// Get all chunks whose chunk_name matches the given name, optionally filtered by repo
    pub async fn get_chunks_by_name(
        &self,
        name: &str,
        repo: Option<&str>,
    ) -> Result<Vec<Chunk>> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        let mut filter = format!("chunk_name = '{}'", name.replace('\'', "''"));
        if let Some(repo_name) = repo {
            filter.push_str(&format!(" AND repo = '{}'", repo_name.replace('\'', "''")));
        }

        let results = table
            .query()
            .only_if(filter)
            .execute()
            .await
            .context("Failed to query chunks by name")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect chunks by name")?;

        let mut chunks = Vec::new();
        for batch in &batches {
            let ids = batch.column_by_name("id").context("Missing id column")?
                .as_any().downcast_ref::<StringArray>().context("id column has wrong type")?;
            let file_paths = batch.column_by_name("file_path").context("Missing file_path column")?
                .as_any().downcast_ref::<StringArray>().context("file_path column has wrong type")?;
            let chunk_names = batch.column_by_name("chunk_name").context("Missing chunk_name column")?
                .as_any().downcast_ref::<StringArray>().context("chunk_name column has wrong type")?;
            let chunk_types = batch.column_by_name("chunk_type").context("Missing chunk_type column")?
                .as_any().downcast_ref::<StringArray>().context("chunk_type column has wrong type")?;
            let start_lines = batch.column_by_name("start_line").context("Missing start_line column")?
                .as_any().downcast_ref::<UInt32Array>().context("start_line column has wrong type")?;
            let end_lines = batch.column_by_name("end_line").context("Missing end_line column")?
                .as_any().downcast_ref::<UInt32Array>().context("end_line column has wrong type")?;
            let contents = batch.column_by_name("content").context("Missing content column")?
                .as_any().downcast_ref::<StringArray>().context("content column has wrong type")?;
            let languages = batch.column_by_name("language").context("Missing language column")?
                .as_any().downcast_ref::<StringArray>().context("language column has wrong type")?;

            for i in 0..batch.num_rows() {
                chunks.push(Chunk {
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
                });
            }
        }

        Ok(chunks)
    }

    /// Get index statistics, optionally filtered by repo
    pub async fn get_stats(&self, repo: Option<&str>) -> Result<IndexStats> {
        let table = match &self.table {
            Some(t) => t,
            None => {
                return Ok(IndexStats {
                    total_files: 0,
                    total_chunks: 0,
                    total_embeddings: 0,
                    languages: vec![],
                    last_indexed: None,
                    index_size_bytes: 0,
                });
            }
        };

        let repo_filter = repo.map(|r| format!("repo = '{}'", r.replace('\'', "''")));

        let total_chunks = table.count_rows(repo_filter.clone()).await.unwrap_or(0) as u64;

        // Query for stats by selecting relevant columns
        let mut q = table
            .query()
            .select(lancedb::query::Select::Columns(vec![
                "file_path".to_string(),
                "language".to_string(),
                "indexed_at".to_string(),
            ]));

        if let Some(ref filter) = repo_filter {
            q = q.only_if(filter.clone());
        }

        let results = q
            .execute()
            .await
            .context("Failed to query stats")?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .context("Failed to collect stats")?;

        let mut file_set = std::collections::HashSet::new();
        let mut lang_files: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        let mut lang_chunks: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        let mut last_indexed: Option<i64> = None;

        for batch in &batches {
            let file_paths = string_column(batch, "file_path")?;
            let languages = string_column(batch, "language")?;
            let indexed_ats = string_column(batch, "indexed_at")?;

            for i in 0..batch.num_rows() {
                let fp = file_paths.value(i).to_string();
                let lang = languages.value(i).to_string();
                let ts = indexed_ats.value(i).parse::<i64>().unwrap_or(0);

                file_set.insert(fp.clone());

                lang_files
                    .entry(lang.clone())
                    .or_default()
                    .insert(fp);
                *lang_chunks.entry(lang).or_insert(0) += 1;

                if last_indexed.is_none() || Some(ts) > last_indexed {
                    last_indexed = Some(ts);
                }
            }
        }

        let languages: Vec<LanguageStats> = lang_files
            .iter()
            .map(|(lang, files)| LanguageStats {
                language: lang.clone(),
                file_count: files.len() as u64,
                chunk_count: *lang_chunks.get(lang).unwrap_or(&0),
            })
            .collect();

        Ok(IndexStats {
            total_files: file_set.len() as u64,
            total_chunks,
            total_embeddings: total_chunks,
            languages,
            last_indexed,
            index_size_bytes: 0, // LanceDB doesn't expose this easily
        })
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
        ChunkType::Section => "section",
        ChunkType::Table => "table",
        ChunkType::CodeBlock => "code_block",
        ChunkType::Commit => "commit",
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
        "section" => ChunkType::Section,
        "table" => ChunkType::Table,
        "code_block" => ChunkType::CodeBlock,
        "commit" => ChunkType::Commit,
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
        let mut emb: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        emb.iter_mut().for_each(|x| *x /= norm);
        emb
    }

    fn no_contexts(n: usize) -> Vec<Option<String>> {
        vec![None; n]
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

        store
            .insert(&chunks, &embeddings, &no_contexts(2), "default", "abc123", "1234567890")
            .await
            .unwrap();

        assert_eq!(store.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_search_returns_results() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![sample_chunk("chunk1", "process_data")];
        let embeddings = vec![sample_embedding()];

        store
            .insert(&chunks, &embeddings, &no_contexts(1), "default", "abc123", "1234567890")
            .await
            .unwrap();

        let results = store.search(&sample_embedding(), 10, None).await.unwrap();

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

        store
            .insert(&chunks, &embeddings, &no_contexts(2), "default", "abc123", "1234567890")
            .await
            .unwrap();
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

        store
            .insert(&chunks, &embeddings, &no_contexts(2), "default", "abc123", "1234567890")
            .await
            .unwrap();
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

        let chunks = vec![sample_chunk("chunk1", "original")];
        let embeddings = vec![sample_embedding()];
        store
            .insert(&chunks, &embeddings, &no_contexts(1), "default", "abc123", "1234567890")
            .await
            .unwrap();

        let chunks = vec![sample_chunk("chunk1", "updated")];
        store
            .insert(&chunks, &embeddings, &no_contexts(1), "default", "def456", "1234567891")
            .await
            .unwrap();

        assert_eq!(store.count().await.unwrap(), 1);

        let results = store.search(&sample_embedding(), 10, None).await.unwrap();
        assert_eq!(results[0].chunk.name, Some("updated".to_string()));
    }

    #[tokio::test]
    async fn test_empty_operations() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        store
            .insert(&[], &[], &[], "default", "", "")
            .await
            .unwrap();

        let results = store.search(&sample_embedding(), 10, None).await.unwrap();
        assert!(results.is_empty());

        store.delete(&["nonexistent".to_string()]).await.unwrap();
    }

    #[tokio::test]
    async fn test_reopen_persists_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        {
            let mut store = VectorStore::open(&path).await.unwrap();
            let chunks = vec![sample_chunk("chunk1", "persistent")];
            let embeddings = vec![sample_embedding()];
            store
                .insert(&chunks, &embeddings, &no_contexts(1), "default", "abc123", "1234567890")
                .await
                .unwrap();
        }

        {
            let store = VectorStore::open(&path).await.unwrap();
            assert_eq!(store.count().await.unwrap(), 1);

            let results = store.search(&sample_embedding(), 10, None).await.unwrap();
            assert_eq!(results[0].chunk.name, Some("persistent".to_string()));
        }
    }

    #[tokio::test]
    async fn test_needs_reindex() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        // No data yet - needs reindex
        assert!(store.needs_reindex("src/main.rs", "abc123").await.unwrap());

        let chunks = vec![sample_chunk("chunk1", "main")];
        let embeddings = vec![sample_embedding()];
        store
            .insert(&chunks, &embeddings, &no_contexts(1), "default", "abc123", "1234567890")
            .await
            .unwrap();

        // Same hash - no reindex needed
        assert!(!store.needs_reindex("src/main.rs", "abc123").await.unwrap());

        // Different hash - needs reindex
        assert!(store.needs_reindex("src/main.rs", "different").await.unwrap());

        // Different file - needs reindex
        assert!(store.needs_reindex("src/other.rs", "abc123").await.unwrap());
    }

    #[tokio::test]
    async fn test_get_stats() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![
            Chunk {
                id: "chunk1".to_string(),
                file_path: "src/main.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("main".to_string()),
                start_line: 1,
                end_line: 10,
                content: "fn main() {}".to_string(),
                language: "rust".to_string(),
            },
            Chunk {
                id: "chunk2".to_string(),
                file_path: "src/main.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("helper".to_string()),
                start_line: 12,
                end_line: 20,
                content: "fn helper() {}".to_string(),
                language: "rust".to_string(),
            },
            Chunk {
                id: "chunk3".to_string(),
                file_path: "src/script.py".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("run".to_string()),
                start_line: 1,
                end_line: 5,
                content: "def run(): pass".to_string(),
                language: "python".to_string(),
            },
        ];
        let embeddings = vec![sample_embedding(), sample_embedding(), sample_embedding()];

        store
            .insert(&chunks, &embeddings, &no_contexts(3), "default", "abc123", "1234567890")
            .await
            .unwrap();

        let stats = store.get_stats(None).await.unwrap();
        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.total_chunks, 3);

        let rust_stats = stats.languages.iter().find(|l| l.language == "rust").unwrap();
        assert_eq!(rust_stats.file_count, 1);
        assert_eq!(rust_stats.chunk_count, 2);

        let python_stats = stats.languages.iter().find(|l| l.language == "python").unwrap();
        assert_eq!(python_stats.file_count, 1);
        assert_eq!(python_stats.chunk_count, 1);
    }

    #[tokio::test]
    async fn test_get_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![sample_chunk("chunk1", "main")];
        let embeddings = vec![sample_embedding()];
        store
            .insert(&chunks, &embeddings, &no_contexts(1), "default", "abc123", "1234567890")
            .await
            .unwrap();

        let file = store.get_file("src/main.rs").await.unwrap();
        assert!(file.is_some());
        let file = file.unwrap();
        assert_eq!(file.path, "src/main.rs");
        assert_eq!(file.hash, "abc123");

        let no_file = store.get_file("nonexistent.rs").await.unwrap();
        assert!(no_file.is_none());
    }

    #[tokio::test]
    async fn test_multi_repo_search() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        // Insert chunks into two different repos
        let chunk_a = Chunk {
            id: "repo_a_chunk1".to_string(),
            file_path: "src/main.rs".to_string(),
            chunk_type: ChunkType::Function,
            name: Some("main_a".to_string()),
            start_line: 1,
            end_line: 10,
            content: "fn main_a() {}".to_string(),
            language: "rust".to_string(),
        };
        let chunk_b = Chunk {
            id: "repo_b_chunk1".to_string(),
            file_path: "src/main.rs".to_string(),
            chunk_type: ChunkType::Function,
            name: Some("main_b".to_string()),
            start_line: 1,
            end_line: 10,
            content: "fn main_b() {}".to_string(),
            language: "rust".to_string(),
        };

        store
            .insert(&[chunk_a], &[sample_embedding()], &no_contexts(1), "repo_a", "hash_a", "100")
            .await
            .unwrap();
        store
            .insert(&[chunk_b], &[sample_embedding()], &no_contexts(1), "repo_b", "hash_b", "200")
            .await
            .unwrap();

        // Search all repos
        let all_results = store.search(&sample_embedding(), 10, None).await.unwrap();
        assert_eq!(all_results.len(), 2);

        // Search specific repo
        let repo_a_results = store.search(&sample_embedding(), 10, Some("repo_a")).await.unwrap();
        assert_eq!(repo_a_results.len(), 1);
        assert_eq!(repo_a_results[0].chunk.name, Some("main_a".to_string()));

        let repo_b_results = store.search(&sample_embedding(), 10, Some("repo_b")).await.unwrap();
        assert_eq!(repo_b_results.len(), 1);
        assert_eq!(repo_b_results[0].chunk.name, Some("main_b".to_string()));

        // Get all repos
        let repos = store.get_all_repos().await.unwrap();
        assert_eq!(repos.len(), 2);
        assert!(repos.contains(&"repo_a".to_string()));
        assert!(repos.contains(&"repo_b".to_string()));

        // Get stats filtered by repo
        let stats_a = store.get_stats(Some("repo_a")).await.unwrap();
        assert_eq!(stats_a.total_chunks, 1);

        let stats_all = store.get_stats(None).await.unwrap();
        assert_eq!(stats_all.total_chunks, 2);

        // Get file paths filtered by repo
        let paths_a = store.get_all_file_paths(Some("repo_a")).await.unwrap();
        assert_eq!(paths_a.len(), 1);
    }

    #[tokio::test]
    async fn test_get_chunks_for_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![
            Chunk {
                id: "chunk1".to_string(),
                file_path: "src/a.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("func_b".to_string()),
                start_line: 20,
                end_line: 30,
                content: "fn func_b() {}".to_string(),
                language: "rust".to_string(),
            },
            Chunk {
                id: "chunk2".to_string(),
                file_path: "src/a.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("func_a".to_string()),
                start_line: 1,
                end_line: 10,
                content: "fn func_a() {}".to_string(),
                language: "rust".to_string(),
            },
            Chunk {
                id: "chunk3".to_string(),
                file_path: "src/b.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("other".to_string()),
                start_line: 1,
                end_line: 5,
                content: "fn other() {}".to_string(),
                language: "rust".to_string(),
            },
        ];
        let embeddings = vec![sample_embedding(), sample_embedding(), sample_embedding()];

        store
            .insert(&chunks, &embeddings, &no_contexts(3), "default", "abc123", "1234567890")
            .await
            .unwrap();

        // Get chunks for src/a.rs - should return 2 chunks sorted by start_line
        let result = store.get_chunks_for_file("src/a.rs", None).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].start_line, 1); // func_a first
        assert_eq!(result[1].start_line, 20); // func_b second

        // Get chunks for src/b.rs - should return 1 chunk
        let result = store.get_chunks_for_file("src/b.rs", None).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, Some("other".to_string()));

        // Get chunks for unknown file - should return empty
        let result = store.get_chunks_for_file("unknown.rs", None).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_insert_with_full_context() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![
            sample_chunk("chunk1", "main"),
            sample_chunk("chunk2", "helper"),
        ];
        let embeddings = vec![sample_embedding(), sample_embedding()];
        let contexts = vec![
            Some("// context before\nfn main() { }\n// context after".to_string()),
            None, // No context for this chunk
        ];

        store
            .insert(&chunks, &embeddings, &contexts, "default", "abc123", "1234567890")
            .await
            .unwrap();

        assert_eq!(store.count().await.unwrap(), 2);

        // Verify chunks are searchable
        let results = store.search(&sample_embedding(), 10, None).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_custom_embedding_dimension() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        // Open with custom 768-dim instead of default 384
        let mut store = VectorStore::open_with_dim(&path, 768).await.unwrap();
        assert_eq!(store.embedding_dim(), 768);

        let chunks = vec![sample_chunk("chunk1", "main")];
        // Create a 768-dim embedding
        let mut emb: Vec<f32> = (0..768).map(|i| (i as f32) / 768.0).collect();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        emb.iter_mut().for_each(|x| *x /= norm);

        store
            .insert(&chunks, &[emb.clone()], &no_contexts(1), "default", "abc123", "1234567890")
            .await
            .unwrap();

        assert_eq!(store.count().await.unwrap(), 1);

        let results = store.search(&emb, 10, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.name, Some("main".to_string()));
    }

    #[tokio::test]
    async fn test_get_chunk_embedding() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");

        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![sample_chunk("chunk1", "main")];
        let embedding = sample_embedding();

        store
            .insert(&chunks, &[embedding.clone()], &no_contexts(1), "default", "abc123", "1234567890")
            .await
            .unwrap();

        // Retrieve the stored embedding
        let retrieved = store.get_chunk_embedding("chunk1").await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.len(), 384);

        // Verify values match what we inserted
        for (a, b) in embedding.iter().zip(retrieved.iter()) {
            assert!((a - b).abs() < 1e-6, "Embedding values should match");
        }

        // Non-existent chunk returns None
        let missing = store.get_chunk_embedding("nonexistent").await.unwrap();
        assert!(missing.is_none());
    }
}

use anyhow::Result;

use crate::index::git::{DiffFile, DiffStatus};
use crate::storage::VectorStore;
use crate::types::Chunk;

use super::context::{SeedChunk, SeedSource};

/// Map diff files to seed chunks by finding indexed chunks that overlap changed lines.
///
/// For each `DiffFile`, queries the vector store for all chunks in that file,
/// then scores each chunk by how much it overlaps with the changed line ranges.
/// Chunks with no overlap are excluded.
pub async fn map_diff_to_chunks(
    diff_files: &[DiffFile],
    vector_store: &VectorStore,
    repo: Option<&str>,
) -> Result<Vec<SeedChunk>> {
    let mut seeds: Vec<SeedChunk> = Vec::new();

    for diff_file in diff_files {
        // Skip deleted files — they have no chunks in the current index
        if diff_file.status == DiffStatus::Deleted {
            continue;
        }

        let chunks = vector_store
            .get_chunks_for_file(&diff_file.path, repo)
            .await?;

        for chunk in chunks {
            let score = overlap_score(&chunk, diff_file);
            if score > 0.0 {
                seeds.push(SeedChunk {
                    chunk,
                    score,
                    source: SeedSource::Diff {
                        status: diff_file.status.to_string(),
                        added_lines: diff_file.added_lines.len(),
                        removed_lines: diff_file.removed_lines.len(),
                    },
                });
            }
        }
    }

    // Sort by score descending for consistent ordering
    seeds.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    Ok(seeds)
}

/// Calculate how much a chunk overlaps with changed lines in a diff.
///
/// Returns a score between 0.0 and 1.0 representing the fraction of
/// the chunk's line range that was touched by the diff. A score of 0.0
/// means no overlap; 1.0 means every line in the chunk was changed.
fn overlap_score(chunk: &Chunk, diff_file: &DiffFile) -> f32 {
    let chunk_start = chunk.start_line;
    let chunk_end = chunk.end_line;
    let chunk_lines = (chunk_end - chunk_start + 1) as f32;

    if chunk_lines <= 0.0 {
        return 0.0;
    }

    // Count how many added lines fall within the chunk's range
    let overlapping = diff_file
        .added_lines
        .iter()
        .filter(|&&line| line >= chunk_start && line <= chunk_end)
        .count() as f32;

    overlapping / chunk_lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChunkType;

    fn make_chunk(id: &str, file: &str, start: u32, end: u32) -> Chunk {
        Chunk {
            id: id.to_string(),
            file_path: file.to_string(),
            chunk_type: ChunkType::Function,
            name: Some(format!("fn_{}", id)),
            start_line: start,
            end_line: end,
            content: "fn test() {}".to_string(),
            language: "rust".to_string(),
        }
    }

    fn make_diff(path: &str, added: Vec<u32>, removed: Vec<u32>, status: DiffStatus) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            added_lines: added,
            removed_lines: removed,
            status,
        }
    }

    #[test]
    fn test_overlap_score_full_overlap() {
        let chunk = make_chunk("c1", "a.rs", 10, 14); // lines 10-14 (5 lines)
        let diff = make_diff("a.rs", vec![10, 11, 12, 13, 14], vec![], DiffStatus::Modified);

        let score = overlap_score(&chunk, &diff);
        assert!((score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_overlap_score_partial_overlap() {
        let chunk = make_chunk("c1", "a.rs", 10, 19); // lines 10-19 (10 lines)
        let diff = make_diff("a.rs", vec![15, 16, 17], vec![], DiffStatus::Modified);

        let score = overlap_score(&chunk, &diff);
        assert!((score - 0.3).abs() < 0.001); // 3/10
    }

    #[test]
    fn test_overlap_score_no_overlap() {
        let chunk = make_chunk("c1", "a.rs", 10, 19); // lines 10-19
        let diff = make_diff("a.rs", vec![1, 2, 3, 25, 26], vec![], DiffStatus::Modified);

        let score = overlap_score(&chunk, &diff);
        assert!((score - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_overlap_score_only_added_lines_count() {
        // Removed lines don't count toward overlap (they're in the old version)
        let chunk = make_chunk("c1", "a.rs", 10, 14); // 5 lines
        let diff = make_diff(
            "a.rs",
            vec![12],         // 1 added line in range
            vec![10, 11, 12], // removed lines ignored
            DiffStatus::Modified,
        );

        let score = overlap_score(&chunk, &diff);
        assert!((score - 0.2).abs() < 0.001); // 1/5
    }

    #[test]
    fn test_overlap_score_single_line_chunk() {
        let chunk = make_chunk("c1", "a.rs", 5, 5); // 1 line
        let diff = make_diff("a.rs", vec![5], vec![], DiffStatus::Modified);

        let score = overlap_score(&chunk, &diff);
        assert!((score - 1.0).abs() < 0.001);
    }
}

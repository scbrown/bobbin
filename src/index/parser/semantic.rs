//! Semantic tree traversal and unique syntax-span identities.

use super::*;

impl Parser {
    /// Extract semantic chunks from a syntax tree
    pub(super) fn extract_chunks(
        &self,
        node: &Node,
        content: &str,
        path: &Path,
        language: &str,
        chunks: &mut Vec<Chunk>,
        claimed_ids: &mut std::collections::HashSet<String>,
    ) {
        let chunk_type = self.node_to_chunk_type(node, language);

        if let Some(chunk_type) = chunk_type {
            let name = self.extract_name(node, content, language);
            let start_line = node.start_position().row as u32 + 1;
            let end_line = node.end_position().row as u32 + 1;
            let node_content = &content[node.byte_range()];

            // Preserve the historical ID unless another syntax node already
            // owns this line range. Distinct same-line callbacks otherwise
            // alias before structural edges are even constructed. Byte ranges
            // also distinguish identical callback text at different columns.
            let base = generate_chunk_id(path, start_line, end_line);
            let mut id = base.clone();
            let mut occurrence = 0;
            while !claimed_ids.insert(id.clone()) {
                occurrence += 1;
                id = format!(
                    "{base}-B{}-{}-{occurrence}",
                    node.start_byte(),
                    node.end_byte()
                );
            }

            chunks.push(Chunk {
                id,
                file_path: path.to_string_lossy().to_string(),
                chunk_type,
                name,
                start_line,
                end_line,
                content: node_content.to_string(),
                language: language.to_string(),
                tags: String::new(),
            });
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_chunks(&child, content, path, language, chunks, claimed_ids);
        }
    }
}

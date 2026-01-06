pub mod embedder;
pub mod git;
pub mod parser;

pub use embedder::{ensure_model, Embedder, SharedEmbedder};
pub use git::{FileHistoryEntry, GitAnalyzer};
pub use parser::Parser;

pub mod context;
pub mod hybrid;
pub mod keyword;
pub mod preprocess;
pub mod review;
pub mod semantic;

// Context assembler types used by cli/context.rs and mcp/server.rs directly
pub use hybrid::HybridSearch;
pub use preprocess::preprocess_for_keywords;
pub use semantic::SemanticSearch;

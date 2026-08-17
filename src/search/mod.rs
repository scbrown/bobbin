pub mod context;
pub mod hybrid;
pub mod intent;
pub mod keyword;
pub mod ppr;
pub mod preprocess;
pub mod query;
pub mod review;
pub mod semantic;

// Context assembler types used by cli/context.rs and mcp/server.rs directly
pub use hybrid::HybridSearch;
pub use semantic::SemanticSearch;

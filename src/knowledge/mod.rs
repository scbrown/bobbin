pub mod chunks;
pub mod coupling;
pub mod embedding;
pub mod inferred;
pub mod mentions;
pub mod quarantine;

#[cfg(test)]
#[path = "inferred_tests.rs"]
mod inferred_tests;

//! Consumer-side data model for Quipu share bundle contract v1.
//!
//! This module deliberately models the published wire contract without
//! registering an MCP tool or performing I/O. The runtime share/import/merge
//! surfaces remain gated on their upstream Quipu phases. Unknown fields are
//! retained so a v1 bundle with additive extensions can be read and rewritten
//! without Bobbin silently deleting data it does not yet understand.

// This is a contract-first leaf module. Its public types become runtime-used
// only when the upstream share/import phases land.
#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const SHARE_MANIFEST_V1: &str = "https://github.com/scbrown/quipu/share-manifest/v1";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContractError {
    #[error("unsupported Quipu share manifest schema: {0}")]
    UnsupportedSchema(String),
    #[error("invalid Quipu share bundle path for {field}: expected {expected}, got {actual}")]
    InvalidPath {
        field: &'static str,
        expected: &'static str,
        actual: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShareManifestV1 {
    pub schema: String,
    pub share_id: String,
    pub store_id: String,
    pub tx_anchor: u64,
    pub graph_hash: String,
    pub shapes_hash: String,
    pub scope: ShareScopeV1,
    pub parent_share: Option<String>,
    pub created_at: String,
    pub producer: ShareProducerV1,
    pub files: ShareFilesV1,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

impl ShareManifestV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != SHARE_MANIFEST_V1 {
            return Err(ContractError::UnsupportedSchema(self.schema.clone()));
        }
        require_path("files.graph", "export.nt", &self.files.graph)?;
        require_path("files.shapes", "shapes.ttl", &self.files.shapes)?;
        if let Some(path) = &self.files.turtle_view {
            require_path("files.turtle_view", "export.ttl", path)?;
        }
        Ok(())
    }
}

fn require_path(
    field: &'static str,
    expected: &'static str,
    actual: &str,
) -> Result<(), ContractError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ContractError::InvalidPath {
            field,
            expected,
            actual: actual.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShareScopeV1 {
    pub kind: ShareScopeKindV1,
    pub value: Option<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShareScopeKindV1 {
    Root,
    Graph,
    Group,
    Construct,
    ShapeLens,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShareProducerV1 {
    pub name: String,
    pub version: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShareFilesV1 {
    pub graph: String,
    pub shapes: String,
    pub turtle_view: Option<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportRequestV1 {
    pub manifest: ShareManifestV1,
    pub export_ntriples: String,
    pub shapes_turtle: String,
    pub source: String,
    pub actor: Option<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportResponseV1 {
    pub outcome: ImportOutcomeV1,
    pub import_id: String,
    pub share_id: String,
    pub graph_hash: String,
    pub staging_graph: String,
    pub triples: ImportTripleCountsV1,
    pub resolution: ImportResolutionV1,
    pub validation: ImportValidationV1,
    pub promotion: ImportPromotionV1,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportOutcomeV1 {
    Staged,
    Unchanged,
    Quarantined,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportTripleCountsV1 {
    pub accepted: u64,
    pub quarantined: u64,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportResolutionV1 {
    pub exact_merges: Vec<ImportExactMergeV1>,
    pub candidates: Vec<ImportCandidateV1>,
    pub unmatched: Vec<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportExactMergeV1 {
    pub foreign: String,
    pub local: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportCandidateV1 {
    pub foreign: String,
    pub local: String,
    pub score: f64,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportValidationV1 {
    pub conforms: bool,
    pub report: Value,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportPromotionV1 {
    pub eligible: bool,
    pub blockers: Vec<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use sha2::{Digest, Sha256};

    use super::*;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quipu-share-v1")
    }

    #[test]
    fn published_v1_fixture_is_self_consistent() {
        let dir = fixture_dir();
        let manifest: ShareManifestV1 =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        manifest.validate().unwrap();

        let graph = fs::read(dir.join(&manifest.files.graph)).unwrap();
        let shapes = fs::read(dir.join(&manifest.files.shapes)).unwrap();
        assert_eq!(manifest.graph_hash, sha256_id(&graph));
        assert_eq!(manifest.shapes_hash, sha256_id(&shapes));
        assert_eq!(manifest.share_id, fixture_share_id(&manifest));
        assert!(
            graph.ends_with(b"\n"),
            "canonical export.nt must be LF terminated"
        );

        let lines: Vec<&[u8]> = graph
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .collect();
        assert!(lines.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(lines.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn import_request_and_response_match_published_v1_contract() {
        let dir = fixture_dir();
        let request: ImportRequestV1 =
            serde_json::from_slice(&fs::read(dir.join("import-request.json")).unwrap()).unwrap();
        request.manifest.validate().unwrap();
        assert_eq!(
            request.export_ntriples.as_bytes(),
            fs::read(dir.join("export.nt")).unwrap()
        );
        assert_eq!(
            request.shapes_turtle.as_bytes(),
            fs::read(dir.join("shapes.ttl")).unwrap()
        );

        let response: ImportResponseV1 =
            serde_json::from_slice(&fs::read(dir.join("import-response.json")).unwrap()).unwrap();
        assert_eq!(response.outcome, ImportOutcomeV1::Quarantined);
        assert!(!response.resolution.candidates.is_empty());
        assert!(!response.promotion.eligible);
        assert!(!response.promotion.blockers.is_empty());
    }

    #[test]
    fn additive_v1_fields_survive_a_typed_round_trip() {
        let mut raw: Value =
            serde_json::from_slice(&fs::read(fixture_dir().join("manifest.json")).unwrap())
                .unwrap();
        raw["future_top_level"] = serde_json::json!({"kept": true});
        raw["scope"]["future_scope_field"] = serde_json::json!([1, 2, 3]);
        raw["producer"]["future_producer_field"] = serde_json::json!("kept");
        raw["files"]["future_files_field"] = serde_json::json!(42);

        let typed: ShareManifestV1 = serde_json::from_value(raw.clone()).unwrap();
        typed.validate().unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), raw);
    }

    #[test]
    fn unsupported_major_and_noncanonical_paths_are_refused() {
        let mut manifest: ShareManifestV1 =
            serde_json::from_slice(&fs::read(fixture_dir().join("manifest.json")).unwrap())
                .unwrap();
        manifest.schema = "https://github.com/scbrown/quipu/share-manifest/v2".into();
        assert!(matches!(
            manifest.validate(),
            Err(ContractError::UnsupportedSchema(_))
        ));

        manifest.schema = SHARE_MANIFEST_V1.into();
        manifest.files.graph = "../export.nt".into();
        assert!(matches!(
            manifest.validate(),
            Err(ContractError::InvalidPath {
                field: "files.graph",
                ..
            })
        ));
    }

    fn sha256_id(bytes: &[u8]) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
    }

    /// Canonicalize the deliberately small JSON subset used by this fixture.
    /// Its strings, integer, arrays, objects, booleans and null map directly
    /// to RFC 8785/JCS; production canonicalization remains Quipu's job.
    fn fixture_share_id(manifest: &ShareManifestV1) -> String {
        let mut value = serde_json::to_value(manifest).unwrap();
        value.as_object_mut().unwrap().remove("share_id");
        let mut canonical = String::new();
        write_fixture_jcs(&value, &mut canonical);
        sha256_id(canonical.as_bytes())
    }

    fn write_fixture_jcs(value: &Value, out: &mut String) {
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
            Value::Number(v) => out.push_str(&v.to_string()),
            Value::String(v) => out.push_str(&serde_json::to_string(v).unwrap()),
            Value::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_fixture_jcs(value, out);
                }
                out.push(']');
            }
            Value::Object(values) => {
                out.push('{');
                let mut keys: Vec<&String> = values.keys().collect();
                keys.sort_unstable();
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key).unwrap());
                    out.push(':');
                    write_fixture_jcs(&values[key], out);
                }
                out.push('}');
            }
        }
    }
}

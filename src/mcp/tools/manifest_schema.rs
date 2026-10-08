use serde::Deserialize;

/// Request to stage a canonical v1 Quipu share. This never promotes it.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct KnowledgeImportRequest {
    #[schemars(schema_with = "unrestricted_json_schema")]
    pub manifest: serde_json::Value,
    pub export_ntriples: String,
    pub shapes_turtle: Option<String>,
    pub source: String,
    pub actor: Option<String>,
}

/// Object-form `true`: accepts exactly the same JSON values, but MCP clients
/// which require object-valued property schemas can still list every tool.
fn unrestricted_json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = generator.subschema_for::<serde_json::Value>();
    schema.ensure_object();
    schema
}

#[cfg(test)]
mod knowledge_import_schema_tests {
    use super::KnowledgeImportRequest;

    #[test]
    fn unrestricted_manifest_has_object_schema_without_narrowing_values() {
        let schema = schemars::schema_for!(KnowledgeImportRequest);
        assert_eq!(
            schema.as_value()["properties"]["manifest"],
            serde_json::json!({})
        );
        for manifest in [
            serde_json::Value::Null,
            serde_json::json!([1]),
            serde_json::json!({"a": 1}),
        ] {
            let request = serde_json::json!({"manifest": manifest, "export_ntriples": "",
                                            "source": "test"});
            assert!(serde_json::from_value::<KnowledgeImportRequest>(request).is_ok());
        }
    }
}

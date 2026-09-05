//! Bobbin's LOCAL embedded knowledge-graph MCP tools.
//!
//! Split from `mcp::knowledge_tools` (aegis-fmcth7) to keep both files under
//! the 500-line ceiling without adding a new allowlist entry — the allowlist's
//! own header says shrinking it is the point, so a new file should not be born
//! needing one.
//!
//! The seam is the one the annotations already draw: these three write to
//! bobbin's OWN embedded graph (`open_world_hint = false`), while the tools
//! left behind talk to a remote Quipu (`open_world_hint = true`). Same reason
//! codex treats the two groups differently, so it is a real boundary rather
//! than a line-count convenience.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router, ErrorData as McpError};

use super::server::BobbinMcpServer;
use super::tools::*;

#[tool_router(router = local_graph_router, vis = "pub(crate)")]
impl BobbinMcpServer {
    /// Write facts into the local knowledge graph.
    #[tool(
        description = "Write facts into bobbin's LOCAL embedded knowledge graph as RDF Turtle. \
ALWAYS READ THE 'shacl_validated' FIELD IN THE RESULT. When true, the write was checked against the configured \
SHACL shapes before being committed and a violating write would have been refused. When FALSE, validation was \
NOT COMPILED IN and the facts were stored UNCHECKED — a success does not mean they are conformant, only that they \
were accepted. Do not treat an unvalidated success as evidence of well-formedness. \
This writes only to the local graph — it never writes to the remote ontology Quipu, which is read-only from here. \
Pass 'actor' and 'source' whenever you have them: a fact whose origin is unrecorded cannot be assessed later.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn knowledge_knot(
        &self,
        Parameters(req): Parameters<KnowledgeKnotRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "knowledge")]
        {
            let mut store = self.open_quipu_store().map_err(|e| {
                McpError::internal_error(format!("Failed to open knowledge graph: {e}"), None)
            })?;

            let mut input = serde_json::json!({ "turtle": req.turtle });
            // Only set the optional keys when present. quipu's knot body is
            // free-form JSON with no unknown-field rejection, so a null here
            // would be indistinguishable from a value it chose to ignore.
            if let Some(actor) = &req.actor {
                input["actor"] = serde_json::json!(actor);
            }
            if let Some(source) = &req.source {
                input["source"] = serde_json::json!(source);
            }
            if let Some(shapes) = &req.shapes {
                input["shapes"] = serde_json::json!(shapes);
            }

            let result = quipu::tool_knot(&mut store, &input).map_err(|e| {
                McpError::internal_error(format!("Knowledge write refused: {e}"), None)
            })?;

            // A SHACL refusal arrives from current quipu as `Ok` with
            // `conforms: false` (plus violations/issues), NOT as an `Err`.
            // Surfacing it as a tool error rather than a success-with-a-field
            // is deliberate: a refused write that returns success is the
            // failure mode this whole subsystem exists to prevent.
            if result.get("conforms").and_then(|v| v.as_bool()) == Some(false) {
                return Err(McpError::internal_error(
                    format!(
                        "Knowledge write refused by SHACL validation: {}",
                        serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())
                    ),
                    None,
                ));
            }

            // The honesty field. quipu's write-time SHACL check is
            // `#[cfg(feature = "shacl")]` on ITS side, so when bobbin builds
            // quipu without that feature the validation silently does not run
            // and a write returns success either way. Reporting it means an
            // unvalidated write is legible as one instead of being
            // indistinguishable from a validated one.
            let payload = serde_json::json!({
                "written": result,
                "shacl_validated": Self::KNOWLEDGE_SHACL_ENABLED,
                "store": self.knowledge_store_info(),
            });
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string()),
            )]))
        }
        #[cfg(not(feature = "knowledge"))]
        {
            let _ = req;
            Err(McpError::internal_error(
                "Knowledge graph tools require the 'knowledge' feature. Rebuild with: cargo build --features knowledge".to_string(),
                None,
            ))
        }
    }

    /// Run the quarantined-track inferred extractor over markdown prose.
    #[tool(
        description = "Extract CANDIDATE entities and relationships from markdown prose via bobbin's inferred-track \
extractor seam (currently the deterministic backtick-coderef heuristic — NOT a language model, and honestly labeled as such). \
Everything returned is a CLAIM at quarantined standing, never an observation: the response envelope carries the \
camayoc crew:inferred plane, trust rank 0, and sourceKind=inferred — treat the facts accordingly. \
With push=true the stamped facts also land in the quarantine plane via a graph-routed /knot write, each fact carrying \
quipu:derivedBy (extractor+params) and aegis:sourceKind=inferred; the write REFUSES if the embedded quipu cannot \
enforce graph routing (facts would masquerade in ROOT) or the plane is unregistered. \
Promotion out of quarantine is camayoc's authority-gated plane-promotion flow, never this tool's.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn knowledge_inferred_extract(
        &self,
        Parameters(req): Parameters<KnowledgeInferredExtractRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "knowledge")]
        {
            use crate::knowledge::inferred::{BacktickCoderefExtractor, InferredExtractor};
            use crate::knowledge::quarantine;

            let repo = req.repo.unwrap_or_else(|| "adhoc".to_string());
            let file_path = req.file_path.unwrap_or_else(|| "adhoc.md".to_string());
            let chunk = crate::types::Chunk {
                id: format!("{file_path}:1"),
                file_path,
                chunk_type: crate::types::ChunkType::Section,
                name: None,
                start_line: 1,
                end_line: req.text.lines().count().max(1) as u32,
                content: req.text,
                language: "markdown".to_string(),
                tags: String::new(),
            };
            let extractor = BacktickCoderefExtractor::default();
            let extraction = extractor.extract(std::slice::from_ref(&chunk), &repo);
            let stamped = quarantine::QuarantinedFacts::stamp(&extractor, &extraction, &repo);

            let mut facts = serde_json::json!({
                "extractor": { "id": extractor.id(), "params": extractor.params() },
                "entities": extraction.entities,
                "relations": extraction.relations,
                "quarantine": {
                    "graph": stamped.graph_iri(),
                    "snapshot": stamped.snapshot_key(),
                    "turtle": stamped.turtle(),
                },
            });

            if req.push.unwrap_or(false) {
                let mut store = self.open_quipu_store().map_err(|e| {
                    McpError::internal_error(format!("Failed to open knowledge graph: {e}"), None)
                })?;
                let (tx_id, count) =
                    quarantine::push_inferred(&mut store, &stamped).map_err(|e| {
                        McpError::internal_error(format!("Quarantine push refused: {e:#}"), None)
                    })?;
                facts["pushed"] = serde_json::json!({ "tx_id": tx_id, "count": count });
            }

            // The envelope rule: inferred facts are NEVER served bare.
            let payload = quarantine::serve_quarantined(facts);
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string()),
            )]))
        }
        #[cfg(not(feature = "knowledge"))]
        {
            let _ = req;
            Err(McpError::internal_error(
                "Knowledge graph tools require the 'knowledge' feature. Rebuild with: cargo build --features knowledge".to_string(),
                None,
            ))
        }
    }

    /// Resolve chunk mention literals against the live entity graph.
    #[tool(
        description = "Run the idempotent chunk→entity mention reconcile pass over bobbin's LOCAL knowledge graph. \
Resolves weak `bobbin:mentions \"SymbolName\"` literals on Chunk facts into typed Ref edges against the live \
entity graph, and reports every mention honestly as resolved (exactly one match — edge written), dangling \
(no match — literal left in place for a later run), or ambiguous (multiple matches — left unresolved, never \
guessed). Safe to re-run: an unchanged store yields the identical classification with edges_written = 0. \
Run it after new entities land in the graph to pick up previously dangling mentions.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn knowledge_reconcile_mentions(
        &self,
        Parameters(req): Parameters<KnowledgeReconcileRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "knowledge")]
        {
            let mut store = self.open_quipu_store().map_err(|e| {
                McpError::internal_error(format!("Failed to open knowledge graph: {e}"), None)
            })?;

            let timestamp = chrono::Utc::now().to_rfc3339();
            let report = crate::knowledge::mentions::reconcile_mentions(&mut store, &timestamp)
                .map_err(|e| {
                    McpError::internal_error(format!("Mention reconcile failed: {e}"), None)
                })?;

            let max_details = req.max_details.unwrap_or(50);
            let total_details = report.details.len();
            let details: Vec<_> = report.details.iter().take(max_details).collect();
            let payload = serde_json::json!({
                "resolved": report.resolved,
                "dangling": report.dangling,
                "ambiguous": report.ambiguous,
                "edges_written": report.edges_written,
                "details": details,
                "details_truncated": total_details > max_details,
                "store": self.knowledge_store_info(),
            });
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string()),
            )]))
        }
        #[cfg(not(feature = "knowledge"))]
        {
            let _ = req;
            Err(McpError::internal_error(
                "Knowledge graph tools require the 'knowledge' feature. Rebuild with: cargo build --features knowledge".to_string(),
                None,
            ))
        }
    }
}

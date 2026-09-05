//! Bobbin's Quipu knowledge-graph MCP tools.
//!
//! Split out of `mcp::server` (aegis-fmcth7). Annotating all 35 tools added
//! ~194 lines to a file already allowlisted at 4170 in the file-size ratchet,
//! and that allowlist is explicit that its entries are ceilings which never
//! loosen — "Remove entries by splitting files." Raising the number by hand to
//! land a change faster is exactly the move this repo's own policy forbids, so
//! the tools moved instead.
//!
//! These nine are the natural seam: they are the only tools that talk to a
//! remote Quipu rather than bobbin's local index, and they carry every
//! approval-requiring annotation in the server bar `knowledge_knot`'s local
//! write. rmcp supports this directly — `#[tool_router(router = ..., vis =
//! ...)]` builds a second router that `BobbinMcpServer::new` adds to the first.

use std::borrow::Cow;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router, ErrorData as McpError};

use super::server::BobbinMcpServer;
use super::tools::*;

#[tool_router(router = knowledge_router, vis = "pub(crate)")]
impl BobbinMcpServer {
    // ── Quipu knowledge graph tools ───────────────────────────────

    /// Whether quipu's write-time SHACL validation is compiled into this build.
    ///
    /// It IS, since the quipu bump to 0.3.23 (rev 37bfc06a): Cargo.toml
    /// declares the quipu dependency with `features = ["shacl"]`, so
    /// `tool_knot`'s `#[cfg(feature = "shacl")]` validation is compiled in and
    /// every knowledge write is checked against the stored shapes before
    /// commit. The chrono clash that used to make this impossible
    /// (rudof_lib needed `chrono ^0.4.42` while arrow-array 53 capped at
    /// `<0.4.40` — the old bobbin-di7 gap) was dissolved by the
    /// lancedb 0.27 / arrow 57 bump that landed with it.
    ///
    /// It stays a constant rather than a `cfg!` because bobbin cannot inspect
    /// its dependency's features at compile time; this mirrors the manifest.
    /// **A gate compiled out is indistinguishable from a gate that passed
    /// unless something says so** — which is why the field is reported on
    /// every write. Flip this in the same change that changes the quipu
    /// dependency's `shacl` feature; never on its own.
    pub(super) const KNOWLEDGE_SHACL_ENABLED: bool = true;

    #[tool(
        description = "Export a scoped slice from the configured remote Quipu using Quipu's canonical RDF serializer. Returns the exact RDF text plus its SHA-256 identity, or only the digest when digest_only=true. Requires an explicit scope and never reads Bobbin's separate embedded code graph.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn knowledge_export(
        &self,
        Parameters(req): Parameters<KnowledgeExportRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "knowledge")]
        {
            let base = self.quipu_remote_url().ok_or_else(|| {
                McpError::invalid_request(
                    "knowledge_export requires quipu_endpoint or BOBBIN_QUIPU_REMOTE",
                    None,
                )
            })?;
            let scope = Self::share_scope(req.scope_kind, req.scope_value).map_err(|e| {
                McpError::invalid_params(format!("invalid export scope: {e}"), None)
            })?;
            let mut body = Self::export_scope(scope);
            body["format"] = serde_json::json!(req.format.unwrap_or_else(|| "ntriples".into()));
            let result = Self::quipu_export_post(
                &base,
                body,
                req.max_bytes,
                req.digest_only.unwrap_or(false),
            )
            .await
            .map_err(|e| McpError::internal_error(format!("Quipu export failed: {e:#}"), None))?;
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&result).unwrap(),
            )]))
        }
        #[cfg(not(feature = "knowledge"))]
        {
            let _ = req;
            Err(McpError::internal_error(
                "Knowledge graph tools require the 'knowledge' feature",
                None,
            ))
        }
    }

    #[tool(
        description = "Produce a canonical v1 share bundle through the configured remote Quipu. Returns Quipu's manifest and exact files unchanged; Bobbin does not reserialize RDF, synthesize a manifest, or calculate a competing share identity. Requires an explicit scope.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn knowledge_share(
        &self,
        Parameters(req): Parameters<KnowledgeShareRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "knowledge")]
        {
            let base = self.quipu_remote_url().ok_or_else(|| {
                McpError::invalid_request(
                    "knowledge_share requires quipu_endpoint or BOBBIN_QUIPU_REMOTE",
                    None,
                )
            })?;
            let scope = Self::share_scope(req.scope_kind, req.scope_value)
                .map_err(|e| McpError::invalid_params(format!("invalid share scope: {e}"), None))?;
            let body = serde_json::json!({
                "scope": scope, "shapes": req.shapes.unwrap_or_default(),
                "no_shapes": req.no_shapes.unwrap_or(false), "parent_share": req.parent_share,
                "turtle_view": req.turtle_view.unwrap_or(false), "max_bytes": req.max_bytes,
            });
            let result = Self::quipu_share_post(&base, "/share", body, false)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("Quipu share failed: {e:#}"), None)
                })?;
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&result).unwrap(),
            )]))
        }
        #[cfg(not(feature = "knowledge"))]
        {
            let _ = req;
            Err(McpError::internal_error(
                "Knowledge graph tools require the 'knowledge' feature",
                None,
            ))
        }
    }

    #[tool(
        description = "Verify and stage a canonical v1 share bundle in the configured remote Quipu. NEVER PROMOTES. Returns Quipu's full result unchanged, including resolution candidates, validation, quarantine blockers, staging graph, and promotion eligibility. An unchanged outcome is success.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn knowledge_import(
        &self,
        Parameters(req): Parameters<KnowledgeImportRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "knowledge")]
        {
            let base = self.quipu_remote_url().ok_or_else(|| {
                McpError::invalid_request(
                    "knowledge_import requires quipu_endpoint or BOBBIN_QUIPU_REMOTE",
                    None,
                )
            })?;
            let body = serde_json::json!({
                "manifest": req.manifest, "export_ntriples": req.export_ntriples,
                "shapes_turtle": req.shapes_turtle.unwrap_or_default(),
                "source": req.source, "actor": req.actor,
            });
            let result = Self::quipu_share_post(&base, "/import", body, true)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("Quipu import failed: {e:#}"), None)
                })?;
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&result).unwrap(),
            )]))
        }
        #[cfg(not(feature = "knowledge"))]
        {
            let _ = req;
            Err(McpError::internal_error(
                "Knowledge graph tools require the 'knowledge' feature",
                None,
            ))
        }
    }

    #[tool(
        description = "Explicitly promote an eligible, already-staged Quipu share into ROOT. This is separate from knowledge_import so quarantine and review cannot be bypassed accidentally. Returns Quipu's promotion result unchanged.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn knowledge_import_promote(
        &self,
        Parameters(req): Parameters<KnowledgeImportPromoteRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "knowledge")]
        {
            let base = self.quipu_remote_url().ok_or_else(|| {
                McpError::invalid_request(
                    "knowledge_import_promote requires quipu_endpoint or BOBBIN_QUIPU_REMOTE",
                    None,
                )
            })?;
            let body = serde_json::json!({ "share_id": req.share_id, "actor": req.actor });
            let result = Self::quipu_share_post(&base, "/import/promote", body, true)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("Quipu promotion failed: {e:#}"), None)
                })?;
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&result).unwrap(),
            )]))
        }
        #[cfg(not(feature = "knowledge"))]
        {
            let _ = req;
            Err(McpError::internal_error(
                "Knowledge graph tools require the 'knowledge' feature",
                None,
            ))
        }
    }

    /// Query the knowledge graph for entities relevant to a topic
    #[tool(
        description = "Find entities and facts relevant to a topic across BOTH knowledge graphs this deployment has. \
Returns two clearly separated sections. 'ontology': the organisation knowledge graph on a remote Quipu (semantic search, then each \
matching entity's facts fetched individually) — this is where infrastructure, ownership and operational facts live. \
'local_code_graph': bobbin's own embedded graph of code entities and file-coupling from git history (IRIs under http://aegis.gastown.local/ontology/code/). \
ALWAYS read the 'store' field: if ontology.consulted is false the ontology was NOT ASKED (no remote configured), and if it carries \
an 'error' that is a TRANSPORT FAILURE — neither is evidence a fact is absent. Best for: 'who owns X?', 'what runs on Y?', \
'which files change together with Z?'",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn knowledge_context(
        &self,
        Parameters(req): Parameters<KnowledgeContextRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "knowledge")]
        {
            let store = self.open_quipu_store().map_err(|e| {
                McpError::internal_error(format!("Failed to open knowledge graph: {e}"), None)
            })?;

            let input = serde_json::json!({
                "query": req.query,
                "max_entities": req.max_entities.unwrap_or(20),
                "expand_links": req.expand_links.unwrap_or(true),
            });

            let local = quipu::tool_context(&store, &input).map_err(|e| {
                McpError::internal_error(format!("Knowledge graph query failed: {e}"), None)
            })?;

            // The ontology leg is TWO calls on purpose (aegis-rwozs, measured):
            // /context is a LABEL/text match and returns 0 entities for a natural-language
            // question, while /search (semantic) finds them. So: semantic search for the
            // entities, then fetch each one's facts.
            //
            // Every hit is fetched INDIVIDUALLY rather than relying on owl:sameAs to pull a
            // twin's facts in: sameAs is INERT on the deployed quipu (aegis-yro9m, dearing),
            // so an entity with a sameAs twin silently yields less than it appears to. Reading
            // each returned entity directly is what makes the answer whole.
            let ontology = match self.quipu_remote_url() {
                None => serde_json::json!({
                    "consulted": false,
                    "reason": "no ontology Quipu configured (quipu_endpoint / BOBBIN_QUIPU_REMOTE unset)",
                }),
                Some(base) => {
                    let max = req.max_entities.unwrap_or(20).min(25);
                    match Self::quipu_remote_post(
                        &base,
                        "/search",
                        serde_json::json!({"query": req.query}),
                    )
                    .await
                    {
                        Err(e) => serde_json::json!({
                            "consulted": true,
                            "error": format!("{e:#}"),
                            "warning": "The ontology could NOT be reached. TRANSPORT FAILURE, \
                        not an empty result — do not read it as 'the fact is absent'.",
                        }),
                        Ok(hits) => {
                            let mut entities = Vec::new();
                            let empty = Vec::new();
                            let results = hits
                                .get("results")
                                .and_then(|r| r.as_array())
                                .unwrap_or(&empty);
                            for hit in results.iter().take(max) {
                                let Some(iri) = hit.get("entity").and_then(|v| v.as_str()) else {
                                    continue;
                                };
                                let facts = Self::quipu_remote_post(
                                    &base,
                                    "/query",
                                    serde_json::json!({"query": format!(
                                        "SELECT ?p ?o WHERE {{ <{iri}> ?p ?o }}")}),
                                )
                                .await
                                .ok();
                                entities.push(serde_json::json!({
                                    "iri": iri,
                                    "score": hit.get("score"),
                                    "facts": facts.as_ref()
                                        .and_then(|f| f.get("rows")).cloned()
                                        .unwrap_or(serde_json::Value::Null),
                                    "fact_count": facts.as_ref()
                                        .and_then(|f| f.get("count")).cloned()
                                        .unwrap_or(serde_json::Value::Null),
                                }));
                            }
                            serde_json::json!({
                                "consulted": true,
                                "count": entities.len(),
                                "entities": entities,
                            })
                        }
                    }
                }
            };

            // The ontology is the answer to the question asked; the local code graph
            // is context. Trim the context harder so it cannot bury the answer.
            let mut ontology = ontology;
            let mut local = local;
            Self::trim_payload(&mut ontology, 600, 40);
            Self::trim_payload(&mut local, 200, 8);

            let result = serde_json::json!({
                "ontology": ontology,
                "local_code_graph": local,
                "store": self.knowledge_store_info(),
            });

            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string()),
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

    /// Run a SPARQL query against the knowledge graph
    #[tool(
        description = "Execute a SPARQL SELECT against BOTH knowledge graphs and return each result separately. \
'ontology': the organisation knowledge graph on a remote Quipu (infrastructure, ownership, operational facts). \
'local_code_graph': bobbin's own embedded graph (code entities and file-coupling from git history, IRIs under http://aegis.gastown.local/ontology/code/). \
The SAME query runs against both, so an IRI that exists in only one returns rows in only that section. \
ALWAYS read the 'store' field: ontology.consulted=false means NOT ASKED (no remote configured) and an 'error' means TRANSPORT \
FAILURE — an empty section is NEVER by itself evidence the fact does not exist. Supports valid_at and tx for temporal queries. \
Example: 'SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10'",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn knowledge_query(
        &self,
        Parameters(req): Parameters<KnowledgeQueryRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "knowledge")]
        {
            let store = self.open_quipu_store().map_err(|e| {
                McpError::internal_error(format!("Failed to open knowledge graph: {e}"), None)
            })?;

            let input = serde_json::json!({
                "query": req.query,
                "valid_at": req.valid_at,
                "tx": req.tx,
            });

            let local = quipu::tool_query(&store, &input)
                .map_err(|e| McpError::internal_error(format!("SPARQL query failed: {e}"), None))?;
            let mut ontology = self.ontology_sparql_section(input.clone()).await;
            let mut local = local;
            Self::trim_payload(&mut ontology, 600, 60);
            Self::trim_payload(&mut local, 200, 20);

            let result = serde_json::json!({
                "ontology": ontology,
                "local_code_graph": local,
                "store": self.knowledge_store_info(),
            });

            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string()),
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

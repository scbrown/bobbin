//! Shared Quipu helpers for Bobbin's knowledge and local graph MCP tools.
//!
//! The child module holds the tool router so the helpers and handlers each
//! remain below the file-size limit. Only helpers used by local graph tools
//! are visible outside this module.

mod handlers;

#[cfg(feature = "knowledge")]
use anyhow::{Context, Result};

use super::server::BobbinMcpServer;
#[cfg(feature = "knowledge")]
use super::tools::KnowledgeShareScopeKind;
#[cfg(feature = "knowledge")]
use crate::config::Config;
#[cfg(feature = "knowledge")]
use crate::index::Embedder;

impl BobbinMcpServer {
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
}

#[cfg(feature = "knowledge")]
impl BobbinMcpServer {
    /// Resolve the path of the LOCAL Quipu store these tools read.
    ///
    /// This is bobbin's OWN embedded graph, not any Quipu server on the network.
    /// It is returned to callers alongside every knowledge result (aegis-rwozs):
    /// a caller that cannot tell WHICH graph replied cannot tell a miss from an
    /// absence, and this deployment has two Quipu stores with disjoint contents.
    fn quipu_store_path(&self) -> std::path::PathBuf {
        let quipu_config = quipu::QuipuConfig::load(self.repo_root());
        if quipu_config.store_path.is_relative() {
            self.repo_root().join(&quipu_config.store_path)
        } else {
            quipu_config.store_path.clone()
        }
    }

    /// URL of the REMOTE Quipu instance holding the organisation ontology.
    ///
    /// This is the EXISTING `quipu_endpoint` config key — the same one the search
    /// handler already uses for spotlight annotations — rather than a second knob
    /// meaning the same thing. A deployment that already annotates search results
    /// from its ontology gets federated knowledge tools with no config change, and
    /// there is no way to have one working while the other silently is not.
    /// `BOBBIN_QUIPU_REMOTE` overrides it for testing. Unset => local graph only.
    fn quipu_remote_url(&self) -> Option<String> {
        std::env::var("BOBBIN_QUIPU_REMOTE")
            .ok()
            .or_else(|| {
                Config::load(&Config::config_path(self.repo_root()))
                    .ok()
                    .and_then(|c| c.quipu_endpoint)
            })
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
    }

    /// POST a JSON body to a path on the remote Quipu and return the parsed reply.
    async fn quipu_remote_post(
        base: &str,
        path: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .context("building HTTP client for remote quipu")?;
        let resp = client
            .post(format!("{base}{path}"))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {path} to remote quipu"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "remote quipu {path} returned HTTP {status}: {}",
                text.chars().take(300).collect::<String>()
            );
        }
        serde_json::from_str(&text).with_context(|| format!("parsing remote quipu {path} response"))
    }

    /// POST a canonical share operation, authenticating writes and returning
    /// Quipu's JSON without reshaping it.
    async fn quipu_share_post(
        base: &str,
        path: &str,
        body: serde_json::Value,
        authenticated: bool,
    ) -> Result<serde_json::Value> {
        let timeout = if authenticated { 900 } else { 120 };
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout))
            .build()
            .context("building HTTP client for remote quipu share operation")?;
        let mut request = client
            .post(format!("{base}{path}"))
            .header("X-Quipu-Client", "agent-adhoc")
            .json(&body);
        if authenticated {
            let token = crate::knowledge::chunks::quipu_auth_token()
                .context("remote Quipu write requires QUIPU_AUTH_TOKEN or a readable token file")?;
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("POST {path} to remote quipu"))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "remote quipu {path} returned HTTP {status}: {}",
                text.chars().take(300).collect::<String>()
            );
        }
        serde_json::from_str(&text).with_context(|| format!("parsing remote quipu {path} response"))
    }

    async fn quipu_export_post(
        base: &str,
        body: serde_json::Value,
        max_bytes: Option<usize>,
        digest_only: bool,
    ) -> Result<serde_json::Value> {
        use sha2::{Digest, Sha256};

        let response = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .context("building HTTP client for remote quipu export")?
            .post(format!("{base}/export"))
            .header("X-Quipu-Client", "agent-adhoc")
            .json(&body)
            .send()
            .await
            .context("POST /export to remote quipu")?;
        let status = response.status();
        let bytes = response.bytes().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "remote quipu /export returned HTTP {status}: {}",
                String::from_utf8_lossy(&bytes)
                    .chars()
                    .take(300)
                    .collect::<String>()
            );
        }
        if let Some(limit) = max_bytes {
            if bytes.len() > limit {
                anyhow::bail!(
                    "remote quipu export is {} bytes, above caller max_bytes {limit}",
                    bytes.len()
                );
            }
        }
        let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
        let mut result = serde_json::json!({
            "bytes": bytes.len(),
            "sha256": sha256,
        });
        if !digest_only {
            result["rdf"] = serde_json::Value::String(
                String::from_utf8(bytes.to_vec()).context("Quipu export was not UTF-8 RDF")?,
            );
        }
        Ok(result)
    }

    fn share_scope(
        kind: KnowledgeShareScopeKind,
        value: Option<String>,
    ) -> Result<serde_json::Value> {
        let kind = match kind {
            KnowledgeShareScopeKind::Root => "root",
            KnowledgeShareScopeKind::Graph => "graph",
            KnowledgeShareScopeKind::Group => "group",
            KnowledgeShareScopeKind::Construct => "construct",
        };
        if kind == "root" && value.is_some() {
            anyhow::bail!("root scope does not accept scope_value");
        }
        if kind != "root" && value.as_deref().is_none_or(str::is_empty) {
            anyhow::bail!("{kind} scope requires scope_value");
        }
        Ok(serde_json::json!({ "kind": kind, "value": value }))
    }

    fn export_scope(scope: serde_json::Value) -> serde_json::Value {
        let kind = scope["kind"].as_str().unwrap_or("root");
        let value = scope["value"].clone();
        match kind {
            "graph" => serde_json::json!({ "graph": value }),
            "group" => serde_json::json!({ "group_id": value }),
            "construct" => serde_json::json!({ "construct": value }),
            _ => serde_json::json!({}),
        }
    }

    /// Trim a knowledge payload so it fits a caller's tool-output budget.
    ///
    /// aegis-rczil: the un-trimmed reply to one ownership question was 83KB /
    /// 2205 lines and EXCEEDED the caller's limit — they received an error and a
    /// file path instead of an answer, with the two lines they wanted diluted 4:1
    /// by the file-coupling graph. A correct answer nobody can receive is not an
    /// answer.
    ///
    /// Truncation is ALWAYS ANNOUNCED IN BAND — an elided array gains a
    /// "… [+N more]" element and a clipped string says how much was dropped.
    /// Silent truncation would be the same defect this whole area keeps hitting:
    /// a well-formed reply that quietly answers less than it appears to.
    fn trim_payload(v: &mut serde_json::Value, max_str: usize, max_arr: usize) {
        use serde_json::Value;
        match v {
            Value::String(s) => {
                if s.chars().count() > max_str {
                    let kept: String = s.chars().take(max_str).collect();
                    let dropped = s.chars().count() - max_str;
                    *s = format!("{kept}… [+{dropped} chars omitted]");
                }
            }
            Value::Array(a) => {
                let total = a.len();
                if total > max_arr {
                    a.truncate(max_arr);
                    a.push(Value::String(format!(
                        "… [+{} more omitted]",
                        total - max_arr
                    )));
                }
                for item in a.iter_mut() {
                    Self::trim_payload(item, max_str, max_arr);
                }
            }
            Value::Object(o) => {
                for (_k, val) in o.iter_mut() {
                    Self::trim_payload(val, max_str, max_arr);
                }
            }
            _ => {}
        }
    }

    /// Describe which store(s) a knowledge answer came from.
    ///
    /// A caller that cannot tell WHICH graph replied cannot tell a miss from an
    /// absence, and this deployment has two Quipu graphs with disjoint contents
    /// (aegis-rwozs). Every knowledge response carries this.
    pub(super) fn knowledge_store_info(&self) -> serde_json::Value {
        serde_json::json!({
            "local_code_graph": {
                "path": self.quipu_store_path().to_string_lossy(),
                "contains": "bobbin's OWN graph: code entities and file-coupling \
        derived from git history (IRIs under http://aegis.gastown.local/ontology/code/)",
            },
            "ontology": match self.quipu_remote_url() {
                Some(url) => serde_json::json!({
                    "configured": true,
                    "url": url,
                    "contains": "the organisation ontology served by a remote Quipu",
                }),
                None => serde_json::json!({
                    "configured": false,
                    "note": "No ontology Quipu configured (set `quipu_endpoint` in \
        bobbin's config, or BOBBIN_QUIPU_REMOTE). Ontology facts are NOT being consulted — \
        an empty ontology section here means NOT ASKED, not 'not present'.",
                }),
            },
        })
    }

    /// Run a SPARQL SELECT against the remote ontology, as a reportable section.
    ///
    /// A transport failure is reported LOUDLY as an `error` rather than collapsing
    /// into an empty result set — the whole defect this fixes was an empty answer
    /// that was indistinguishable from a real absence.
    async fn ontology_sparql_section(&self, body: serde_json::Value) -> serde_json::Value {
        let Some(base) = self.quipu_remote_url() else {
            return serde_json::json!({
                "consulted": false,
                "reason": "no ontology Quipu configured (quipu_endpoint / BOBBIN_QUIPU_REMOTE unset)",
            });
        };
        match Self::quipu_remote_post(&base, "/query", body).await {
            Ok(v) => {
                let mut out = v;
                if let Some(o) = out.as_object_mut() {
                    o.insert("consulted".into(), serde_json::json!(true));
                }
                out
            }
            Err(e) => serde_json::json!({
                "consulted": true,
                "error": format!("{e:#}"),
                "warning": "The ontology could NOT be reached. This is a TRANSPORT \
            FAILURE, not an empty result — do not read it as 'the fact is absent'.",
            }),
        }
    }

    /// Open the Quipu knowledge graph store
    pub(super) fn open_quipu_store(&self) -> Result<quipu::Store> {
        let db_path = self.quipu_store_path();
        // Ensure parent directory exists.
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create quipu store directory")?;
        }
        let mut store = quipu::Store::open(db_path.to_string_lossy().as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to open quipu store: {e}"))?;
        self.attach_shared_embedder(&mut store);
        Ok(store)
    }

    /// Hand quipu bobbin's ONNX embedder (bobbin-di7 Phase 2).
    ///
    /// Best-effort by design: a store with no provider still stores facts, it
    /// just does not vectorise them, and failing the whole write because the
    /// model is missing would be worse than degrading. But the degradation is
    /// **logged**, because a graph that is silently unsearchable semantically
    /// looks exactly like one where nothing matched.
    ///
    /// Sharing one embedder is not just an efficiency: two ONNX sessions would
    /// mean two vector spaces, and cosine similarity across different models is
    /// a number rather than a measurement.
    fn attach_shared_embedder(&self, store: &mut quipu::Store) {
        let config = Config::load(&Config::config_path(self.repo_root())).unwrap_or_default();
        let model_dir = match Config::model_cache_dir() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("knowledge graph will not auto-embed (no model cache dir): {e:#}");
                return;
            }
        };
        match Embedder::from_config(&config.embedding, &model_dir) {
            Ok(embedder) => {
                crate::knowledge::embedding::attach_embedder(store, std::sync::Arc::new(embedder));
            }
            Err(e) => {
                tracing::warn!("knowledge graph will not auto-embed (embedder unavailable): {e:#}");
            }
        }
    }
}

#[cfg(all(test, feature = "knowledge"))]
mod share_adapter_tests {
    use super::*;

    #[test]
    fn canonical_scope_translation_refuses_ambiguous_values() {
        assert_eq!(
            BobbinMcpServer::share_scope(KnowledgeShareScopeKind::Root, None).unwrap(),
            serde_json::json!({"kind": "root", "value": null})
        );
        assert!(BobbinMcpServer::share_scope(
            KnowledgeShareScopeKind::Root,
            Some("unexpected".into())
        )
        .is_err());
        assert!(BobbinMcpServer::share_scope(KnowledgeShareScopeKind::Group, None).is_err());
        assert_eq!(
            BobbinMcpServer::share_scope(
                KnowledgeShareScopeKind::Group,
                Some("repo:example".into())
            )
            .unwrap(),
            serde_json::json!({"kind": "group", "value": "repo:example"})
        );
    }

    #[test]
    fn export_scope_uses_quipus_wire_keys() {
        let group = BobbinMcpServer::share_scope(
            KnowledgeShareScopeKind::Group,
            Some("repo:example".into()),
        )
        .unwrap();
        assert_eq!(
            BobbinMcpServer::export_scope(group),
            serde_json::json!({"group_id": "repo:example"})
        );
    }
}

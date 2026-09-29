use anyhow::Result;

/// Prove the store enforces `/knot` graph routing before writing into `graph`.
///
/// Two checks, both required: a write aimed at a deliberately UNREGISTERED sentinel
/// graph must be refused with "unknown graph" (so the key is not being dropped), and
/// the requested graph itself must be accepted by an empty write (so it is registered).
/// The empty write replaces nothing: it carries no snapshot key.
pub(super) fn require_graph_routing(store: &mut quipu::Store, graph: &str) -> Result<()> {
    let sentinel = format!("{graph}/bobbin-unregistered-sentinel");
    match quipu::tool_knot(
        store,
        &serde_json::json!({"turtle": "", "actor": "bobbin", "source": "chunk-graph-probe", "graph": sentinel}),
    ) {
        Ok(_) => anyhow::bail!(
            "quipu ACCEPTED a write aimed at an unregistered sentinel graph, so it is dropping \
             the /knot 'graph' key: chunks meant for <{graph}> would land in ROOT. Refusing to push."
        ),
        Err(e) if e.to_string().contains("unknown graph") => {}
        Err(e) => anyhow::bail!("quipu graph-routing probe failed: {e}"),
    }
    quipu::tool_knot(
        store,
        &serde_json::json!({"turtle": "", "actor": "bobbin", "source": "chunk-graph-probe", "graph": graph}),
    )
    .map_err(|e| anyhow::anyhow!("target graph <{graph}> is not usable (register it first): {e}"))?;
    Ok(())
}

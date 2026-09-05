//! The tool-annotation guard (aegis-fmcth7).
//!
//! Every MCP tool bobbin ships must declare its annotations, and must declare
//! them EXPLICITLY. This module holds the test that enforces it.
//!
//! WHY THIS EXISTS. bobbin-mcp shipped 35 tools and zero annotations. Codex
//! decides whether a tool needs approval with
//!
//!     destructive_hint.unwrap_or(true) || open_world_hint.unwrap_or(true)
//!
//! so an unannotated tool is treated as destructive AND open-world, and under
//! `approval_policy = never` that is a flat refusal. bobbin's whole surface was
//! therefore reachable only by keeping a BLANKET pre-approval on the server —
//! the aegis-h3zyq0 workaround, still load-bearing for bobbin after
//! aegis-n549ii removed it for homelab.
//!
//! THE PART THAT IS EASY TO GET WRONG, AND WAS. `unwrap_or(true)` means an
//! OMITTED hint is not "unspecified", it is "dangerous". The MCP spec says
//! `destructive_hint` is "meaningful only when readOnlyHint == false", which
//! reads as licence to leave it off a read-only tool — and doing that ships
//! annotations that LOOK complete, pass an eyeball review, appear in
//! `tools/list`, and change nothing at all, because codex still resolves
//! `None -> true`. The first draft of this change did exactly that for all 24
//! read-only tools. So the guard below checks for EXPLICIT `Some(..)` on the
//! two hints codex actually reads, not merely that annotations exist.
//!
//! WHY THERE IS NO DEAD-CLASSIFICATION CHECK. homelab-mcp's equivalent guard
//! needs one, because its classification lives in a table beside the tools and
//! a stale entry there reads as coverage for a tool that no longer exists.
//! Bobbin's annotations live ON the tool declaration, so a classification
//! cannot outlive its tool — deleting the tool deletes the annotation. That is
//! a property of the shape, not a check we skipped.

#[cfg(test)]
mod tests {
    use super::super::server::BobbinMcpServer;

    /// Every shipped tool declares annotations, and declares the two hints
    /// codex reads explicitly.
    #[test]
    fn every_tool_is_annotated_explicitly() {
        let tools = BobbinMcpServer::advertised_tools();

        // CONTROL. An empty router would satisfy every assertion below
        // vacuously and report a confident pass — the exact failure this
        // whole guard exists to prevent, one level up.
        assert!(
            tools.len() >= 30,
            "expected the full tool surface, got {} — a truncated or empty \
             router makes every check below vacuous",
            tools.len()
        );

        let mut unannotated = Vec::new();
        let mut implicit = Vec::new();

        for tool in &tools {
            let Some(ann) = tool.annotations.as_ref() else {
                unannotated.push(tool.name.to_string());
                continue;
            };
            // `unwrap_or(true)` on the consumer side means None == dangerous.
            // These two decide approval, so neither may be left to a default.
            if ann.destructive_hint.is_none() {
                implicit.push(format!("{}: destructive_hint", tool.name));
            }
            if ann.open_world_hint.is_none() {
                implicit.push(format!("{}: open_world_hint", tool.name));
            }
            if ann.read_only_hint.is_none() {
                implicit.push(format!("{}: read_only_hint", tool.name));
            }
        }

        assert!(
            unannotated.is_empty(),
            "{} tool(s) ship with NO annotations: {:?}\n\
             An unannotated tool is un-callable for every codex worker under \
             `approval_policy = never`. Add `annotations(...)` to its \
             `#[tool(...)]` — see the module docs for the classification.",
            unannotated.len(),
            unannotated
        );

        assert!(
            implicit.is_empty(),
            "{} hint(s) left implicit: {:?}\n\
             Codex resolves a missing hint with `unwrap_or(true)`, so omitting \
             one is the same as declaring the tool dangerous. Set it \
             explicitly, even where the MCP spec calls it 'not meaningful'.",
            implicit.len(),
            implicit
        );
    }

    /// A read-only tool must not also claim to be destructive: that pair is
    /// contradictory, and the contradiction resolves the unsafe way.
    #[test]
    fn read_only_tools_are_not_destructive() {
        let tools = BobbinMcpServer::advertised_tools();
        assert!(!tools.is_empty(), "control: the router must not be empty");

        let bad: Vec<String> = tools
            .iter()
            .filter_map(|t| {
                let ann = t.annotations.as_ref()?;
                (ann.read_only_hint == Some(true) && ann.destructive_hint == Some(true))
                    .then(|| t.name.to_string())
            })
            .collect();

        assert!(
            bad.is_empty(),
            "tool(s) marked BOTH read-only and destructive: {:?}",
            bad
        );
    }

    /// The tool surface is 35. A split that silently dropped a tool would pass
    /// every other check here, so the count is pinned deliberately: change it
    /// in the same commit that adds or removes a tool, never to make a test go
    /// green.
    #[test]
    fn the_tool_surface_is_the_expected_size() {
        let tools = BobbinMcpServer::advertised_tools();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        names.sort_unstable();
        assert_eq!(
            tools.len(),
            35,
            "tool count changed — surface is now: {:?}",
            names
        );
    }
}

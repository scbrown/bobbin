use super::types::FileSymbolInfo;
use std::collections::HashSet;

/// Format a context bundle into a compact text block for Claude Code injection.
///
/// Produces a plain-text summary of relevant code chunks, enforcing a hard line
/// budget on the output. The `threshold` filters out low-scoring chunks. The
/// output budget is taken from `bundle.budget.max_lines`.
pub(super) fn format_context_for_injection(
    bundle: &crate::search::context::ContextBundle,
    threshold: f32,
    show_docs: bool,
    injection_id: Option<&str>,
    format_mode: &str,
) -> String {
    use crate::types::FileCategory;
    use std::fmt::Write;

    let budget = bundle.budget.max_lines;
    let mut out = String::new();

    // Format header based on mode
    match format_mode {
        "xml" => {
            let inj_attr = injection_id
                .map(|id| format!(" injection_id=\"{}\"", id))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "<bobbin-context files=\"{}\" source=\"{}\" docs=\"{}\" lines=\"{}/{}\"{}>\n",
                bundle.summary.total_files,
                bundle.summary.source_files,
                bundle.summary.doc_files,
                bundle.budget.used_lines,
                bundle.budget.max_lines,
                inj_attr,
            );
        }
        "minimal" => {
            let inj_suffix = injection_id
                .map(|id| format!(" [injection_id: {}]", id))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "# Bobbin context ({} files, {}/{} lines){}",
                bundle.summary.total_files,
                bundle.budget.used_lines,
                bundle.budget.max_lines,
                inj_suffix,
            );
        }
        _ => {
            let header = if let Some(inj_id) = injection_id {
                format!(
                    "Bobbin found {} relevant files ({} source, {} docs, {}/{} budget lines) [injection_id: {}]:",
                    bundle.summary.total_files,
                    bundle.summary.source_files,
                    bundle.summary.doc_files,
                    bundle.budget.used_lines,
                    bundle.budget.max_lines,
                    inj_id,
                )
            } else {
                format!(
                    "Bobbin found {} relevant files ({} source, {} docs, {}/{} budget lines):",
                    bundle.summary.total_files,
                    bundle.summary.source_files,
                    bundle.summary.doc_files,
                    bundle.budget.used_lines,
                    bundle.budget.max_lines,
                )
            };
            out.push_str(&header);
            out.push('\n');
        }
    }

    // Partition files: source/test/custom first, then docs/config
    let source_files: Vec<_> = bundle.files.iter()
        .filter(|f| !f.category.is_doc_like())
        .collect();
    let doc_files: Vec<_> = bundle.files.iter()
        .filter(|f| f.category.is_doc_like())
        .collect();

    // Emit source files section
    if !source_files.is_empty() {
        if format_mode != "xml" && format_mode != "minimal" {
            let _ = write!(out, "\n=== Source Files ===\n");
        }
        format_file_chunks(&mut out, &source_files, threshold, budget, format_mode);
    }

    // Emit documentation section (if show_docs is true)
    if show_docs && !doc_files.is_empty() {
        if format_mode != "xml" && format_mode != "minimal" {
            let _ = write!(out, "\n=== Documentation ===\n");
        }
        format_file_chunks(&mut out, &doc_files, threshold, budget, format_mode);
    }

    if format_mode == "xml" {
        let _ = write!(out, "</bobbin-context>\n");
    }

    // Final enforcement: trim to budget
    let lines: Vec<&str> = out.lines().collect();
    if lines.len() > budget {
        lines[..budget].join("\n") + "\n"
    } else {
        out
    }
}

/// Format chunks from a list of files into the output string, respecting budget.
pub(super) fn format_file_chunks(
    out: &mut String,
    files: &[&crate::search::context::ContextFile],
    threshold: f32,
    budget: usize,
    format_mode: &str,
) {
    use std::fmt::Write;

    // Track line count incrementally to avoid O(n²) recounting
    let mut current_lines = out.lines().count();

    for file in files {
        // Build display path with repo prefix when available and not already present
        let display_path = match &file.repo {
            Some(repo) if !file.path.starts_with("repos/") && !file.path.starts_with("/") && !file.path.starts_with("beads:") => {
                format!("repos/{}/{}", repo, file.path)
            }
            _ => file.path.clone(),
        };

        for chunk in &file.chunks {
            if chunk.score < threshold {
                continue;
            }
            let name = chunk
                .name
                .as_ref()
                .map(|n| format!(" {}", n))
                .unwrap_or_default();
            let content_str = chunk.content.as_deref().unwrap_or("");
            let chunk_type_str = serde_json::to_string(&chunk.chunk_type)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            let chunk_section = format_search_chunk(
                &display_path,
                chunk.start_line,
                chunk.end_line,
                &name,
                &chunk_type_str,
                chunk.score,
                content_str,
                "",
                format_mode,
            );

            // Check if adding this chunk would exceed budget
            let chunk_line_count = chunk_section.lines().count();
            if current_lines + chunk_line_count > budget {
                return;
            }
            current_lines += chunk_line_count;
            let _ = write!(out, "{}", chunk_section);
        }
    }
}

/// Format a ContextResponse into structured text for injection, with optional bundle annotations.
pub(super) fn format_context_response_with_bundles(
    resp: &crate::http::client::ContextResponse,
    budget: usize,
    show_docs: bool,
    injection_id: &str,
    format_mode: &str,
    matched_bundles: &[crate::tags::BundleConfig],
    bundle_auto_inject: bool,
    bundle_inject_lines: usize,
    bundle_max_inject: usize,
) -> String {
    format_context_response_inner(resp, budget, show_docs, injection_id, format_mode, matched_bundles, bundle_auto_inject, bundle_inject_lines, bundle_max_inject)
}

pub(super) fn format_context_response_inner(
    resp: &crate::http::client::ContextResponse,
    budget: usize,
    show_docs: bool,
    injection_id: &str,
    format_mode: &str,
    matched_bundles: &[crate::tags::BundleConfig],
    bundle_auto_inject: bool,
    bundle_inject_lines: usize,
    bundle_max_inject: usize,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    // Header with summary and injection_id for feedback reference
    match format_mode {
        "xml" => {
            let _ = writeln!(
                out,
                "<bobbin-context files=\"{}\" direct=\"{}\" coupled=\"{}\" bridged=\"{}\" chunks=\"{}\" budget=\"{}\" injection_id=\"{}\">",
                resp.summary.total_files,
                resp.summary.direct_hits,
                resp.summary.coupled_additions,
                resp.summary.bridged_additions,
                resp.summary.total_chunks,
                budget,
                injection_id,
            );
        }
        "minimal" => {
            let _ = writeln!(
                out,
                "# Bobbin context ({} files, {}/{} lines) [injection_id: {}]",
                resp.summary.total_files,
                resp.summary.total_chunks,
                budget,
                injection_id,
            );
        }
        _ => {
            let _ = writeln!(
                out,
                "Bobbin found {} relevant files ({} direct, {} coupled, {} bridged, {}/{} budget lines) [injection_id: {}]:",
                resp.summary.total_files,
                resp.summary.direct_hits,
                resp.summary.coupled_additions,
                resp.summary.bridged_additions,
                resp.summary.total_chunks,
                budget,
                injection_id,
            );
        }
    }

    // Bundle annotations — either inline content or signpost
    if !matched_bundles.is_empty() {
        let inject_count = matched_bundles.len().min(bundle_max_inject);
        let _ = writeln!(out);
        for (i, bundle) in matched_bundles.iter().enumerate() {
            let _ = writeln!(out, "📦 bundle:{} — \"{}\"", bundle.name, bundle.description);
            if bundle_auto_inject && i < inject_count {
                // Render compact inline content: refs, files, docs, sub-bundles
                let mut lines_used = 0;
                let max_lines = bundle_inject_lines;

                // Refs (most valuable — specific symbols)
                if !bundle.refs.is_empty() && lines_used < max_lines {
                    let _ = writeln!(out, "   Refs:");
                    lines_used += 1;
                    for ref_str in &bundle.refs {
                        if lines_used >= max_lines { break; }
                        if let Some(parsed) = crate::tags::BundleRef::parse(ref_str) {
                            let _ = writeln!(out, "   - {}", parsed.display_l0());
                        } else {
                            let _ = writeln!(out, "   - {}", ref_str);
                        }
                        lines_used += 1;
                    }
                }

                // Files
                if !bundle.files.is_empty() && lines_used < max_lines {
                    let _ = writeln!(out, "   Files:");
                    lines_used += 1;
                    for f in &bundle.files {
                        if lines_used >= max_lines { break; }
                        let _ = writeln!(out, "   - {}", f);
                        lines_used += 1;
                    }
                }

                // Docs
                if !bundle.docs.is_empty() && lines_used < max_lines {
                    let _ = writeln!(out, "   Docs:");
                    lines_used += 1;
                    for d in &bundle.docs {
                        if lines_used >= max_lines { break; }
                        let _ = writeln!(out, "   - {}", d);
                        lines_used += 1;
                    }
                }

                // Beads
                if !bundle.beads.is_empty() && lines_used < max_lines {
                    let _ = writeln!(out, "   Beads:");
                    lines_used += 1;
                    for b in &bundle.beads {
                        if lines_used >= max_lines { break; }
                        let _ = writeln!(out, "   - bead:{}", b);
                        lines_used += 1;
                    }
                }

                // Includes (other bundles)
                if !bundle.includes.is_empty() && lines_used < max_lines {
                    let _ = writeln!(out, "   Includes: {}", bundle.includes.join(", "));
                }

                let _ = writeln!(out, "   → `bobbin bundle show {} --deep` for full source", bundle.name);
            } else {
                let _ = writeln!(out, "   → `bobbin bundle show {}` for full context", bundle.name);
            }
        }
    }

    // Partition files by type
    let is_doc = |path: &str| -> bool {
        path.ends_with(".md") || path.ends_with(".txt") || path.ends_with(".rst")
            || path.ends_with(".adoc") || path.contains("/docs/")
    };

    let source_files: Vec<_> = resp.files.iter().filter(|f| !is_doc(&f.path)).collect();
    let doc_files: Vec<_> = resp.files.iter().filter(|f| is_doc(&f.path)).collect();

    let mut line_count = out.lines().count();

    if !source_files.is_empty() {
        match format_mode {
            "xml" => { /* no section header in xml mode */ }
            "minimal" => { /* no section header in minimal mode */ }
            _ => {
                let _ = write!(out, "\n=== Source Files ===\n");
                line_count += 2;
            }
        }
        format_remote_file_chunks(&mut out, &source_files, budget, &mut line_count, format_mode);
    }

    if show_docs && !doc_files.is_empty() {
        match format_mode {
            "xml" => { /* no section header in xml mode */ }
            "minimal" => { /* no section header in minimal mode */ }
            _ => {
                let _ = write!(out, "\n=== Documentation ===\n");
                line_count += 2;
            }
        }
        format_remote_file_chunks(&mut out, &doc_files, budget, &mut line_count, format_mode);
    }

    if format_mode == "xml" {
        let _ = write!(out, "</bobbin-context>\n");
    }

    out
}
/// Format chunks from remote context response files into output string.
pub(super) fn format_remote_file_chunks(
    out: &mut String,
    files: &[&crate::http::client::ContextFileOutput],
    budget: usize,
    line_count: &mut usize,
    format_mode: &str,
) {
    use std::fmt::Write;

    for file in files {
        // Build display path with repo prefix when available and not already present
        let display_path = match &file.repo {
            Some(repo) if !file.path.starts_with("repos/") && !file.path.starts_with("/") && !file.path.starts_with("beads:") => {
                format!("repos/{}/{}", repo, file.path)
            }
            _ => file.path.clone(),
        };

        // Show coupling info if present
        let relevance_info = if !file.coupled_to.is_empty() {
            format!(" [coupled via {}]", file.coupled_to.join(", "))
        } else if file.relevance == "bridged" {
            " [bridged from docs]".to_string()
        } else {
            String::new()
        };

        for chunk in &file.chunks {
            let name = chunk
                .name
                .as_ref()
                .map(|n| format!(" {}", n))
                .unwrap_or_default();
            let content_str = chunk.content.as_deref().unwrap_or("");
            let chunk_section = format_search_chunk(
                &display_path,
                chunk.start_line,
                chunk.end_line,
                &name,
                &chunk.chunk_type,
                chunk.score,
                content_str,
                &relevance_info,
                format_mode,
            );

            let chunk_line_count = chunk_section.lines().count();
            if *line_count + chunk_line_count > budget {
                return;
            }
            *line_count += chunk_line_count;
            let _ = write!(out, "{}", chunk_section);
        }
    }
}

/// Format a single chunk according to the injection format mode.
pub(super) fn format_search_chunk(
    path: &str,
    start_line: u32,
    end_line: u32,
    name: &str,
    chunk_type: &str,
    score: f32,
    content: &str,
    relevance_info: &str,
    format_mode: &str,
) -> String {
    let content_suffix = if content.ends_with('\n') { "" } else { "\n" };
    match format_mode {
        "minimal" => {
            // Clean, minimal format — just path and content, no metadata noise
            format!(
                "\n# {} (lines {}-{})\n{}{}",
                path, start_line, end_line, content, content_suffix,
            )
        }
        "verbose" => {
            // Standard format + explicit name/type on separate line for clarity
            let mut s = format!(
                "\n--- {}:{}-{}{} ({}, score {:.2}){} ---\n",
                path, start_line, end_line, name, chunk_type, score, relevance_info,
            );
            if !name.is_empty() {
                s.push_str(&format!("  // {}{}\n", chunk_type, name));
            }
            s.push_str(content);
            s.push_str(content_suffix);
            s
        }
        "xml" => {
            // XML-structured format for LLMs that may parse structure better
            let name_attr = if name.is_empty() {
                String::new()
            } else {
                format!(" name=\"{}\"", name.trim())
            };
            let rel_attr = if relevance_info.is_empty() {
                String::new()
            } else {
                format!(" relevance=\"{}\"", relevance_info.trim().trim_matches(|c| c == '[' || c == ']'))
            };
            format!(
                "<file path=\"{}\" lines=\"{}-{}\" type=\"{}\" score=\"{:.2}\"{}{}>
{}{}</file>\n",
                path, start_line, end_line, chunk_type, score, name_attr, rel_attr,
                content, content_suffix,
            )
        }
        _ => {
            // "standard" — the current default format
            format!(
                "\n--- {}:{}-{}{} ({}, score {:.2}){} ---\n{}{}",
                path, start_line, end_line, name, chunk_type, score, relevance_info,
                content, content_suffix,
            )
        }
    }
}

/// Format the header for search fallback injection.
pub(super) fn format_search_fallback_header(result_count: usize, injection_id: &str, format_mode: &str) -> String {
    match format_mode {
        "xml" => format!(
            "<bobbin-context chunks=\"{}\" mode=\"search-fallback\" injection_id=\"{}\">\n",
            result_count, injection_id,
        ),
        "minimal" => format!(
            "# Bobbin context ({} chunks, search fallback) [injection_id: {}]\n",
            result_count, injection_id,
        ),
        _ => {
            let mut out = format!(
                "Bobbin found {} relevant chunks (via search fallback) [injection_id: {}]:\n",
                result_count, injection_id,
            );
            out.push_str("\n=== Source Files ===\n");
            out
        }
    }
}

/// Format session context as compact markdown for Claude Code SessionStart recovery.
///
/// Produces a markdown summary of working state (modified files, recent commits,
/// coupled files) within the given line budget. Budget is enforced on output lines;
/// if truncation is needed the last line is a notice message, counted within budget.
pub(super) fn format_session_context(
    modified_files: &[String],
    recent_commits: &[String],
    file_symbols: &[FileSymbolInfo],
    coupled_files: &[(String, String, f32)],
    budget: usize,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push("## Working Context (recovered after compaction)".to_string());
    lines.push(String::new());

    // Modified files section
    if !modified_files.is_empty() {
        lines.push("### Modified files".to_string());
        for file in modified_files {
            // Find symbols for this file
            let symbols_str = file_symbols
                .iter()
                .find(|fs| fs.path == *file)
                .map(|fs| {
                    let names: Vec<String> = fs
                        .symbols
                        .iter()
                        .take(5)
                        .map(|s| s.name.clone())
                        .collect();
                    if names.is_empty() {
                        String::new()
                    } else {
                        let count = fs.symbols.len();
                        let display = names.join(", ");
                        if count > 5 {
                            format!(" ({} symbols: {}, ...)", count, display)
                        } else {
                            format!(" ({} symbols: {})", count, display)
                        }
                    }
                })
                .unwrap_or_default();
            lines.push(format!("- {}{}", file, symbols_str));
        }
        lines.push(String::new());
    }

    // Recent commits section
    if !recent_commits.is_empty() {
        lines.push("### Recent commits".to_string());
        for commit in recent_commits {
            lines.push(format!("- {}", commit));
        }
        lines.push(String::new());
    }

    // File symbols for non-modified files (recently changed files that aren't modified)
    let modified_set: HashSet<&String> = modified_files.iter().collect();
    let other_symbols: Vec<&FileSymbolInfo> = file_symbols
        .iter()
        .filter(|fs| !modified_set.contains(&fs.path))
        .collect();

    if !other_symbols.is_empty() {
        lines.push("### Recently changed files".to_string());
        for fs in &other_symbols {
            let names: Vec<String> = fs
                .symbols
                .iter()
                .take(5)
                .map(|s| s.name.clone())
                .collect();
            let symbols_str = if names.is_empty() {
                String::new()
            } else {
                let count = fs.symbols.len();
                let display = names.join(", ");
                if count > 5 {
                    format!(" ({} symbols: {}, ...)", count, display)
                } else {
                    format!(" ({} symbols: {})", count, display)
                }
            };
            lines.push(format!("- {}{}", fs.path, symbols_str));
        }
        lines.push(String::new());
    }

    // Coupled files section
    if !coupled_files.is_empty() {
        lines.push("### Related files (via coupling)".to_string());
        for (path, coupled_to, score) in coupled_files.iter().take(5) {
            lines.push(format!(
                "- {} (coupled with {}, score: {:.2})",
                path, coupled_to, score
            ));
        }
        lines.push(String::new());
    }

    // Enforce budget (reserve 1 line for truncation message if needed)
    if lines.len() > budget {
        lines.truncate(budget.saturating_sub(1));
        lines.push("... (truncated to fit budget)".to_string());
    }

    lines.join("\n")
}

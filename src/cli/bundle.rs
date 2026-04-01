use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::tags::{BundleConfig, BundleRef, RefTarget, TagsConfig};

#[derive(Args)]
pub struct BundleArgs {
    #[command(subcommand)]
    command: BundleCommands,

    /// Directory containing .bobbin/ config
    #[arg(default_value = ".", global = true)]
    path: PathBuf,
}

#[derive(Subcommand)]
enum BundleCommands {
    /// List all bundles (L0 map view)
    List(ListArgs),
    /// Show a bundle's contents (L1 outline by default, L2 with --deep)
    Show(ShowArgs),
    /// Create a new bundle
    Create(CreateArgs),
    /// Add a member (file, ref, doc, keyword, tag, bead) to a bundle
    Add(AddArgs),
    /// Remove a member from a bundle
    Remove(RemoveArgs),
    /// Check bundle health: validate all refs/files still resolve
    Check(CheckArgs),
    /// Suggest new bundles from coupling graph analysis
    Suggest(SuggestArgs),
    /// Show bundle usage stats (beads with b:<slug> labels)
    Stats(StatsArgs),
}

#[derive(Args)]
struct ListArgs {
    /// Filter bundles by repo
    #[arg(long)]
    repo: Option<String>,
    /// Show flat list instead of tree
    #[arg(long)]
    flat: bool,
}

#[derive(Args)]
struct ShowArgs {
    /// Bundle name or slug (e.g. "context", "context/pipeline", "b:context-pipeline")
    name: String,
    /// Show L2 deep view (full file/ref contents)
    #[arg(long)]
    deep: bool,
    /// Show L0 map view (sub-bundles only)
    #[arg(long)]
    map: bool,
    /// Override repo root for file resolution (e.g. path to bobbin source checkout)
    #[arg(long)]
    repo_root: Option<PathBuf>,
}

#[derive(Args)]
struct CreateArgs {
    /// Bundle name (use `/` for hierarchy, e.g. "search/reranking")
    name: String,
    /// One-line description
    #[arg(short, long)]
    description: Option<String>,
    /// Keywords (comma-separated or repeated)
    #[arg(short, long, value_delimiter = ',')]
    keywords: Vec<String>,
    /// Explicit files
    #[arg(short, long, value_delimiter = ',')]
    files: Vec<String>,
    /// Sub-file refs (file::symbol, file#heading)
    #[arg(short, long, value_delimiter = ',')]
    refs: Vec<String>,
    /// Documentation files
    #[arg(long, value_delimiter = ',')]
    docs: Vec<String>,
    /// Tags for membership
    #[arg(short, long, value_delimiter = ',')]
    tags: Vec<String>,
    /// Include other bundles at L2
    #[arg(short, long, value_delimiter = ',')]
    includes: Vec<String>,
    /// Bead references (rig:bead-id, e.g. "aegis:aegis-h8x")
    #[arg(short = 'b', long, value_delimiter = ',')]
    beads: Vec<String>,
    /// Repo scope
    #[arg(long, value_delimiter = ',')]
    repos: Vec<String>,
    /// Custom slug override
    #[arg(long)]
    slug: Option<String>,
    /// Write to global config (~/.config/bobbin/tags.toml) instead of local
    #[arg(long)]
    global: bool,
}

#[derive(Args)]
struct AddArgs {
    /// Bundle name or slug
    name: String,
    /// Files to add
    #[arg(short, long, value_delimiter = ',')]
    files: Vec<String>,
    /// Refs to add (file::symbol, file#heading)
    #[arg(short, long, value_delimiter = ',')]
    refs: Vec<String>,
    /// Docs to add
    #[arg(long, value_delimiter = ',')]
    docs: Vec<String>,
    /// Keywords to add
    #[arg(short, long, value_delimiter = ',')]
    keywords: Vec<String>,
    /// Tags to add
    #[arg(short, long, value_delimiter = ',')]
    tags: Vec<String>,
    /// Includes to add
    #[arg(short, long, value_delimiter = ',')]
    includes: Vec<String>,
    /// Bead references to add (rig:bead-id, e.g. "aegis:aegis-h8x")
    #[arg(short = 'b', long, value_delimiter = ',')]
    beads: Vec<String>,
    /// Write to global config
    #[arg(long)]
    global: bool,
}

#[derive(Args)]
struct RemoveArgs {
    /// Bundle name or slug
    name: String,
    /// Files to remove
    #[arg(short, long, value_delimiter = ',')]
    files: Vec<String>,
    /// Refs to remove
    #[arg(short, long, value_delimiter = ',')]
    refs: Vec<String>,
    /// Docs to remove
    #[arg(long, value_delimiter = ',')]
    docs: Vec<String>,
    /// Keywords to remove
    #[arg(short, long, value_delimiter = ',')]
    keywords: Vec<String>,
    /// Tags to remove
    #[arg(short, long, value_delimiter = ',')]
    tags: Vec<String>,
    /// Includes to remove
    #[arg(short, long, value_delimiter = ',')]
    includes: Vec<String>,
    /// Bead references to remove
    #[arg(short = 'b', long, value_delimiter = ',')]
    beads: Vec<String>,
    /// Remove the entire bundle
    #[arg(long)]
    all: bool,
    /// Write to global config
    #[arg(long)]
    global: bool,
}

#[derive(Args)]
struct CheckArgs {
    /// Check a specific bundle (default: all bundles)
    name: Option<String>,
    /// Override repo root for file resolution
    #[arg(long)]
    repo_root: Option<PathBuf>,
}

#[derive(Args)]
struct StatsArgs {
    /// Show stats for a specific bundle (default: all)
    name: Option<String>,
}

#[derive(Args)]
struct SuggestArgs {
    /// Minimum coupling score to consider (default: 0.3)
    #[arg(long, default_value_t = 0.3)]
    threshold: f32,
    /// Minimum cluster size to suggest (default: 3)
    #[arg(long, default_value_t = 3)]
    min_size: usize,
    /// Filter to a specific repo
    #[arg(long)]
    repo: Option<String>,
}

pub async fn run(args: BundleArgs, output: OutputConfig) -> Result<()> {
    match args.command {
        BundleCommands::List(list_args) => run_list(args.path, list_args, output).await,
        BundleCommands::Show(show_args) => run_show(args.path, show_args, output).await,
        BundleCommands::Create(create_args) => run_create(args.path, create_args, output).await,
        BundleCommands::Add(add_args) => run_add(args.path, add_args, output).await,
        BundleCommands::Remove(remove_args) => run_remove(args.path, remove_args, output).await,
        BundleCommands::Check(check_args) => run_check(args.path, check_args, output).await,
        BundleCommands::Suggest(suggest_args) => run_suggest(args.path, suggest_args, output).await,
        BundleCommands::Stats(stats_args) => run_stats(args.path, stats_args, output).await,
    }
}

/// Walk up from the given path to find a directory containing .bobbin/tags.toml.
/// Unlike find_bobbin_root (which just needs .bobbin/ to exist), this looks for
/// tags.toml specifically and doesn't stop at git boundaries.
fn find_tags_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let tags = current.join(".bobbin").join("tags.toml");
        if tags.exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Load tags config with bundle definitions, walking up directories to find tags.toml,
/// then falling back to global config.
fn load_tags_with_bundles(repo_root: &std::path::Path) -> TagsConfig {
    // First check the given path, then walk up to find tags.toml
    let effective_root = find_tags_root(repo_root).unwrap_or_else(|| repo_root.to_path_buf());
    let local_path = TagsConfig::tags_path(&effective_root);
    let mut config = TagsConfig::load_or_default(&local_path);

    // If no bundles found locally, check global config
    if config.bundles.is_empty() {
        if let Some(global_dir) = Config::global_config_dir() {
            let global_tags_path = global_dir.join("tags.toml");
            if global_tags_path.exists() {
                let global_config = TagsConfig::load_or_default(&global_tags_path);
                if !global_config.bundles.is_empty() {
                    config.bundles = global_config.bundles;
                }
            }
        }
    }

    config
}

async fn run_list(path: PathBuf, args: ListArgs, output: OutputConfig) -> Result<()> {
    let repo_root = path.canonicalize().unwrap_or(path);
    let config = load_tags_with_bundles(&repo_root);

    let bundles = &config.bundles;
    if bundles.is_empty() {
        if output.json {
            println!("{{\"bundles\":[]}}");
        } else {
            println!("No bundles defined. Add [[bundles]] entries to .bobbin/tags.toml");
        }
        return Ok(());
    }

    // Filter by repo if specified
    let filtered: Vec<&BundleConfig> = if let Some(ref repo) = args.repo {
        bundles
            .iter()
            .filter(|b| b.repos.is_empty() || b.repos.contains(repo))
            .collect()
    } else {
        bundles.iter().collect()
    };

    if output.json {
        let json_bundles: Vec<serde_json::Value> = filtered
            .iter()
            .map(|b| {
                serde_json::json!({
                    "name": b.name,
                    "slug": b.slug(),
                    "description": b.description,
                    "keywords": b.keywords,
                    "file_count": b.member_files().len(),
                    "ref_count": b.refs.len(),
                    "repos": b.repos,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "bundles": json_bundles }))?);
        return Ok(());
    }

    if args.flat {
        // Flat list
        for b in &filtered {
            let file_count = b.member_files().len();
            let ref_count = b.refs.len();
            println!(
                "  {} — {} ({} files, {} refs)",
                b.name, b.description, file_count, ref_count
            );
        }
    } else {
        // Tree view: group by hierarchy
        println!("Context Bundles ({} total):\n", filtered.len());
        print_bundle_tree(&filtered);
    }

    Ok(())
}

/// Print bundles as a tree based on `/` hierarchy in names.
fn print_bundle_tree(bundles: &[&BundleConfig]) {
    // Build tree structure
    let mut roots: Vec<&str> = Vec::new();
    let mut children: HashMap<&str, Vec<&BundleConfig>> = HashMap::new();
    let mut root_bundles: Vec<&BundleConfig> = Vec::new();

    for b in bundles {
        if let Some(parent) = b.parent_name() {
            children.entry(parent).or_default().push(b);
        } else {
            roots.push(&b.name);
            root_bundles.push(b);
        }
    }

    for root_bundle in &root_bundles {
        let file_count = root_bundle.member_files().len();
        println!(
            "  {} — \"{}\" ({} files)",
            root_bundle.name, root_bundle.description, file_count
        );
        if let Some(kids) = children.get(root_bundle.name.as_str()) {
            for (i, kid) in kids.iter().enumerate() {
                let prefix = if i == kids.len() - 1 { "└──" } else { "├──" };
                let kid_files = kid.member_files().len();
                println!(
                    "    {} {} — \"{}\" ({} files)",
                    prefix,
                    kid.name.rsplit_once('/').map(|(_, n)| n).unwrap_or(&kid.name),
                    kid.description,
                    kid_files
                );
            }
        }
    }

    // Show any orphans (children whose parents aren't defined as bundles)
    let defined_names: Vec<&str> = bundles.iter().map(|b| b.name.as_str()).collect();
    for b in bundles {
        if let Some(parent) = b.parent_name() {
            if !defined_names.contains(&parent) && !roots.contains(&parent) {
                let file_count = b.member_files().len();
                println!(
                    "  {} — \"{}\" ({} files)",
                    b.name, b.description, file_count
                );
            }
        }
    }
}

async fn run_show(path: PathBuf, args: ShowArgs, output: OutputConfig) -> Result<()> {
    let default_root = path.canonicalize().unwrap_or(path);
    let repo_root = match &args.repo_root {
        Some(r) => r.canonicalize().unwrap_or_else(|_| r.clone()),
        None => default_root.clone(),
    };
    let config = load_tags_with_bundles(&default_root);

    // Resolve name: strip "b:" prefix if present, convert slug back to name
    let name = resolve_bundle_name(&args.name, &config.bundles);

    let bundle = config
        .bundles
        .iter()
        .find(|b| b.name == name)
        .ok_or_else(|| {
            let available: Vec<&str> = config.bundles.iter().map(|b| b.name.as_str()).collect();
            anyhow::anyhow!(
                "Bundle '{}' not found. Available: {}",
                name,
                if available.is_empty() {
                    "(none)".to_string()
                } else {
                    available.join(", ")
                }
            )
        })?;

    // Determine level
    let level = if args.map {
        0
    } else if args.deep {
        2
    } else {
        1
    };

    if output.json {
        let children: Vec<serde_json::Value> = config
            .bundles
            .iter()
            .filter(|b| b.parent_name() == Some(&bundle.name))
            .map(|b| serde_json::json!({
                "name": b.name,
                "description": b.description,
                "file_count": b.member_files().len(),
            }))
            .collect();
        let refs_json: Vec<serde_json::Value> = bundle.refs.iter().map(|r| {
            if let Some(parsed) = BundleRef::parse(r) {
                serde_json::json!({
                    "raw": r,
                    "file": parsed.file,
                    "target": match &parsed.target {
                        RefTarget::WholeFile => "file".to_string(),
                        RefTarget::Symbol(s) => format!("symbol:{}", s),
                        RefTarget::Heading(h) => format!("heading:{}", h),
                    },
                    "repo": parsed.repo,
                    "modifier": parsed.modifier,
                })
            } else {
                serde_json::json!({ "raw": r })
            }
        }).collect();
        let json = serde_json::json!({
            "name": bundle.name,
            "slug": bundle.slug(),
            "description": bundle.description,
            "level": level,
            "keywords": bundle.keywords,
            "tags": bundle.tags,
            "files": bundle.files,
            "refs": refs_json,
            "docs": bundle.docs,
            "beads": bundle.beads,
            "includes": bundle.includes,
            "implements": bundle.implements,
            "depends_on": bundle.depends_on,
            "tests": bundle.tests,
            "repos": bundle.repos,
            "children": children,
            "member_files": bundle.member_files(),
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    match level {
        0 => show_l0(bundle, &config.bundles),
        1 => show_l1(bundle, &config.bundles, &repo_root).await,
        2 => show_l2(bundle, &config.bundles, &repo_root).await,
        _ => unreachable!(),
    }
}

/// Resolve a bundle name from user input.
/// Handles: "context", "b:context-pipeline", "context-pipeline" (slug), "context/pipeline".
/// Normalize the file portion of a ref string, converting absolute paths to repo-relative.
/// Handles formats like `file::symbol`, `file#heading`, and `repo:file::symbol`.
fn normalize_ref_path(r: &str, repo_root: &std::path::Path) -> String {
    // Check for repo: prefix (no / or . before the colon)
    let (prefix, rest) = if let Some(colon_pos) = r.find(':') {
        let before = &r[..colon_pos];
        if !before.contains('/') && !before.contains('.') && !before.is_empty() {
            // Check this isn't a :: (symbol separator)
            if r[colon_pos..].starts_with("::") {
                ("", r)
            } else {
                (before, &r[colon_pos + 1..])
            }
        } else {
            ("", r)
        }
    } else {
        ("", r)
    };

    // Find the delimiter (:: or #)
    let (file_part, suffix) = if let Some(pos) = rest.find("::") {
        (&rest[..pos], &rest[pos..])
    } else if let Some(pos) = rest.find('#') {
        (&rest[..pos], &rest[pos..])
    } else {
        (rest, "")
    };

    // Normalize the file part if absolute
    let path = std::path::Path::new(file_part);
    if path.is_absolute() {
        if let Ok(rel) = path.strip_prefix(repo_root) {
            let normalized = rel.to_string_lossy();
            eprintln!(
                "note: normalized absolute ref path to repo-relative: {} → {}",
                file_part, normalized
            );
            return if prefix.is_empty() {
                format!("{}{}", normalized, suffix)
            } else {
                format!("{}:{}{}", prefix, normalized, suffix)
            };
        }
        eprintln!(
            "warning: absolute path in ref '{}' is outside repo root, storing as-is",
            r
        );
    }
    r.to_string()
}

fn resolve_bundle_name(input: &str, bundles: &[BundleConfig]) -> String {
    // Strip b: prefix
    let name = input.strip_prefix("b:").unwrap_or(input);

    // Direct name match
    if bundles.iter().any(|b| b.name == name) {
        return name.to_string();
    }

    // Slug match
    if let Some(b) = bundles.iter().find(|b| b.slug() == name) {
        return b.name.clone();
    }

    // Try converting hyphens to slashes (slug → name)
    let as_path = name.replace('-', "/");
    if bundles.iter().any(|b| b.name == as_path) {
        return as_path;
    }

    // Return as-is (will fail with helpful error later)
    name.to_string()
}

/// L0: Map view — bundle name, description, and sub-bundles.
fn show_l0(bundle: &BundleConfig, all_bundles: &[BundleConfig]) -> Result<()> {
    println!("📦 bundle:{} — \"{}\"", bundle.name, bundle.description);

    if !bundle.repos.is_empty() {
        println!("   Repos: {}", bundle.repos.join(", "));
    }

    if !bundle.keywords.is_empty() {
        println!("   Keywords: {}", bundle.keywords.join(", "));
    }

    // Show children
    let children: Vec<&BundleConfig> = all_bundles
        .iter()
        .filter(|b| b.parent_name() == Some(&bundle.name))
        .collect();

    if !children.is_empty() {
        println!();
        for child in &children {
            let child_short = child.name.rsplit_once('/').map(|(_, n)| n).unwrap_or(&child.name);
            println!("   {} — \"{}\"", child_short, child.description);
        }
    }

    // Show membership summary
    let files = bundle.member_files();
    let ref_count = bundle.refs.len();
    let tag_count = bundle.tags.len();
    let bead_count = bundle.beads.len();
    println!();
    if bead_count > 0 {
        println!(
            "   {} files, {} refs, {} tag memberships, {} beads",
            files.len(),
            ref_count,
            tag_count,
            bead_count
        );
    } else {
        println!(
            "   {} files, {} refs, {} tag memberships",
            files.len(),
            ref_count,
            tag_count
        );
    }
    println!("   → `bobbin bundle show {}` for outline", bundle.name);
    println!("   → `bobbin bundle show {} --deep` for full context", bundle.name);

    Ok(())
}

/// L1: Outline view — file paths with symbol names and preview.
async fn show_l1(
    bundle: &BundleConfig,
    all_bundles: &[BundleConfig],
    repo_root: &std::path::Path,
) -> Result<()> {
    println!("📦 bundle:{} — \"{}\"", bundle.name, bundle.description);
    println!();

    if !bundle.repos.is_empty() {
        println!("Repos: {}", bundle.repos.join(", "));
    }
    if !bundle.keywords.is_empty() {
        println!("Keywords: {}", bundle.keywords.join(", "));
    }
    if !bundle.tags.is_empty() {
        println!("Tags: {}", bundle.tags.join(", "));
    }

    // Show refs with parsed details
    if !bundle.refs.is_empty() {
        println!();
        println!("=== Refs ({}) ===", bundle.refs.len());
        for ref_str in &bundle.refs {
            if let Some(parsed) = BundleRef::parse(ref_str) {
                let display = parsed.display_l0();
                match &parsed.target {
                    RefTarget::WholeFile => {
                        // For whole files, try to show symbol count from the file
                        let symbol_hint = count_symbols_in_file(repo_root, &parsed.file).await;
                        println!("  {} {}", display, symbol_hint);
                    }
                    RefTarget::Symbol(_) => {
                        let modifier_hint = parsed
                            .modifier
                            .as_ref()
                            .map(|m| format!(" (+{})", m))
                            .unwrap_or_default();
                        println!("  {}{}", display, modifier_hint);
                    }
                    RefTarget::Heading(h) => {
                        println!("  {} (section: {})", display, h);
                    }
                }
            } else {
                println!("  {} (unparseable)", ref_str);
            }
        }
    }

    // Show explicit files
    if !bundle.files.is_empty() {
        println!();
        println!("=== Files ({}) ===", bundle.files.len());
        for f in &bundle.files {
            println!("  {}", f);
        }
    }

    // Show docs
    if !bundle.docs.is_empty() {
        println!();
        println!("=== Docs ({}) ===", bundle.docs.len());
        for d in &bundle.docs {
            println!("  {}", d);
        }
    }

    // Show beads
    if !bundle.beads.is_empty() {
        println!();
        println!("=== Beads ({}) ===", bundle.beads.len());
        for b in &bundle.beads {
            println!("  {}", b);
        }
    }

    // Show includes
    if !bundle.includes.is_empty() {
        println!();
        println!("=== Includes (L2 only) ===");
        for inc in &bundle.includes {
            if let Some(inc_bundle) = all_bundles.iter().find(|b| b.name == *inc) {
                println!("  {} — \"{}\"", inc, inc_bundle.description);
            } else {
                println!("  {} (not found)", inc);
            }
        }
    }

    // Show ontology relationships
    let has_relationships = !bundle.implements.is_empty()
        || !bundle.depends_on.is_empty()
        || !bundle.tests.is_empty();
    if has_relationships {
        println!();
        println!("=== Relationships ===");
        for r in &bundle.implements {
            println!("  implements: {}", r);
        }
        for r in &bundle.depends_on {
            println!("  depends_on: {}", r);
        }
        for r in &bundle.tests {
            println!("  tests: {}", r);
        }
    }

    // Show children
    let children: Vec<&BundleConfig> = all_bundles
        .iter()
        .filter(|b| b.parent_name() == Some(&bundle.name))
        .collect();
    if !children.is_empty() {
        println!();
        println!("=== Sub-bundles ({}) ===", children.len());
        for child in &children {
            let child_short = child.name.rsplit_once('/').map(|(_, n)| n).unwrap_or(&child.name);
            let files = child.member_files().len();
            println!("  {} — \"{}\" ({} files)", child_short, child.description, files);
        }
    }

    println!();
    println!("→ `bobbin bundle show {} --deep` for full file contents", bundle.name);

    Ok(())
}

/// L2: Deep view — full file contents for all refs and files.
async fn show_l2(
    bundle: &BundleConfig,
    all_bundles: &[BundleConfig],
    repo_root: &std::path::Path,
) -> Result<()> {
    println!("📦 bundle:{} — \"{}\" [DEEP]", bundle.name, bundle.description);
    println!();

    if !bundle.keywords.is_empty() {
        println!("Keywords: {}", bundle.keywords.join(", "));
    }

    let mut failed_reads: usize = 0;

    // Show refs with full content
    if !bundle.refs.is_empty() {
        println!();
        println!("=== Refs ({}) ===", bundle.refs.len());
        for ref_str in &bundle.refs {
            if let Some(parsed) = BundleRef::parse(ref_str) {
                println!();
                println!("--- {} ---", parsed.display_l0());
                // Read the actual file content
                let file_path = repo_root.join(&parsed.file);
                match std::fs::read_to_string(&file_path) {
                    Ok(content) => {
                        match &parsed.target {
                            RefTarget::WholeFile => {
                                // Show full file with line numbers
                                print_with_line_numbers(&content, None, None);
                            }
                            RefTarget::Symbol(symbol_name) => {
                                // Find the symbol in the file and show its body
                                print_symbol_from_content(&content, symbol_name, &parsed.file);
                            }
                            RefTarget::Heading(heading) => {
                                print_heading_section(&content, heading);
                            }
                        }
                    }
                    Err(_) => {
                        println!("  (file not found at {})", file_path.display());
                        failed_reads += 1;
                    }
                }
            }
        }
    }

    // Show explicit files with content
    if !bundle.files.is_empty() {
        println!();
        println!("=== Files ({}) ===", bundle.files.len());
        for f in &bundle.files {
            println!();
            println!("--- {} ---", f);
            // Handle cross-repo refs (repo:path)
            let file_path = if f.contains(':') {
                // Cross-repo — just show the path, can't resolve locally
                println!("  (cross-repo file, use `bobbin bundle show {} --deep` on the target repo)", bundle.name);
                continue;
            } else {
                repo_root.join(f)
            };
            match std::fs::read_to_string(&file_path) {
                Ok(content) => {
                    print_with_line_numbers(&content, None, Some(100));
                }
                Err(_) => {
                    println!("  (file not found at {})", file_path.display());
                    failed_reads += 1;
                }
            }
        }
    }

    // Hint when files couldn't be resolved
    if failed_reads > 0 {
        println!();
        println!(
            "⚠ {} file(s) not found. Bundle files are relative to the source repo root.",
            failed_reads
        );
        println!(
            "  Try: bobbin bundle show {} --deep --repo-root /path/to/repo",
            bundle.name
        );
    }

    // Show beads with content (resolve via bd show --json)
    if !bundle.beads.is_empty() {
        println!();
        println!("=== Beads ({}) ===", bundle.beads.len());
        for bead_ref in &bundle.beads {
            println!();
            println!("--- bead:{} ---", bead_ref);
            // Try to resolve bead content via bd show --json
            let bead_id = bead_ref.split(':').last().unwrap_or(bead_ref);
            match std::process::Command::new("bd")
                .args(["show", bead_id, "--json"])
                .output()
            {
                Ok(output) if output.status.success() => {
                    let json_str = String::from_utf8_lossy(&output.stdout);
                    if let Ok(bead_json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        if let Some(title) = bead_json.get("title").and_then(|v| v.as_str()) {
                            println!("  Title: {}", title);
                        }
                        if let Some(status) = bead_json.get("status").and_then(|v| v.as_str()) {
                            println!("  Status: {}", status);
                        }
                        if let Some(priority) = bead_json.get("priority").and_then(|v| v.as_str()) {
                            println!("  Priority: {}", priority);
                        }
                        if let Some(desc) = bead_json.get("description").and_then(|v| v.as_str()) {
                            if !desc.is_empty() {
                                println!();
                                for line in desc.lines().take(30) {
                                    println!("  {}", line);
                                }
                                let total_lines = desc.lines().count();
                                if total_lines > 30 {
                                    println!("  ... ({} more lines)", total_lines - 30);
                                }
                            }
                        }
                    } else {
                        // Fallback: print raw output
                        print!("{}", json_str);
                    }
                }
                _ => {
                    println!("  (could not resolve — run `bd show {}` manually)", bead_id);
                }
            }
        }
    }

    // Show includes (expand included bundles at L1)
    if !bundle.includes.is_empty() {
        println!();
        println!("=== Included Bundles ===");
        for inc in &bundle.includes {
            if let Some(inc_bundle) = all_bundles.iter().find(|b| b.name == *inc) {
                println!();
                println!("--- included: {} — \"{}\" ---", inc, inc_bundle.description);
                // Show included bundle at L1 level (not recursive L2)
                for ref_str in &inc_bundle.refs {
                    if let Some(parsed) = BundleRef::parse(ref_str) {
                        println!("  {}", parsed.display_l0());
                    }
                }
                for f in &inc_bundle.files {
                    println!("  {}", f);
                }
            }
        }
    }

    Ok(())
}

// === CRUD Commands (Phase 4) ===

/// Resolve which tags.toml to write to: local .bobbin/tags.toml or global ~/.config/bobbin/tags.toml.
fn resolve_tags_path(repo_root: &std::path::Path, global: bool) -> std::path::PathBuf {
    if global {
        Config::global_config_dir()
            .map(|d| d.join("tags.toml"))
            .unwrap_or_else(|| TagsConfig::tags_path(repo_root))
    } else {
        // Walk up to find existing tags.toml, or default to given path for new files
        let effective_root = find_tags_root(repo_root).unwrap_or_else(|| repo_root.to_path_buf());
        TagsConfig::tags_path(&effective_root)
    }
}

/// Load the tags config from the appropriate file (local or global), returning the path used.
fn load_tags_for_write(
    repo_root: &std::path::Path,
    global: bool,
) -> (TagsConfig, std::path::PathBuf) {
    let path = resolve_tags_path(repo_root, global);
    let config = if path.exists() {
        TagsConfig::load_or_default(&path)
    } else {
        // Check if global has bundles when local doesn't exist
        if !global {
            if let Some(global_dir) = Config::global_config_dir() {
                let global_path = global_dir.join("tags.toml");
                if global_path.exists() {
                    let gc = TagsConfig::load_or_default(&global_path);
                    if !gc.bundles.is_empty() {
                        eprintln!(
                            "hint: bundles found in global config ({}). Use --global to modify those.",
                            global_path.display()
                        );
                    }
                }
            }
        }
        TagsConfig::default()
    };
    (config, path)
}

/// Format a BundleConfig as a TOML `[[bundles]]` entry for appending.
fn format_bundle_toml(bundle: &BundleConfig) -> String {
    let mut lines = vec!["[[bundles]]".to_string()];
    lines.push(format!("name = {:?}", bundle.name));
    lines.push(format!("description = {:?}", bundle.description));

    if let Some(ref slug) = bundle.slug {
        lines.push(format!("slug = {:?}", slug));
    }
    if !bundle.keywords.is_empty() {
        lines.push(format_toml_string_array("keywords", &bundle.keywords));
    }
    if !bundle.tags.is_empty() {
        lines.push(format_toml_string_array("tags", &bundle.tags));
    }
    if !bundle.files.is_empty() {
        lines.push(format_toml_string_array("files", &bundle.files));
    }
    if !bundle.refs.is_empty() {
        lines.push(format_toml_ref_array("refs", &bundle.refs));
    }
    if !bundle.docs.is_empty() {
        lines.push(format_toml_string_array("docs", &bundle.docs));
    }
    if !bundle.beads.is_empty() {
        lines.push(format_toml_string_array("beads", &bundle.beads));
    }
    if !bundle.includes.is_empty() {
        lines.push(format_toml_string_array("includes", &bundle.includes));
    }
    if !bundle.implements.is_empty() {
        lines.push(format_toml_string_array("implements", &bundle.implements));
    }
    if !bundle.depends_on.is_empty() {
        lines.push(format_toml_string_array("depends_on", &bundle.depends_on));
    }
    if !bundle.tests.is_empty() {
        lines.push(format_toml_string_array("tests", &bundle.tests));
    }
    if !bundle.repos.is_empty() {
        lines.push(format_toml_string_array("repos", &bundle.repos));
    }

    lines.join("\n")
}

/// Format a TOML string array inline or multiline depending on length.
fn format_toml_string_array(key: &str, values: &[String]) -> String {
    let inline = format!(
        "{} = [{}]",
        key,
        values
            .iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if inline.len() <= 100 {
        inline
    } else {
        // Multiline
        let mut s = format!("{} = [\n", key);
        for v in values {
            s.push_str(&format!("    {:?},\n", v));
        }
        s.push(']');
        s
    }
}

/// Format refs as multiline TOML array (refs tend to be long).
fn format_toml_ref_array(key: &str, values: &[String]) -> String {
    if values.len() == 1 {
        format!("{} = [{:?}]", key, values[0])
    } else {
        let mut s = format!("{} = [\n", key);
        for v in values {
            s.push_str(&format!("    {:?},\n", v));
        }
        s.push(']');
        s
    }
}

async fn run_create(path: PathBuf, args: CreateArgs, output: OutputConfig) -> Result<()> {
    let repo_root = path.canonicalize().unwrap_or(path);
    let tags_path = resolve_tags_path(&repo_root, args.global);

    // Check for duplicate
    if tags_path.exists() {
        let config = TagsConfig::load_or_default(&tags_path);
        if config.find_bundle(&args.name).is_some() {
            bail!("Bundle '{}' already exists in {}", args.name, tags_path.display());
        }
    }

    // Validate name
    if args.name.is_empty() || args.name.starts_with('/') || args.name.ends_with('/') {
        bail!("Invalid bundle name '{}': must not be empty or start/end with '/'", args.name);
    }

    let description = args
        .description
        .unwrap_or_else(|| format!("Bundle: {}", args.name));

    let bundle = BundleConfig {
        name: args.name.clone(),
        description: description.clone(),
        keywords: args.keywords,
        tags: args.tags,
        files: args.files,
        refs: args.refs,
        docs: args.docs,
        beads: args.beads,
        includes: args.includes,
        implements: Vec::new(),
        depends_on: Vec::new(),
        tests: Vec::new(),
        repos: args.repos,
        slug: args.slug,
    };

    // Append to file (preserves existing content and comments)
    let toml_entry = format_bundle_toml(&bundle);
    let mut content = if tags_path.exists() {
        std::fs::read_to_string(&tags_path)?
    } else {
        if let Some(parent) = tags_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        String::new()
    };

    // Ensure trailing newline before appending
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&toml_entry);
    content.push('\n');

    std::fs::write(&tags_path, &content)?;

    if output.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "created": args.name,
                "slug": bundle.slug(),
                "path": tags_path.display().to_string(),
            }))?
        );
    } else {
        println!("✓ Created bundle '{}' (slug: {})", args.name, bundle.slug());
        println!("  → {}", tags_path.display());
        println!("  → `bobbin bundle show {}`", args.name);
    }

    Ok(())
}

async fn run_add(path: PathBuf, args: AddArgs, _output: OutputConfig) -> Result<()> {
    let repo_root = path.canonicalize().unwrap_or(path);
    let (mut config, tags_path) = load_tags_for_write(&repo_root, args.global);

    let name = resolve_bundle_name(&args.name, &config.bundles);
    let bundle = config
        .bundles
        .iter_mut()
        .find(|b| b.name == name)
        .ok_or_else(|| anyhow::anyhow!("Bundle '{}' not found", name))?;

    let mut added = Vec::new();

    // Helper: normalize absolute paths to repo-relative.
    // If the path is absolute and starts with repo_root, strip the prefix.
    let normalize_path = |p: &str| -> String {
        let path = std::path::Path::new(p);
        if path.is_absolute() {
            if let Ok(rel) = path.strip_prefix(&repo_root) {
                let normalized = rel.to_string_lossy().to_string();
                eprintln!(
                    "note: normalized absolute path to repo-relative: {} → {}",
                    p, normalized
                );
                return normalized;
            }
            eprintln!(
                "warning: absolute path '{}' is outside repo root ({}), storing as-is",
                p,
                repo_root.display()
            );
        }
        p.to_string()
    };

    for f in &args.files {
        let normalized = normalize_path(f);
        if !bundle.files.contains(&normalized) {
            bundle.files.push(normalized.clone());
            added.push(format!("file:{}", normalized));
        }
    }
    for r in &args.refs {
        // For refs, normalize the file portion before the :: or # delimiter
        let normalized = normalize_ref_path(r, &repo_root);
        if !bundle.refs.contains(&normalized) {
            // Validate ref syntax
            if BundleRef::parse(&normalized).is_none() {
                eprintln!("warning: '{}' doesn't match ref syntax (file::symbol or file#heading), adding anyway", normalized);
            }
            bundle.refs.push(normalized.clone());
            added.push(format!("ref:{}", normalized));
        }
    }
    for d in &args.docs {
        let normalized = normalize_path(d);
        if !bundle.docs.contains(&normalized) {
            bundle.docs.push(normalized.clone());
            added.push(format!("doc:{}", normalized));
        }
    }
    for k in &args.keywords {
        if !bundle.keywords.contains(k) {
            bundle.keywords.push(k.clone());
            added.push(format!("keyword:{}", k));
        }
    }
    for t in &args.tags {
        if !bundle.tags.contains(t) {
            bundle.tags.push(t.clone());
            added.push(format!("tag:{}", t));
        }
    }
    for i in &args.includes {
        if !bundle.includes.contains(i) {
            bundle.includes.push(i.clone());
            added.push(format!("include:{}", i));
        }
    }
    for b in &args.beads {
        if !bundle.beads.contains(b) {
            bundle.beads.push(b.clone());
            added.push(format!("bead:{}", b));
        }
    }

    if added.is_empty() {
        println!("Nothing to add (all members already present)");
        return Ok(());
    }

    config.save(&tags_path)?;
    println!("✓ Added {} member(s) to '{}':", added.len(), name);
    for a in &added {
        println!("  + {}", a);
    }

    Ok(())
}

async fn run_remove(path: PathBuf, args: RemoveArgs, _output: OutputConfig) -> Result<()> {
    let repo_root = path.canonicalize().unwrap_or(path);
    let (mut config, tags_path) = load_tags_for_write(&repo_root, args.global);

    let name = resolve_bundle_name(&args.name, &config.bundles);

    if args.all {
        let before = config.bundles.len();
        config.bundles.retain(|b| b.name != name);
        if config.bundles.len() == before {
            bail!("Bundle '{}' not found", name);
        }
        config.save(&tags_path)?;
        println!("✓ Removed bundle '{}'", name);
        return Ok(());
    }

    let bundle = config
        .bundles
        .iter_mut()
        .find(|b| b.name == name)
        .ok_or_else(|| anyhow::anyhow!("Bundle '{}' not found", name))?;

    let mut removed = Vec::new();

    for f in &args.files {
        if let Some(pos) = bundle.files.iter().position(|x| x == f) {
            bundle.files.remove(pos);
            removed.push(format!("file:{}", f));
        }
    }
    for r in &args.refs {
        if let Some(pos) = bundle.refs.iter().position(|x| x == r) {
            bundle.refs.remove(pos);
            removed.push(format!("ref:{}", r));
        }
    }
    for d in &args.docs {
        if let Some(pos) = bundle.docs.iter().position(|x| x == d) {
            bundle.docs.remove(pos);
            removed.push(format!("doc:{}", d));
        }
    }
    for k in &args.keywords {
        if let Some(pos) = bundle.keywords.iter().position(|x| x == k) {
            bundle.keywords.remove(pos);
            removed.push(format!("keyword:{}", k));
        }
    }
    for t in &args.tags {
        if let Some(pos) = bundle.tags.iter().position(|x| x == t) {
            bundle.tags.remove(pos);
            removed.push(format!("tag:{}", t));
        }
    }
    for i in &args.includes {
        if let Some(pos) = bundle.includes.iter().position(|x| x == i) {
            bundle.includes.remove(pos);
            removed.push(format!("include:{}", i));
        }
    }
    for b in &args.beads {
        if let Some(pos) = bundle.beads.iter().position(|x| x == b) {
            bundle.beads.remove(pos);
            removed.push(format!("bead:{}", b));
        }
    }

    if removed.is_empty() {
        println!("Nothing to remove (no matching members found)");
        return Ok(());
    }

    config.save(&tags_path)?;
    println!("✓ Removed {} member(s) from '{}':", removed.len(), name);
    for r in &removed {
        println!("  - {}", r);
    }

    Ok(())
}

async fn run_check(path: PathBuf, args: CheckArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args.repo_root.unwrap_or_else(|| path.canonicalize().unwrap_or(path));
    let config = load_tags_with_bundles(&repo_root);

    let bundles_to_check: Vec<&BundleConfig> = if let Some(ref name) = args.name {
        match config.find_bundle(name) {
            Some(b) => vec![b],
            None => bail!("Bundle '{}' not found", name),
        }
    } else {
        config.bundles.iter().collect()
    };

    if bundles_to_check.is_empty() {
        println!("No bundles found.");
        return Ok(());
    }

    let mut total_issues = 0usize;
    let mut total_refs = 0usize;
    let mut total_files = 0usize;
    let mut bundles_healthy = 0usize;
    let mut bundles_stale = 0usize;

    for bundle in &bundles_to_check {
        let mut issues: Vec<String> = Vec::new();

        // Check files exist
        for f in &bundle.files {
            total_files += 1;
            let file_path = if f.contains(':') {
                // cross-repo ref like "repo:path" — skip, can't validate locally
                continue;
            } else {
                repo_root.join(f)
            };
            if !file_path.exists() {
                issues.push(format!("  ✗ file missing: {}", f));
            }
        }

        // Check docs exist
        for d in &bundle.docs {
            total_files += 1;
            let doc_path = repo_root.join(d);
            if !doc_path.exists() {
                issues.push(format!("  ✗ doc missing: {}", d));
            }
        }

        // Check refs resolve
        for ref_str in &bundle.refs {
            total_refs += 1;
            if let Some(parsed) = BundleRef::parse(ref_str) {
                if parsed.file.contains(':') {
                    continue; // cross-repo, skip
                }
                let file_path = repo_root.join(&parsed.file);
                if !file_path.exists() {
                    issues.push(format!("  ✗ ref file missing: {}", ref_str));
                } else if let RefTarget::Symbol(ref sym) = parsed.target {
                    // Check symbol exists in the file
                    if let Ok(content) = std::fs::read_to_string(&file_path) {
                        if !content.contains(sym.as_str()) {
                            issues.push(format!("  ⚠ symbol not found: {} (in {})", sym, parsed.file));
                        }
                    }
                } else if let RefTarget::Heading(ref heading) = parsed.target {
                    if let Ok(content) = std::fs::read_to_string(&file_path) {
                        let heading_pattern = format!("# {}", heading);
                        if !content.lines().any(|l| l.trim_start_matches('#').trim().starts_with(heading.as_str())) {
                            issues.push(format!("  ⚠ heading not found: {} (in {})", heading, parsed.file));
                        }
                        let _ = heading_pattern; // used for clarity
                    }
                }
            } else {
                issues.push(format!("  ⚠ unparseable ref: {}", ref_str));
            }
        }

        // Check beads resolve
        for bead_ref in &bundle.beads {
            let bead_id = bead_ref.split(':').last().unwrap_or(bead_ref);
            match std::process::Command::new("bd")
                .args(["show", bead_id, "--json"])
                .output()
            {
                Ok(output) if output.status.success() => {
                    // Bead resolves OK
                }
                _ => {
                    issues.push(format!("  ⚠ bead not found: {}", bead_ref));
                }
            }
        }

        // Check includes exist
        for inc in &bundle.includes {
            if config.find_bundle(inc).is_none() {
                issues.push(format!("  ✗ included bundle not found: {}", inc));
            }
        }

        if issues.is_empty() {
            bundles_healthy += 1;
        } else {
            bundles_stale += 1;
            total_issues += issues.len();
            println!("⚠ {} — \"{}\"", bundle.name, bundle.description);
            for issue in &issues {
                println!("{}", issue);
            }
            println!();
        }
    }

    // Summary
    if output.json {
        println!(
            "{{\"bundles_checked\":{},\"healthy\":{},\"stale\":{},\"issues\":{},\"refs_checked\":{},\"files_checked\":{}}}",
            bundles_to_check.len(), bundles_healthy, bundles_stale, total_issues, total_refs, total_files
        );
    } else {
        println!("Bundle health: {} checked, {} healthy, {} with issues ({} total issues)",
            bundles_to_check.len(), bundles_healthy, bundles_stale, total_issues);
        println!("  Refs checked: {}, Files checked: {}", total_refs, total_files);
        if bundles_stale == 0 {
            println!("  ✓ All bundles healthy");
        }
    }

    Ok(())
}

async fn run_stats(path: PathBuf, args: StatsArgs, output: OutputConfig) -> Result<()> {
    let repo_root = path.canonicalize().unwrap_or(path);
    let config = load_tags_with_bundles(&repo_root);

    let bundles_to_check: Vec<&BundleConfig> = if let Some(ref name) = args.name {
        match config.find_bundle(name) {
            Some(b) => vec![b],
            None => bail!("Bundle '{}' not found", name),
        }
    } else {
        config.bundles.iter().collect()
    };

    if bundles_to_check.is_empty() {
        println!("No bundles found.");
        return Ok(());
    }

    // For each bundle, query bd for beads with b:<slug> label
    let mut stats: Vec<(String, String, usize, usize, usize)> = Vec::new(); // (name, slug, open, closed, total)

    for bundle in &bundles_to_check {
        let slug = bundle.slug();
        let label = format!("b:{}", slug);

        // Try bd list with label filter (best-effort — bd might not be available)
        let result = std::process::Command::new("bd")
            .args(["list", "--json", "--label", &label, "--limit", "0", "--flat"])
            .output();

        match result {
            Ok(output_cmd) if output_cmd.status.success() => {
                let stdout = String::from_utf8_lossy(&output_cmd.stdout);
                // Parse JSON array of issues
                if let Ok(issues) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                    let total = issues.len();
                    let open = issues.iter().filter(|i| {
                        let s = i.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        s == "open" || s == "in_progress"
                    }).count();
                    let closed = issues.iter().filter(|i| {
                        i.get("status").and_then(|v| v.as_str()) == Some("closed")
                    }).count();
                    stats.push((bundle.name.clone(), slug, open, closed, total));
                } else {
                    stats.push((bundle.name.clone(), slug, 0, 0, 0));
                }
            }
            _ => {
                // bd not available or failed
                stats.push((bundle.name.clone(), slug, 0, 0, 0));
            }
        }
    }

    if output.json {
        let json_stats: Vec<serde_json::Value> = stats.iter().map(|(name, slug, open, closed, total)| {
            serde_json::json!({
                "bundle": name,
                "slug": slug,
                "label": format!("b:{}", slug),
                "open": open,
                "closed": closed,
                "total": total,
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&json_stats)?);
    } else {
        println!("Bundle usage (beads with b:<slug> labels):\n");
        let mut any_beads = false;
        for (name, slug, open, closed, total) in &stats {
            if *total > 0 {
                any_beads = true;
                println!("  {} (b:{}) — {} total ({} open, {} closed)", name, slug, total, open, closed);
            }
        }
        if !any_beads {
            println!("  No beads found with b:<slug> labels.");
            println!();
            println!("  Label beads with bundle slugs to track work:");
            println!("    bd new -t task \"description\" -l b:{}", stats.first().map(|s| s.1.as_str()).unwrap_or("context"));
        }
    }

    Ok(())
}

async fn run_suggest(path: PathBuf, args: SuggestArgs, output: OutputConfig) -> Result<()> {
    let repo_root = path.canonicalize().unwrap_or(path);
    let config = load_tags_with_bundles(&repo_root);

    // Collect all files already in bundles
    let mut bundled_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    for bundle in &config.bundles {
        for f in bundle.member_files() {
            bundled_files.insert(f);
        }
    }

    // Load coupling data from local index store
    let index_path = repo_root.join(".bobbin").join("index.db");
    if !index_path.exists() {
        bail!("No index.db found at {:?}. Run `bobbin index` first.", index_path);
    }
    let store = crate::storage::sqlite::MetadataStore::open(&index_path)?;
    let edges = store.all_coupling(args.threshold, 5000)?;

    if edges.is_empty() {
        println!("No coupling data found above threshold {}. Try lowering --threshold.", args.threshold);
        return Ok(());
    }

    // Build adjacency graph (union-find for connected components)
    let mut adj: HashMap<String, Vec<(String, f32)>> = HashMap::new();
    for edge in &edges {
        adj.entry(edge.file_a.clone()).or_default().push((edge.file_b.clone(), edge.score));
        adj.entry(edge.file_b.clone()).or_default().push((edge.file_a.clone(), edge.score));
    }

    // Find connected components via BFS
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut clusters: Vec<Vec<String>> = Vec::new();

    for file in adj.keys() {
        if visited.contains(file) {
            continue;
        }
        let mut component = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(file.clone());
        visited.insert(file.clone());
        while let Some(current) = queue.pop_front() {
            component.push(current.clone());
            if let Some(neighbors) = adj.get(&current) {
                for (neighbor, _) in neighbors {
                    if !visited.contains(neighbor) {
                        visited.insert(neighbor.clone());
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
        if component.len() >= args.min_size {
            clusters.push(component);
        }
    }

    // Sort clusters by size (largest first)
    clusters.sort_by(|a, b| b.len().cmp(&a.len()));

    if clusters.is_empty() {
        println!("No file clusters found with >= {} members above coupling threshold {}.",
            args.min_size, args.threshold);
        return Ok(());
    }

    // For each cluster, check how many files are already bundled
    println!("Suggested bundles from coupling analysis:\n");
    let mut suggestion_count = 0;

    for (i, cluster) in clusters.iter().enumerate() {
        let unbundled: Vec<&String> = cluster.iter().filter(|f| !bundled_files.contains(*f)).collect();
        let bundled_count = cluster.len() - unbundled.len();

        // Skip clusters where most files are already bundled
        if unbundled.is_empty() {
            continue;
        }

        suggestion_count += 1;

        // Try to derive a name from common path prefix
        let common_prefix = common_path_prefix(&cluster.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let suggested_name = if common_prefix.is_empty() {
            format!("cluster-{}", i + 1)
        } else {
            common_prefix.trim_end_matches('/').replace('/', "-")
        };

        // Calculate average coupling score within cluster
        let mut total_score = 0.0f32;
        let mut edge_count = 0;
        for edge in &edges {
            if cluster.contains(&edge.file_a) && cluster.contains(&edge.file_b) {
                total_score += edge.score;
                edge_count += 1;
            }
        }
        let avg_score = if edge_count > 0 { total_score / edge_count as f32 } else { 0.0 };

        if output.json {
            println!("  {{\"name\":\"{}\",\"files\":{},\"unbundled\":{},\"avg_coupling\":{:.2}}}",
                suggested_name, cluster.len(), unbundled.len(), avg_score);
        } else {
            println!("{}. {} ({} files, {} unbundled, avg coupling: {:.2})",
                suggestion_count, suggested_name, cluster.len(), unbundled.len(), avg_score);
            if bundled_count > 0 {
                println!("   Already bundled: {} files", bundled_count);
            }
            for f in &unbundled[..unbundled.len().min(10)] {
                println!("   - {}", f);
            }
            if unbundled.len() > 10 {
                println!("   ... and {} more", unbundled.len() - 10);
            }
            println!();
        }
    }

    if suggestion_count == 0 {
        println!("All coupled file clusters are already covered by existing bundles.");
    } else {
        println!("Found {} potential bundle(s). Create with:", suggestion_count);
        println!("  bobbin bundle create \"<name>\" --global -f \"<file1>,<file2>,...\"");
    }

    Ok(())
}

/// Find the common path prefix of a set of file paths.
fn common_path_prefix(paths: &[&str]) -> String {
    if paths.is_empty() {
        return String::new();
    }
    let first = paths[0];
    let mut prefix_len = 0;
    for (i, c) in first.char_indices() {
        if paths.iter().all(|p| p.get(..=i).map(|s| s.ends_with(c)).unwrap_or(false)) {
            if c == '/' {
                prefix_len = i + 1;
            }
        } else {
            break;
        }
    }
    first[..prefix_len].to_string()
}

/// Try to count symbols in a file (best-effort, returns a hint string).
async fn count_symbols_in_file(repo_root: &std::path::Path, file: &str) -> String {
    let file_path = repo_root.join(file);
    match std::fs::read_to_string(&file_path) {
        Ok(content) => {
            let lines = content.lines().count();
            format!("({} lines)", lines)
        }
        Err(_) => "(file not found)".to_string(),
    }
}

/// Print content with line numbers, optionally starting at an offset and limiting lines.
fn print_with_line_numbers(content: &str, start_line: Option<usize>, max_lines: Option<usize>) {
    let start = start_line.unwrap_or(0);
    let lines: Vec<&str> = content.lines().collect();
    let end = max_lines
        .map(|m| (start + m).min(lines.len()))
        .unwrap_or(lines.len());

    for (i, line) in lines[start..end].iter().enumerate() {
        println!("{:>4} {}", start + i + 1, line);
    }

    if end < lines.len() {
        println!("  ... ({} more lines)", lines.len() - end);
    }
}

/// Find a symbol (function, struct, impl) in file content and print its body.
fn print_symbol_from_content(content: &str, symbol_name: &str, file_path: &str) {
    let lines: Vec<&str> = content.lines().collect();

    // Support glob patterns in symbol names
    let is_glob = symbol_name.contains('*') || symbol_name.contains('?');

    let mut found = false;
    for (i, line) in lines.iter().enumerate() {
        let matches = if is_glob {
            if let Ok(pat) = glob::Pattern::new(symbol_name) {
                // Extract the identifier from the line and check against pattern
                extract_symbol_name_from_line(line)
                    .map(|name| pat.matches(&name))
                    .unwrap_or(false)
            } else {
                false
            }
        } else {
            line.contains(symbol_name)
                && (line.contains(&format!("fn {}", symbol_name))
                    || line.contains(&format!("struct {}", symbol_name))
                    || line.contains(&format!("enum {}", symbol_name))
                    || line.contains(&format!("trait {}", symbol_name))
                    || line.contains(&format!("impl {}", symbol_name))
                    || line.contains(&format!("type {}", symbol_name))
                    || line.contains(&format!("const {}", symbol_name))
                    || line.contains(&format!("static {}", symbol_name))
                    || line.contains(&format!("mod {}", symbol_name))
                    || line.contains(&format!("def {}", symbol_name))
                    || line.contains(&format!("class {}", symbol_name))
                    || line.contains(&format!("func {}", symbol_name)))
        };

        if matches {
            found = true;
            // Find the end of the symbol body (brace matching for Rust/Go/etc)
            let end = find_block_end(&lines, i);
            let block_lines = end - i;
            println!("  {}:{}-{}", file_path, i + 1, end);
            for j in i..end.min(i + 50) {
                println!("{:>4} {}", j + 1, lines[j]);
            }
            if block_lines > 50 {
                println!("  ... ({} more lines in this symbol)", block_lines - 50);
            }
            if !is_glob {
                break; // For exact names, just show the first match
            }
            println!();
        }
    }

    if !found {
        println!("  (symbol '{}' not found in {})", symbol_name, file_path);
    }
}

/// Extract a symbol name from a code line (the identifier after fn/struct/etc keywords).
fn extract_symbol_name_from_line(line: &str) -> Option<String> {
    let keywords = ["fn ", "struct ", "enum ", "trait ", "impl ", "type ", "const ", "static ",
                     "mod ", "def ", "class ", "func "];
    for kw in &keywords {
        if let Some(idx) = line.find(kw) {
            let after = &line[idx + kw.len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Find the end of a code block starting at `start_line` using brace matching.
fn find_block_end(lines: &[&str], start_line: usize) -> usize {
    let mut depth = 0i32;
    let mut found_open = false;

    for i in start_line..lines.len() {
        for ch in lines[i].chars() {
            if ch == '{' {
                depth += 1;
                found_open = true;
            } else if ch == '}' {
                depth -= 1;
                if found_open && depth == 0 {
                    return i + 1;
                }
            }
        }
    }

    // No braces found — likely a single-line definition or Python-style
    // Return up to next blank line or 20 lines
    for i in (start_line + 1)..lines.len().min(start_line + 20) {
        if lines[i].trim().is_empty() {
            return i;
        }
    }
    (start_line + 20).min(lines.len())
}

/// Print a markdown section starting at a heading.
fn print_heading_section(content: &str, target_heading: &str) {
    let lines: Vec<&str> = content.lines().collect();
    let mut in_section = false;
    let mut section_level = 0;
    let mut count = 0;

    for (i, line) in lines.iter().enumerate() {
        if line.starts_with('#') {
            let level = line.chars().take_while(|c| *c == '#').count();
            let heading_text = line.trim_start_matches('#').trim();

            if heading_text.eq_ignore_ascii_case(target_heading) {
                in_section = true;
                section_level = level;
                println!("{:>4} {}", i + 1, line);
                count += 1;
                continue;
            }

            if in_section && level <= section_level {
                // Hit a same-level or higher heading — end of section
                break;
            }
        }

        if in_section {
            println!("{:>4} {}", i + 1, line);
            count += 1;
            if count > 100 {
                println!("  ... (section truncated at 100 lines)");
                break;
            }
        }
    }

    if !in_section {
        println!("  (heading '{}' not found)", target_heading);
    }
}

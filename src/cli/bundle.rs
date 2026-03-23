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
    /// Add a member (file, ref, doc, keyword, tag) to a bundle
    Add(AddArgs),
    /// Remove a member from a bundle
    Remove(RemoveArgs),
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
    /// Remove the entire bundle
    #[arg(long)]
    all: bool,
    /// Write to global config
    #[arg(long)]
    global: bool,
}

pub async fn run(args: BundleArgs, output: OutputConfig) -> Result<()> {
    match args.command {
        BundleCommands::List(list_args) => run_list(args.path, list_args, output).await,
        BundleCommands::Show(show_args) => run_show(args.path, show_args, output).await,
        BundleCommands::Create(create_args) => run_create(args.path, create_args, output).await,
        BundleCommands::Add(add_args) => run_add(args.path, add_args, output).await,
        BundleCommands::Remove(remove_args) => run_remove(args.path, remove_args, output).await,
    }
}

/// Load tags config with bundle definitions, checking local .bobbin/ first, then global config.
fn load_tags_with_bundles(repo_root: &std::path::Path) -> TagsConfig {
    let local_path = TagsConfig::tags_path(repo_root);
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
    let repo_root = path.canonicalize().unwrap_or(path);
    let config = load_tags_with_bundles(&repo_root);

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
            "includes": bundle.includes,
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
    println!();
    println!(
        "   {} files, {} refs, {} tag memberships",
        files.len(),
        ref_count,
        tag_count
    );
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
                    Err(e) => {
                        println!("  (unable to read: {})", e);
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
                Err(e) => {
                    println!("  (unable to read: {})", e);
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
        TagsConfig::tags_path(repo_root)
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
    if !bundle.includes.is_empty() {
        lines.push(format_toml_string_array("includes", &bundle.includes));
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
        includes: args.includes,
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

    for f in &args.files {
        if !bundle.files.contains(f) {
            bundle.files.push(f.clone());
            added.push(format!("file:{}", f));
        }
    }
    for r in &args.refs {
        if !bundle.refs.contains(r) {
            // Validate ref syntax
            if BundleRef::parse(r).is_none() {
                eprintln!("warning: '{}' doesn't match ref syntax (file::symbol or file#heading), adding anyway", r);
            }
            bundle.refs.push(r.clone());
            added.push(format!("ref:{}", r));
        }
    }
    for d in &args.docs {
        if !bundle.docs.contains(d) {
            bundle.docs.push(d.clone());
            added.push(format!("doc:{}", d));
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

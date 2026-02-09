use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::OutputConfig;
use crate::config::Config;

#[derive(Args)]
pub struct TourArgs {
    /// Run tour for a specific feature only (e.g. 'search', 'hooks')
    #[arg(value_name = "FEATURE")]
    feature: Option<String>,

    /// Directory to tour (defaults to current directory)
    #[arg(long, default_value = ".")]
    path: PathBuf,

    /// Skip interactive pauses (run all steps continuously)
    #[arg(long)]
    non_interactive: bool,

    /// List available tour steps without running them
    #[arg(long)]
    list: bool,
}

/// A single step in the guided tour.
///
/// IMPORTANT: When adding a new bobbin command, you MUST add a corresponding
/// tour step to the `tour_steps()` function. This ensures all features are
/// discoverable through `bobbin tour`.
struct TourStep {
    /// Short identifier for per-feature filtering (e.g. "search", "hooks")
    id: &'static str,
    /// Display title shown to the user
    title: &'static str,
    /// Explanation shown before running the command
    intro: &'static str,
    /// Function that builds the command for this step.
    /// Returns None to skip (e.g. when no suitable example file exists).
    build: fn(&Path) -> Option<StepCommand>,
}

struct StepCommand {
    /// Arguments to pass to the bobbin binary
    args: Vec<String>,
    /// Explanation shown after the command output
    explanation: &'static str,
}

/// Registry of all tour steps. The order here defines the tour progression.
///
/// When adding a new bobbin subcommand, add a `TourStep` here so users can
/// discover it via `bobbin tour`. Each step should demonstrate the command
/// against the user's actual repository.
fn tour_steps() -> Vec<TourStep> {
    vec![
        TourStep {
            id: "status",
            title: "Index Status",
            intro: "Let's start by checking your bobbin index.\n\
                    This shows what's been indexed — files, chunks, languages.",
            build: |_| {
                Some(StepCommand {
                    args: vec!["status".into(), "--detailed".into()],
                    explanation: "The index stores code split into semantic chunks (functions, classes,\n\
                                 structs, etc.) using tree-sitter. Each chunk is embedded as a vector\n\
                                 with a local ML model — no data leaves your machine.",
                })
            },
        },
        TourStep {
            id: "search",
            title: "Semantic Search",
            intro: "Semantic search finds code by meaning, not just keywords.\n\
                    It understands that 'error handling' matches 'catch' and 'Result<T>'.",
            build: |_| {
                Some(StepCommand {
                    args: vec![
                        "search".into(),
                        "error handling".into(),
                        "-n".into(),
                        "5".into(),
                    ],
                    explanation: "Results are ranked by vector similarity to your query.\n\
                                 Hybrid mode (default) combines semantic + keyword search\n\
                                 using Reciprocal Rank Fusion for the best of both worlds.",
                })
            },
        },
        TourStep {
            id: "grep",
            title: "Keyword Search",
            intro: "For exact pattern matching, bobbin grep uses the full-text index.\n\
                    Familiar syntax, instant results over your entire codebase.",
            build: |_| {
                Some(StepCommand {
                    args: vec!["grep".into(), "TODO".into(), "-n".into(), "5".into()],
                    explanation: "Grep searches the FTS index — no file scanning needed.\n\
                                 Use -E for regex, -i for case-insensitive, -t for type filtering.\n\
                                 Combine with semantic search for comprehensive code discovery.",
                })
            },
        },
        TourStep {
            id: "context",
            title: "Context Assembly",
            intro: "Context assembly is bobbin's killer feature: it builds a focused\n\
                    code bundle for a task by combining search with coupling analysis.",
            build: |_| {
                Some(StepCommand {
                    args: vec![
                        "context".into(),
                        "understand the main entry point".into(),
                        "-b".into(),
                        "200".into(),
                        "-c".into(),
                        "preview".into(),
                    ],
                    explanation: "Context finds relevant code, then expands to coupled files\n\
                                 (files that frequently change together in git). The budget (-b)\n\
                                 controls output size — perfect for feeding to LLM prompts.",
                })
            },
        },
        TourStep {
            id: "related",
            title: "Related Files",
            intro: "Related files discovers co-change patterns from git history.\n\
                    Files that always change together reveal hidden architectural couplings.",
            build: |path| {
                let file = find_example_file(path)?;
                Some(StepCommand {
                    args: vec!["related".into(), file, "-n".into(), "5".into()],
                    explanation: "Co-change scores come from git history analysis.\n\
                                 High scores mean files almost always change together —\n\
                                 catching couplings that import graphs miss.",
                })
            },
        },
        TourStep {
            id: "deps",
            title: "Import Dependencies",
            intro: "Deps traces the import graph for any file.\n\
                    See what a file imports and what depends on it.",
            build: |path| {
                let file = find_example_file(path)?;
                Some(StepCommand {
                    args: vec!["deps".into(), file, "--both".into()],
                    explanation: "Import analysis uses tree-sitter to parse actual import\n\
                                 statements. The --both flag shows both directions:\n\
                                 what this file imports and what files import it.",
                })
            },
        },
        TourStep {
            id: "hotspots",
            title: "Code Hotspots",
            intro: "Hotspots identify code that's both frequently changed AND complex.\n\
                    These are your highest-risk files — where bugs most likely hide.",
            build: |_| {
                Some(StepCommand {
                    args: vec!["hotspots".into(), "-n".into(), "5".into()],
                    explanation: "Score = sqrt(churn x complexity). High churn alone is fine\n\
                                 (config files change often). High complexity alone is manageable\n\
                                 (stable algorithms). Both together? That's a hotspot.",
                })
            },
        },
        TourStep {
            id: "hooks",
            title: "Claude Code Hooks",
            intro: "Hooks integrate bobbin with Claude Code for automatic context injection.\n\
                    Every prompt you send gets enriched with relevant code — automatically.",
            build: |_| {
                Some(StepCommand {
                    args: vec!["hook".into(), "status".into()],
                    explanation: "When installed, hooks fire on every Claude Code prompt.\n\
                                 Bobbin searches your index for relevant code and injects\n\
                                 it as context. Install with: bobbin hook install",
                })
            },
        },
    ]
}

/// Find a suitable source file in the repo for demo steps that need a file path.
fn find_example_file(repo_root: &Path) -> Option<String> {
    let candidates = [
        "src/main.rs",
        "src/lib.rs",
        "src/index.ts",
        "src/main.ts",
        "src/app.ts",
        "src/index.js",
        "src/main.py",
        "main.go",
        "src/main.go",
        "src/App.tsx",
        "src/App.jsx",
    ];

    for candidate in &candidates {
        if repo_root.join(candidate).exists() {
            return Some(candidate.to_string());
        }
    }

    // Fallback: pick the first source file from git
    let output = Command::new("git")
        .args(["ls-files", "--cached"])
        .current_dir(repo_root)
        .output()
        .ok()?;

    let files = String::from_utf8_lossy(&output.stdout);
    for line in files.lines().take(200) {
        let ext = line.rsplit('.').next().unwrap_or("");
        if matches!(
            ext,
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "cpp" | "c"
        ) {
            return Some(line.to_string());
        }
    }

    None
}

pub async fn run(args: TourArgs, output: OutputConfig) -> Result<()> {
    let steps = tour_steps();

    // --list: show available steps without requiring initialization
    if args.list {
        if output.json {
            let json_steps: Vec<serde_json::Value> = steps
                .iter()
                .map(|s| serde_json::json!({ "id": s.id, "title": s.title }))
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_steps)?);
        } else {
            println!("{}", "Available tour steps:".bold());
            println!();
            for (i, step) in steps.iter().enumerate() {
                println!(
                    "  {}. {} ({})",
                    i + 1,
                    step.title.cyan(),
                    step.id.dimmed()
                );
            }
            println!();
            println!(
                "Run a specific step: {}",
                "bobbin tour <feature>".green()
            );
        }
        return Ok(());
    }

    // For actual tour execution, bobbin must be initialized
    let repo_root = args.path.canonicalize()?;
    let config_path = Config::config_path(&repo_root);
    if !config_path.exists() {
        bail!(
            "Bobbin not initialized in {}.\n\
             Run `bobbin init` first, then `bobbin index` to build the search index.",
            repo_root.display()
        );
    }

    // Filter to specific feature if requested
    let filtered: Vec<&TourStep> = if let Some(ref feature) = args.feature {
        let matching: Vec<_> = steps.iter().filter(|s| s.id == feature.as_str()).collect();
        if matching.is_empty() {
            let available: Vec<_> = steps.iter().map(|s| s.id).collect();
            bail!(
                "Unknown tour feature: '{}'\nAvailable: {}",
                feature,
                available.join(", ")
            );
        }
        matching
    } else {
        steps.iter().collect()
    };

    let total = filtered.len();
    let bobbin_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("bobbin"));

    if !output.quiet {
        println!();
        println!(
            "{}",
            "========================================================".cyan()
        );
        println!(
            "{}",
            "            Welcome to the Bobbin Tour!                 ".cyan().bold()
        );
        println!(
            "{}",
            "   A guided walkthrough of your code context engine     ".cyan()
        );
        println!(
            "{}",
            "========================================================".cyan()
        );
        println!();
        println!(
            "  Repository: {}",
            repo_root.display().to_string().green()
        );
        println!("  Steps:      {}", total.to_string().cyan());
        if let Some(ref feature) = args.feature {
            println!("  Feature:    {}", feature.cyan());
        }
        println!();
    }

    for (i, step) in filtered.iter().enumerate() {
        let step_num = i + 1;

        // Build the command for this step
        let cmd = match (step.build)(&repo_root) {
            Some(cmd) => cmd,
            None => {
                if !output.quiet {
                    println!(
                        "  {} Skipping {} (no suitable example file found)",
                        "->".dimmed(),
                        step.title,
                    );
                    println!();
                }
                continue;
            }
        };

        if !output.quiet {
            // Step header
            println!("{}", "------------------------------------------------------------".dimmed());
            println!(
                "  {} Step {}/{}: {}",
                ">".green(),
                step_num.to_string().bold(),
                total,
                step.title.cyan().bold(),
            );
            println!("{}", "------------------------------------------------------------".dimmed());
            println!();

            // Introduction
            for line in step.intro.lines() {
                println!("  {}", line);
            }
            println!();

            // Show the command being run
            let cmd_str = format!("bobbin {}", cmd.args.join(" "));
            println!("  {} {}", "$".dimmed(), cmd_str.green());
            println!();
        }

        // Run the actual command
        let result = Command::new(&bobbin_exe)
            .args(&cmd.args)
            .current_dir(&repo_root)
            .output();

        match result {
            Ok(cmd_output) => {
                let stdout = String::from_utf8_lossy(&cmd_output.stdout);
                let stderr = String::from_utf8_lossy(&cmd_output.stderr);

                if !output.quiet {
                    // Indent the command output
                    for line in stdout.lines() {
                        println!("  {} {}", "|".dimmed(), line);
                    }

                    if !cmd_output.status.success() && !stderr.is_empty() {
                        println!();
                        println!("  {} Command exited with non-zero status", "!".yellow());
                        for line in stderr.lines().take(5) {
                            println!("  {} {}", "|".dimmed(), line.dimmed());
                        }
                    }

                    println!();

                    // Explanation
                    println!("  {}", "What happened:".bold());
                    for line in cmd.explanation.lines() {
                        println!("    {}", line.dimmed());
                    }
                    println!();
                }
            }
            Err(e) => {
                if !output.quiet {
                    println!("  {} Command failed: {}", "!".red(), e);
                    println!();
                }
            }
        }

        // Interactive pause (skip for last step and non-interactive mode)
        if !args.non_interactive && !output.quiet && step_num < total {
            print!(
                "  {} ",
                "Press Enter to continue (q to quit)...".dimmed()
            );
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if input.trim().eq_ignore_ascii_case("q") {
                println!();
                println!(
                    "  Tour ended early. Resume any step with: {}",
                    "bobbin tour <feature>".green()
                );
                return Ok(());
            }
            println!();
        }
    }

    if !output.quiet {
        println!("{}", "------------------------------------------------------------".dimmed());
        println!();
        println!(
            "  {} {}",
            "Done!".green().bold(),
            "Tour complete.".bold()
        );
        println!();
        println!("  {}", "Next steps:".bold());
        println!(
            "    {}  Search your code semantically",
            "bobbin search <query>".cyan()
        );
        println!(
            "    {}  Get AI-ready context bundles",
            "bobbin context <task>".cyan()
        );
        println!(
            "    {}  Set up Claude Code integration",
            "bobbin hook install".cyan()
        );
        println!(
            "    {}  See all commands",
            "bobbin --help".cyan()
        );
        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tour_steps_have_unique_ids() {
        let steps = tour_steps();
        let mut ids: Vec<&str> = steps.iter().map(|s| s.id).collect();
        let original_len = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(
            ids.len(),
            original_len,
            "Tour step IDs must be unique"
        );
    }

    #[test]
    fn test_tour_steps_all_build() {
        let steps = tour_steps();
        let dummy_path = PathBuf::from("/nonexistent");
        for step in &steps {
            // Steps that need real files may return None, which is fine.
            // Steps that don't need files should always return Some.
            let result = (step.build)(dummy_path.as_path());
            match step.id {
                "related" | "deps" => {
                    // These need a real file, so None is expected
                }
                _ => {
                    assert!(
                        result.is_some(),
                        "Tour step '{}' returned None for a path-independent step",
                        step.id
                    );
                }
            }
        }
    }

    #[test]
    fn test_tour_step_count() {
        let steps = tour_steps();
        // Ensure we have the expected number of steps matching the spec:
        // status, search, grep, context, related, deps, hotspots, hooks
        assert_eq!(steps.len(), 8, "Expected 8 tour steps");
    }

    #[test]
    fn test_tour_step_order() {
        let steps = tour_steps();
        let ids: Vec<&str> = steps.iter().map(|s| s.id).collect();
        // Verify the progressive order from the spec
        assert_eq!(ids[0], "status");
        assert_eq!(ids[1], "search");
        // Other steps follow
        assert_eq!(ids[ids.len() - 1], "hooks");
    }

    #[test]
    fn test_find_example_file_with_main_rs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.rs"), "fn main() {}").unwrap();

        let result = find_example_file(dir.path());
        assert_eq!(result, Some("src/main.rs".to_string()));
    }

    #[test]
    fn test_find_example_file_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        // No source files, no git — returns None
        let result = find_example_file(dir.path());
        assert!(result.is_none());
    }
}

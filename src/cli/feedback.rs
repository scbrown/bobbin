use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use super::OutputConfig;
use crate::storage::feedback::{FeedbackInput, FeedbackQuery, InjectionRecord};

#[derive(Args)]
pub struct FeedbackArgs {
    #[command(subcommand)]
    command: FeedbackCommands,
}

#[derive(Subcommand)]
enum FeedbackCommands {
    /// Submit feedback on a bobbin injection
    Submit(SubmitArgs),

    /// List recent feedback
    List(ListArgs),

    /// Show details for a specific injection (with its feedback)
    Show(ShowArgs),

    /// Show aggregate feedback statistics
    Stats,
}

#[derive(Args)]
struct SubmitArgs {
    /// Injection ID to provide feedback on
    #[arg(long, short = 'i')]
    injection: String,

    /// Rating: useful, noise, or harmful
    #[arg(long, short = 'r')]
    rating: String,

    /// Reason for the rating
    #[arg(long)]
    reason: Option<String>,

    /// Comma-separated chunk IDs referenced
    #[arg(long)]
    chunks: Option<String>,
}

#[derive(Args)]
struct ListArgs {
    /// Filter by rating (useful, noise, harmful)
    #[arg(long)]
    rating: Option<String>,

    /// Filter by injection ID
    #[arg(long)]
    injection: Option<String>,

    /// Filter by agent
    #[arg(long)]
    agent: Option<String>,

    /// Maximum results
    #[arg(long, default_value = "20")]
    limit: u32,
}

#[derive(Args)]
struct ShowArgs {
    /// Injection ID to show
    injection_id: String,
}

pub async fn run(args: FeedbackArgs, output: OutputConfig) -> Result<()> {
    match args.command {
        FeedbackCommands::Submit(a) => submit(a, &output).await,
        FeedbackCommands::List(a) => list(a, &output).await,
        FeedbackCommands::Show(a) => show(a, &output).await,
        FeedbackCommands::Stats => stats(&output).await,
    }
}

async fn submit(args: SubmitArgs, output: &OutputConfig) -> Result<()> {
    // Validate rating
    match args.rating.as_str() {
        "useful" | "noise" | "harmful" => {}
        _ => anyhow::bail!("rating must be one of: useful, noise, harmful"),
    }

    let chunks_referenced = args.chunks.map(|c| {
        c.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
    });

    let agent = std::env::var("BD_ACTOR")
        .or_else(|_| std::env::var("GT_ROLE"))
        .unwrap_or_default();

    let input = FeedbackInput {
        injection_id: args.injection.clone(),
        agent: Some(agent),
        session_id: None,
        rating: args.rating.clone(),
        reason: args.reason.clone(),
        chunks_referenced,
    };

    if let Some(ref server) = output.server {
        // Remote: POST to server
        let url = format!("{}/feedback", server);
        let resp = reqwest::Client::new()
            .post(&url)
            .json(&input)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .context("Failed to submit feedback to server")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Server returned error: {}", body);
        }
    } else {
        // Local: write to feedback DB
        let repo_root = super::find_bobbin_root()
            .context("Bobbin not initialized. Run `bobbin init` first.")?;
        let fb_path = crate::config::Config::feedback_db_path(&repo_root);
        let store = crate::storage::FeedbackStore::open(&fb_path)
            .context("Failed to open feedback database")?;
        store.store_feedback(&input)?;
    }

    if output.json {
        println!(
            "{}",
            serde_json::json!({"ok": true, "injection_id": args.injection, "rating": args.rating})
        );
    } else {
        println!(
            "{} Feedback submitted: {} → {}",
            "✓".green(),
            args.injection,
            args.rating.bold()
        );
    }

    Ok(())
}

async fn list(args: ListArgs, output: &OutputConfig) -> Result<()> {
    let query = FeedbackQuery {
        injection_id: args.injection,
        rating: args.rating,
        agent: args.agent,
        limit: Some(args.limit),
        offset: None,
    };

    let records = if let Some(ref server) = output.server {
        let url = format!("{}/feedback", server);
        let mut params = Vec::new();
        if let Some(ref inj) = query.injection_id {
            params.push(("injection_id", inj.as_str()));
        }
        if let Some(ref r) = query.rating {
            params.push(("rating", r.as_str()));
        }
        if let Some(ref a) = query.agent {
            params.push(("agent", a.as_str()));
        }
        let limit_str = query.limit.unwrap_or(20).to_string();
        params.push(("limit", &limit_str));

        let resp = reqwest::Client::new()
            .get(&url)
            .query(&params)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .context("Failed to fetch feedback from server")?;

        resp.json::<Vec<crate::storage::feedback::FeedbackRecord>>()
            .await
            .context("Failed to parse feedback response")?
    } else {
        let repo_root = super::find_bobbin_root()
            .context("Bobbin not initialized. Run `bobbin init` first.")?;
        let fb_path = crate::config::Config::feedback_db_path(&repo_root);
        let store = crate::storage::FeedbackStore::open(&fb_path)
            .context("Failed to open feedback database")?;
        store.list_feedback(&query)?
    };

    if output.json {
        println!("{}", serde_json::to_string_pretty(&records)?);
    } else if records.is_empty() {
        println!("No feedback records found.");
    } else {
        for r in &records {
            let rating_colored = match r.rating.as_str() {
                "useful" => r.rating.green(),
                "noise" => r.rating.yellow(),
                "harmful" => r.rating.red(),
                _ => r.rating.normal(),
            };
            println!(
                "{} {} [{}] {} — {}",
                r.timestamp.dimmed(),
                r.injection_id,
                rating_colored,
                r.agent.dimmed(),
                r.reason
            );
        }
        println!("\n{} record(s)", records.len());
    }

    Ok(())
}

async fn show(args: ShowArgs, output: &OutputConfig) -> Result<()> {
    let (injection, feedback) = if let Some(ref server) = output.server {
        let inj_url = format!("{}/injections/{}", server, args.injection_id);
        let fb_url = format!(
            "{}/feedback?injection_id={}",
            server, args.injection_id
        );

        let client = reqwest::Client::new();
        let inj_resp = client
            .get(&inj_url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .context("Failed to fetch injection from server")?;

        let injection: Option<InjectionRecord> = if inj_resp.status().is_success() {
            Some(inj_resp.json().await?)
        } else {
            None
        };

        let fb_resp = client
            .get(&fb_url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .context("Failed to fetch feedback from server")?;

        let feedback: Vec<crate::storage::feedback::FeedbackRecord> =
            fb_resp.json().await.unwrap_or_default();

        (injection, feedback)
    } else {
        let repo_root = super::find_bobbin_root()
            .context("Bobbin not initialized. Run `bobbin init` first.")?;
        let fb_path = crate::config::Config::feedback_db_path(&repo_root);
        let store = crate::storage::FeedbackStore::open(&fb_path)
            .context("Failed to open feedback database")?;

        let injection = store.get_injection(&args.injection_id)?;
        let feedback = store.list_feedback(&FeedbackQuery {
            injection_id: Some(args.injection_id.clone()),
            ..Default::default()
        })?;
        (injection, feedback)
    };

    if output.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "injection": injection,
                "feedback": feedback,
            }))?
        );
    } else {
        match injection {
            Some(inj) => {
                println!("{}", format!("Injection: {}", inj.injection_id).bold());
                println!("  Time:    {}", inj.timestamp);
                println!("  Agent:   {}", inj.agent);
                println!("  Session: {}", inj.session_id);
                println!("  Query:   {}", inj.query);
                println!("  Files:   {} ({} chunks, {} budget lines)",
                    inj.files_returned.len(), inj.total_chunks, inj.budget_lines);
                for f in &inj.files_returned {
                    println!("    - {}", f);
                }
            }
            None => {
                println!("{}", format!("Injection {} not found", args.injection_id).red());
            }
        }

        if !feedback.is_empty() {
            println!("\n{}", "Feedback:".bold());
            for r in &feedback {
                let rating_colored = match r.rating.as_str() {
                    "useful" => r.rating.green(),
                    "noise" => r.rating.yellow(),
                    "harmful" => r.rating.red(),
                    _ => r.rating.normal(),
                };
                println!(
                    "  {} [{}] {} — {}",
                    r.timestamp.dimmed(),
                    rating_colored,
                    r.agent.dimmed(),
                    r.reason
                );
            }
        } else {
            println!("\nNo feedback for this injection.");
        }
    }

    Ok(())
}

async fn stats(output: &OutputConfig) -> Result<()> {
    let stats = if let Some(ref server) = output.server {
        let url = format!("{}/feedback/stats", server);
        let resp = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .context("Failed to fetch stats from server")?;
        resp.json::<crate::storage::feedback::FeedbackStats>()
            .await
            .context("Failed to parse stats response")?
    } else {
        let repo_root = super::find_bobbin_root()
            .context("Bobbin not initialized. Run `bobbin init` first.")?;
        let fb_path = crate::config::Config::feedback_db_path(&repo_root);
        let store = crate::storage::FeedbackStore::open(&fb_path)
            .context("Failed to open feedback database")?;
        store.stats()?
    };

    if output.json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        println!("{}", "Feedback Statistics".bold());
        println!("  Total injections:  {}", stats.total_injections);
        println!("  Total feedback:    {}", stats.total_feedback);
        if stats.total_feedback > 0 {
            println!(
                "  Useful:            {} ({:.0}%)",
                stats.useful,
                stats.useful as f64 / stats.total_feedback as f64 * 100.0
            );
            println!(
                "  Noise:             {} ({:.0}%)",
                stats.noise,
                stats.noise as f64 / stats.total_feedback as f64 * 100.0
            );
            println!(
                "  Harmful:           {} ({:.0}%)",
                stats.harmful,
                stats.harmful as f64 / stats.total_feedback as f64 * 100.0
            );
        }
        if stats.total_injections > 0 {
            println!(
                "  Feedback rate:     {:.0}%",
                stats.total_feedback as f64 / stats.total_injections as f64 * 100.0
            );
        }
    }

    Ok(())
}

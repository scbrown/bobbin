//! `bobbin hooks bundle|install|uninstall|status` — bobbin owns its hook
//! definitions and installs them into Claude Code AND Codex (aegis-5s32or).
//!
//! `bobbin hook install` (singular) predates this and writes Claude Code's
//! settings only; it is unchanged. This command is the harness-neutral one.
//!
//! The bundle (`hooks/bobbin.bundle.json`, schema `st.hook-bundle/1`) is the
//! ONE source of truth for which hooks bobbin wants. On a host running
//! shantytown, install REGISTERS it with `st ops hooks register`, and st renders
//! it into every role's Claude and Codex settings on every emit, so a settings
//! re-emit or relaunch cannot drop it. Without st, install writes the harness's
//! own config directly: `settings.json` for Claude, `config.toml` for Codex.
//! Both carry the same shape, `hooks.<Event> = [{matcher, hooks: [{type,
//! command}]}]`, so one merge serves both. A hook with no `harnesses` list
//! applies to every harness, as in st's schema.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{json, Map, Value};

use super::OutputConfig;

/// `bobbin hooks …`
#[derive(Args)]
pub struct HooksArgs {
    #[command(subcommand)]
    command: HooksCommand,
}

#[derive(Subcommand)]
enum HooksCommand {
    /// Print bobbin's hook bundle (schema `st.hook-bundle/1`): the one source of
    /// truth for which hooks bobbin installs.
    Bundle,
    /// Install bobbin's hooks. With shantytown present this registers the
    /// bundle, so st keeps it rendered into every role's Claude AND Codex
    /// settings; otherwise it writes the harness config directly.
    Install(Target),
    /// Remove bobbin's hooks (only bobbin's; other hooks are untouched).
    Uninstall(Target),
    /// Report, per harness, whether bobbin's hooks are installed.
    Status(Target),
}

#[derive(Args)]
struct Target {
    /// Harness to write directly (no st). Repeatable; default both.
    #[arg(long, value_enum)]
    harness: Vec<Harness>,
    /// Write the project config (`./.claude`, `./.codex`) instead of the user's.
    #[arg(long)]
    project: bool,
    /// Write the harness config directly even when st is present.
    #[arg(long)]
    no_st: bool,
}

/// A harness bobbin can install hooks into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Harness {
    /// Claude Code (`settings.json`).
    Claude,
    /// Codex (`config.toml`).
    Codex,
}

impl Harness {
    fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

/// The shipped bundle, with the build's version substituted.
pub fn bundle() -> Value {
    let text = include_str!("../../hooks/bobbin.bundle.json")
        .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"));
    serde_json::from_str(&text).expect("hooks/bobbin.bundle.json is valid JSON")
}

/// The `(event, matcher, command)` hooks the bundle declares for `harness`.
pub fn hooks_for(bundle: &Value, harness: Harness) -> Vec<(String, Option<String>, String)> {
    bundle["hooks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|h| {
            h["harnesses"]
                .as_array()
                .is_none_or(|hs| hs.iter().any(|x| x == harness.name()))
        })
        .map(|h| {
            (
                h["event"].as_str().unwrap_or_default().to_string(),
                h["matcher"].as_str().map(str::to_string),
                h["command"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// Add every bundle hook for `harness` to a `hooks` table. Idempotent.
/// Returns how many were added.
pub fn merge(config: &mut Value, bundle: &Value, harness: Harness) -> usize {
    if !config.is_object() {
        *config = json!({});
    }
    let map = config.as_object_mut().expect("object");
    if !map.get("hooks").is_some_and(Value::is_object) {
        map.insert("hooks".into(), json!({}));
    }
    let hooks = map
        .get_mut("hooks")
        .and_then(Value::as_object_mut)
        .expect("object");
    let mut added = 0;
    for (event, matcher, command) in hooks_for(bundle, harness) {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("hooks.<event> is a list");
        let index = match groups
            .iter()
            .position(|g| g["matcher"].as_str() == matcher.as_deref())
        {
            Some(i) => i,
            None => {
                let mut g = Map::new();
                if let Some(m) = &matcher {
                    g.insert("matcher".into(), json!(m));
                }
                g.insert("hooks".into(), json!([]));
                groups.push(Value::Object(g));
                groups.len() - 1
            }
        };
        let list = groups[index]["hooks"].as_array_mut().expect("hooks list");
        if !list.iter().any(|h| h["command"] == command.as_str()) {
            list.push(json!({"type": "command", "command": command}));
            added += 1;
        }
    }
    added
}

/// Remove every bundle hook for `harness` (matched by exact command), dropping
/// groups and events left empty. Hooks bobbin did not declare are untouched.
pub fn remove(config: &mut Value, bundle: &Value, harness: Harness) -> usize {
    let ours: Vec<String> = hooks_for(bundle, harness)
        .into_iter()
        .map(|h| h.2)
        .collect();
    let Some(hooks) = config.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut removed = 0;
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = list.len();
                list.retain(|h| !ours.iter().any(|c| h["command"] == c.as_str()));
                removed += before - list.len();
            }
        }
        groups.retain(|g| g["hooks"].as_array().is_none_or(|l| !l.is_empty()));
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|g| !g.is_empty()));
    removed
}

/// How many of the bundle's hooks for `harness` are present in `config`.
pub fn present(config: &Value, bundle: &Value, harness: Harness) -> (usize, usize) {
    let want = hooks_for(bundle, harness);
    let found = want
        .iter()
        .filter(|(event, matcher, command)| {
            config["hooks"][event].as_array().is_some_and(|groups| {
                groups.iter().any(|g| {
                    g["matcher"].as_str() == matcher.as_deref()
                        && g["hooks"]
                            .as_array()
                            .is_some_and(|l| l.iter().any(|h| h["command"] == command.as_str()))
                })
            })
        })
        .count();
    (found, want.len())
}

/// The harness config file: user scope (the harness home) or project scope.
pub fn config_path(harness: Harness, project: bool) -> PathBuf {
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    match (harness, project) {
        (Harness::Claude, false) => home().join(".claude/settings.json"),
        (Harness::Claude, true) => PathBuf::from(".claude/settings.json"),
        (Harness::Codex, false) => std::env::var_os("CODEX_HOME")
            .map_or_else(|| home().join(".codex"), PathBuf::from)
            .join("config.toml"),
        (Harness::Codex, true) => PathBuf::from(".codex/config.toml"),
    }
}

/// Read a harness config as JSON (TOML for Codex). Absent means empty.
pub fn read_config(path: &Path, harness: Harness) -> Result<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(json!({}));
    };
    Ok(match harness {
        Harness::Claude => serde_json::from_str(&text)
            .with_context(|| format!("{} is not valid JSON", path.display()))?,
        Harness::Codex => serde_json::to_value(
            toml::from_str::<toml::Value>(&text)
                .with_context(|| format!("{} is not valid TOML", path.display()))?,
        )?,
    })
}

/// Write a harness config, keeping the previous file as `<name>.bak-bobbin`
/// (a TOML rewrite does not keep comments).
pub fn write_config(path: &Path, harness: Harness, config: &Value) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".bak-bobbin");
        std::fs::copy(path, PathBuf::from(backup))?;
    }
    let text = match harness {
        Harness::Claude => serde_json::to_string_pretty(config)? + "\n",
        Harness::Codex => toml::to_string(&serde_json::from_value::<toml::Value>(config.clone())?)?,
    };
    std::fs::write(path, text)?;
    Ok(())
}

fn st_available() -> bool {
    Command::new("st")
        .args(["ops", "hooks", "list"])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn st_register(register: bool) -> Result<String> {
    let out = if register {
        let file = std::env::temp_dir().join(format!("bobbin-bundle-{}.json", std::process::id()));
        std::fs::write(&file, serde_json::to_string_pretty(&bundle())?)?;
        let out = Command::new("st")
            .args(["ops", "hooks", "register"])
            .arg(&file)
            .output();
        let _ = std::fs::remove_file(&file);
        out?
    } else {
        Command::new("st")
            .args(["ops", "hooks", "unregister", "bobbin"])
            .output()?
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        bail!("st refused: {}", text.trim());
    }
    Ok(text.trim().to_string())
}

/// Run one `bobbin hooks` action.
pub async fn run(args: HooksArgs, _output: OutputConfig) -> Result<()> {
    let bundle = bundle();
    let target = match &args.command {
        HooksCommand::Bundle => {
            println!("{}", serde_json::to_string_pretty(&bundle)?);
            return Ok(());
        }
        HooksCommand::Install(t) | HooksCommand::Uninstall(t) | HooksCommand::Status(t) => t,
    };
    let harnesses = if target.harness.is_empty() {
        vec![Harness::Claude, Harness::Codex]
    } else {
        target.harness.clone()
    };
    let via_st = !target.no_st && st_available();
    let mut ok = true;
    match &args.command {
        HooksCommand::Install(_) | HooksCommand::Uninstall(_) if via_st => {
            let register = matches!(args.command, HooksCommand::Install(_));
            println!("{}", st_register(register)?);
            println!(
                "bobbin hooks {} through shantytown: st renders them into every role's \
                 claude and codex settings. Check with `st ops hooks check`.",
                if register {
                    "registered"
                } else {
                    "unregistered"
                }
            );
        }
        HooksCommand::Status(_) if via_st => {
            let out = Command::new("st")
                .args(["ops", "hooks", "check", "--json"])
                .output()?;
            let report: Value = serde_json::from_slice(&out.stdout)?;
            for h in harnesses {
                let items: Vec<&Value> = report["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|i| i["bundle"] == "bobbin" && i["harness"] == h.name())
                    .collect();
                let count = |k: &str, v: &str| items.iter().filter(|i| i[k] == v).count();
                println!(
                    "{}: {} hook item(s) via st; configured ok {}, live ok {}, firing ok {}, silent {}",
                    h.name(),
                    items.len(),
                    count("configured", "ok"),
                    count("live", "ok"),
                    count("firing", "ok"),
                    count("firing", "silent"),
                );
                ok &= !items.is_empty() && count("configured", "ok") == items.len();
            }
        }
        _ => {
            for h in harnesses {
                let path = config_path(h, target.project);
                let mut cfg = read_config(&path, h)?;
                match &args.command {
                    HooksCommand::Install(_) => {
                        let n = merge(&mut cfg, &bundle, h);
                        if n > 0 {
                            write_config(&path, h, &cfg)?;
                        }
                        println!("{}: added {n} hook(s) to {}", h.name(), path.display());
                    }
                    HooksCommand::Uninstall(_) => {
                        let n = remove(&mut cfg, &bundle, h);
                        if n > 0 {
                            write_config(&path, h, &cfg)?;
                        }
                        println!("{}: removed {n} hook(s) from {}", h.name(), path.display());
                    }
                    _ => {
                        let (have, want) = present(&cfg, &bundle, h);
                        println!(
                            "{}: {have}/{want} bobbin hook(s) in {}",
                            h.name(),
                            path.display()
                        );
                        ok &= have == want;
                    }
                }
            }
        }
    }
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;

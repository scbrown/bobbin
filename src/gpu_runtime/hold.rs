//! A configured hold probe is fail-closed: an unreadable hold pauses GPU work.
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

// The external policy CLI can cost more than an inference batch. Cache only
// a successful clear result, for at most one second; file holds stay immediate.
static LAST_CLEAR: Mutex<Option<Instant>> = Mutex::new(None);

pub fn command() -> Result<Option<Vec<String>>> {
    if let Ok(value) = std::env::var("BOBBIN_GPU_HOLD_COMMAND") {
        let argv: Vec<String> = serde_json::from_str(&value)
            .context("BOBBIN_GPU_HOLD_COMMAND must be a JSON argv array")?;
        anyhow::ensure!(
            !argv.is_empty() && !argv[0].is_empty(),
            "empty GPU hold command"
        );
        return Ok(Some(argv));
    }
    // Managed crew hosts already publish their root. Standalone users need no st.
    Ok(std::env::var("SHANTY_ROOT").ok().map(|root| {
        vec![
            "st".into(),
            "--root".into(),
            root,
            "fleet".into(),
            "hold".into(),
            "gaming".into(),
            "--status".into(),
        ]
    }))
}

pub fn held() -> Result<bool> {
    if let Some(path) = std::env::var_os("BOBBIN_GPU_HOLD_FILE") {
        match std::fs::metadata(PathBuf::from(path)) {
            Ok(_) => return Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    if let Some(argv) = command()? {
        let mut clear = LAST_CLEAR
            .lock()
            .map_err(|_| anyhow::anyhow!("hold cache poisoned"))?;
        if clear.is_some_and(|when| when.elapsed() < Duration::from_secs(1)) {
            return Ok(false);
        }
        *clear = None;
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                if status.success() {
                    *clear = Some(Instant::now());
                }
                return Ok(!status.success());
            }
            if started.elapsed() > Duration::from_secs(5) {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("GPU hold probe timed out");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(false)
}

/// Called immediately before each bounded ONNX GPU batch and session creation.
/// An active session retains VRAM while paused; no new batch is submitted.
pub fn wait() {
    let mut announced = false;
    loop {
        match held() {
            Ok(false) => {
                if announced {
                    eprintln!("GPU resumed after hold");
                }
                return;
            }
            result => {
                if !announced {
                    eprintln!("GPU paused: gaming hold active or unreadable ({result:?})");
                    announced = true;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

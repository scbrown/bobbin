//! Bootstrap CUDA before any ORT call or async worker is started.
//! Native loaders read LD_LIBRARY_PATH at process startup, hence the single exec.
mod hold;
mod provision;

use crate::config::{EmbeddingBackend, EmbeddingConfig};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub use hold::wait as wait_for_hold;

/// Existing verified cache can also supply a CPU session; never downloads.
pub fn cached_ort() -> Option<std::path::PathBuf> {
    provision::cache_root()
        .ok()
        .and_then(|p| provision::verify(&p.join(provision::VERSION)).ok())
}

pub fn force_cpu() -> bool {
    std::env::var("BOBBIN_GPU").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("false"))
}

fn device() -> Result<String> {
    // A tempfile prevents a full stdout pipe from defeating the timeout.
    let output = tempfile::tempfile()?;
    let mut child = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,uuid,driver_version",
            "--format=csv,noheader",
        ])
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(status.success(), "nvidia-smi failed: {status}");
            use std::io::{Read, Seek, SeekFrom};
            let mut output = output;
            output.seek(SeekFrom::Start(0))?;
            let mut text = String::new();
            output.take(8192).read_to_string(&mut text)?;
            anyhow::ensure!(!text.trim().is_empty(), "no NVIDIA devices");
            return Ok(text.trim().to_string());
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("nvidia-smi timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[derive(Serialize)]
pub struct Diagnostics {
    pub device: Option<String>,
    pub policy: String,
    pub runtime: Option<String>,
    pub hold: String,
    pub note: String,
}

impl Diagnostics {
    pub fn print(&self) {
        println!(
            "  GPU device:   {}",
            self.device.as_deref().unwrap_or("not detected")
        );
        println!("  GPU policy:   {}; hold: {}", self.policy, self.hold);
        println!(
            "  GPU runtime:  {}",
            self.runtime.as_deref().unwrap_or("not discovered")
        );
        println!("  {}", self.note);
    }
}

/// Read-only: never downloads or initializes an ONNX session.
pub fn diagnostics() -> Diagnostics {
    let runtime = std::env::var("ORT_DYLIB_PATH").ok().or_else(|| {
        provision::cache_root()
            .ok()
            .map(|p| p.join(provision::VERSION).join("lib").join(provision::CORE))
            .filter(|p| p.is_file())
            .map(|p| p.display().to_string())
    });
    Diagnostics {
        device: device().ok(),
        policy: if force_cpu() { "cpu (BOBBIN_GPU opt-out)" } else { "auto" }.into(),
        runtime,
        hold: match hold::held() { Ok(true) => "active".into(), Ok(false) => "clear".into(), Err(e) => format!("unknown: {e}") },
        note: "Device/runtime discovery only; actual CUDA use is reported when an ONNX session is created.".into(),
    }
}

/// Index bootstrap only; CPU opt-out and API/remote paths must not download.
pub fn prepare(config: &EmbeddingConfig) -> Result<()> {
    if config.backend != EmbeddingBackend::Onnx {
        return Ok(());
    }
    if force_cpu() {
        eprintln!("ONNX device: CPU (BOBBIN_GPU opt-out)");
        return Ok(());
    }
    let gpu = match device() {
        Ok(gpu) => gpu,
        Err(e) => {
            if config.gpu
                || std::env::var("BOBBIN_GPU")
                    .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            {
                eprintln!("warning: GPU requested but NVIDIA probe failed: {e}; CUDA session will be checked at load");
            }
            return Ok(());
        }
    };
    eprintln!("GPU detected: {gpu}");
    match hold::held() {
        Ok(false) => {}
        held => {
            eprintln!("GPU deferred: gaming hold active or unreadable ({held:?}); using CPU");
            std::env::set_var("BOBBIN_GPU", "0");
            return Ok(());
        }
    }
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        eprintln!("warning: automatic CUDA runtime provisioning supports Linux x86_64; using existing runtime on this platform");
        return Ok(());
    }
    // A detected device must attempt CUDA registration, even if the runtime
    // later reports an unavailable provider. That failure must be visible.
    std::env::set_var("BOBBIN_GPU", "1");
    // An explicit external runtime remains authoritative. Its session creation
    // below uses error_on_failure, so it can never earn a false CUDA receipt.
    if let Some(path) = std::env::var_os("ORT_DYLIB_PATH") {
        let path = Path::new(&path);
        if path.is_file()
            && path
                .parent()
                .is_some_and(|p| p.join("libonnxruntime_providers_cuda.so").is_file())
        {
            return Ok(());
        }
    }
    let result = provision::cache_root().and_then(|root| provision::install(&root));
    match result {
        Ok(core) => reexec(&core),
        Err(e) => {
            eprintln!("warning: NVIDIA device present but GPU provisioning failed: {e:#}; falling back to CPU");
            std::env::set_var("BOBBIN_GPU", "0");
            Ok(())
        }
    }
}

fn reexec(core: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let lib = core.parent().context("GPU runtime has no parent")?;
        let mut paths = vec![lib.to_path_buf()];
        if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        let error = Command::new(std::env::current_exe()?)
            .args(std::env::args_os().skip(1))
            .env("ORT_DYLIB_PATH", core)
            .env("LD_LIBRARY_PATH", std::env::join_paths(paths)?)
            .exec();
        Err(error).context("restarting Bobbin with verified GPU runtime")
    }
    #[cfg(not(unix))]
    {
        let _ = core;
        anyhow::bail!("GPU runtime restart is not supported on this platform")
    }
}

#[cfg(test)]
mod tests;

use anyhow::Result;
use clap::Parser;

mod analysis;
mod cli;
mod commands;
mod config;
mod http;
mod index;
mod mcp;
mod metrics;
mod search;
mod storage;
mod types;

use cli::Cli;

/// Search for `libonnxruntime.so` (or `.dylib` on macOS) in common locations
/// and set `ORT_DYLIB_PATH` so the `ort` crate can load it via `dlopen`.
///
/// Search order:
/// 1. `ORT_DYLIB_PATH` already set → no-op
/// 2. `<exe_dir>/lib/` — release tarball bundle
/// 3. `<exe_dir>/` — library next to binary
/// 4. pip/uv cache under `~/.cache/uv/` or `~/.local/lib/python*/site-packages/`
/// 5. System paths: `/usr/lib/`, `/usr/local/lib/`
fn ensure_ort_dylib() {
    if std::env::var_os("ORT_DYLIB_PATH").is_some() {
        return;
    }

    if let Some(path) = find_ort_dylib() {
        std::env::set_var("ORT_DYLIB_PATH", &path);
    }
}

fn find_ort_dylib() -> Option<std::path::PathBuf> {
    let ext_check: fn(&std::path::Path) -> bool = if cfg!(target_os = "macos") {
        |p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with("libonnxruntime") && name.contains(".dylib") && !name.contains("providers")
        }
    } else {
        |p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with("libonnxruntime.so") && !name.contains("providers")
        }
    };

    // 1. Next to executable: <exe_dir>/lib/
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // Check lib/ subdirectory (release bundle layout)
            let lib_dir = exe_dir.join("lib");
            if let Some(found) = search_dir(&lib_dir, ext_check) {
                return Some(found);
            }
            // Check next to the binary itself
            if let Some(found) = search_dir(exe_dir, ext_check) {
                return Some(found);
            }
        }
    }

    // 2. uv/pip cache: ~/.cache/uv/**/onnxruntime/capi/libonnxruntime.so*
    if let Some(home) = home_dir() {
        let uv_cache = home.join(".cache").join("uv");
        if uv_cache.is_dir() {
            if let Some(found) = search_uv_cache(&uv_cache, ext_check) {
                return Some(found);
            }
        }

        // Python site-packages
        let local_lib = home.join(".local").join("lib");
        if local_lib.is_dir() {
            if let Some(found) = search_python_site_packages(&local_lib, ext_check) {
                return Some(found);
            }
        }
    }

    // 3. System paths
    for dir in &["/usr/local/lib", "/usr/lib"] {
        let path = std::path::Path::new(dir);
        if let Some(found) = search_dir(path, ext_check) {
            return Some(found);
        }
    }

    None
}

fn search_dir(dir: &std::path::Path, check: fn(&std::path::Path) -> bool) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && check(&path) {
            return Some(path);
        }
    }
    None
}

/// Walk uv cache looking for onnxruntime/capi/ directories.
fn search_uv_cache(
    uv_cache: &std::path::Path,
    check: fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    // uv layout: ~/.cache/uv/archive-v0/<hash>/onnxruntime/capi/libonnxruntime.so*
    for archive_entry in std::fs::read_dir(uv_cache).ok()?.flatten() {
        let archive_dir = archive_entry.path();
        if !archive_dir.is_dir() {
            continue;
        }
        for hash_entry in std::fs::read_dir(&archive_dir).ok()?.flatten() {
            let capi_dir = hash_entry.path().join("onnxruntime").join("capi");
            if capi_dir.is_dir() {
                if let Some(found) = search_dir(&capi_dir, check) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Search python site-packages for onnxruntime.
fn search_python_site_packages(
    local_lib: &std::path::Path,
    check: fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    // ~/.local/lib/python3.*/site-packages/onnxruntime/capi/
    for py_entry in std::fs::read_dir(local_lib).ok()?.flatten() {
        let name = py_entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("python") {
            let capi_dir = py_entry
                .path()
                .join("site-packages")
                .join("onnxruntime")
                .join("capi");
            if capi_dir.is_dir() {
                if let Some(found) = search_dir(&capi_dir, check) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

#[tokio::main]
async fn main() -> Result<()> {
    ensure_ort_dylib();

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();
    cli.run().await
}

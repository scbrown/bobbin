//! Checksum-pinned userspace libraries, installed atomically in a private cache.
use anyhow::{bail, Context, Result};
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const VERSION: &str = "onnx-1.23.2-cuda12-v1";
pub const CORE: &str = "libonnxruntime.so.1.23.2";
const MAX_ARCHIVE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 6 * 1024 * 1024 * 1024;

#[derive(Deserialize)]
struct Artifact {
    name: String,
    url: String,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    version: String,
    files: BTreeMap<String, String>,
}

pub fn cache_root() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("BOBBIN_GPU_CACHE") {
        return Ok(PathBuf::from(p));
    }
    let dirs = directories::ProjectDirs::from("", "", "bobbin")
        .context("Cannot locate user data directory; set BOBBIN_GPU_CACHE")?;
    Ok(dirs.data_local_dir().join("gpu-runtime"))
}

fn hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Recheck installed bytes, not just a completion marker, on every invocation.
pub fn verify(dir: &Path) -> Result<PathBuf> {
    let receipt: Receipt = serde_json::from_reader(File::open(dir.join("receipt.json"))?)?;
    if receipt.version != VERSION
        || !receipt.files.contains_key(CORE)
        || !receipt
            .files
            .contains_key("libonnxruntime_providers_cuda.so")
    {
        bail!("incomplete GPU runtime receipt");
    }
    for (name, expected) in receipt.files {
        if Path::new(&name).file_name().and_then(|s| s.to_str()) != Some(&name) {
            bail!("invalid GPU runtime receipt path");
        }
        let path = dir.join("lib").join(&name);
        if fs::symlink_metadata(&path)?.file_type().is_symlink() || hash(&path)? != expected {
            bail!("GPU runtime integrity mismatch: {name}");
        }
    }
    Ok(dir.join("lib").join(CORE))
}

pub fn install(root: &Path) -> Result<PathBuf> {
    fs::create_dir_all(root)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("install.lock"))?;
    let start = Instant::now();
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() > Duration::from_secs(600) {
                    bail!("GPU runtime installer lock timed out");
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => return Err(e.into()),
        }
    }
    let destination = root.join(VERSION);
    if let Ok(core) = verify(&destination) {
        return Ok(core);
    }
    eprintln!("GPU: provisioning pinned ONNX Runtime and CUDA userspace libraries in {} (first download is large)", root.display());
    let stage = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(root)?;
    fs::create_dir(stage.path().join("lib"))?;
    fs::create_dir(stage.path().join("licenses"))?;
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(900))
        .build()?;
    let artifacts: Vec<Artifact> = serde_json::from_str(include_str!("artifacts.json"))?;
    let mut files = BTreeMap::new();
    let mut total = 0;
    for artifact in artifacts {
        eprintln!("GPU: downloading {}", artifact.name);
        let mut response = client.get(&artifact.url).send()?.error_for_status()?;
        let mut archive = tempfile::tempfile_in(root)?;
        let copied = std::io::copy(&mut response.by_ref().take(MAX_ARCHIVE + 1), &mut archive)?;
        if copied > MAX_ARCHIVE {
            bail!("GPU archive exceeds size limit");
        }
        use std::io::{Seek, SeekFrom};
        archive.seek(SeekFrom::Start(0))?;
        let mut digest = Sha256::new();
        std::io::copy(&mut archive, &mut digest)?;
        if hex::encode(digest.finalize()) != artifact.sha256 {
            bail!("GPU archive checksum mismatch: {}", artifact.name);
        }
        archive.seek(SeekFrom::Start(0))?;
        extract(
            archive,
            stage.path(),
            &artifact.name,
            &mut files,
            &mut total,
        )?;
    }
    let receipt = Receipt {
        version: VERSION.into(),
        files,
    };
    fs::write(
        stage.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    verify(stage.path())?;
    // Never expose a partial runtime. A damaged prior cache is retained for diagnosis.
    if destination.exists() {
        let quarantine = root.join(format!(".damaged-{}-{}", VERSION, std::process::id()));
        fs::rename(&destination, quarantine)?;
    }
    fs::rename(stage.path(), &destination)?;
    verify(&destination)
}

fn extract<R: Read + std::io::Seek>(
    reader: R,
    stage: &Path,
    artifact: &str,
    files: &mut BTreeMap<String, String>,
    total: &mut u64,
) -> Result<()> {
    let mut zip = zip::ZipArchive::new(reader)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let enclosed = entry.enclosed_name().context("unsafe GPU archive path")?;
        let name = enclosed
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if entry.is_dir() {
            continue;
        }
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            bail!("GPU archive symlinks are not supported");
        }
        let library = name.starts_with("lib") && name.contains(".so");
        let license = name.to_ascii_lowercase().starts_with("license");
        if !library && !license {
            continue;
        }
        *total = total
            .checked_add(entry.size())
            .context("GPU archive size overflow")?;
        if *total > MAX_EXTRACTED {
            bail!("GPU extracted size limit exceeded");
        }
        let dest = if library {
            stage.join("lib").join(&name)
        } else {
            stage
                .join("licenses")
                .join(format!("{artifact}-{i}-{name}"))
        };
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&dest)?;
        let n = std::io::copy(&mut entry.by_ref().take(MAX_EXTRACTED + 1), &mut output)?;
        if n != entry.size() {
            bail!("GPU archive entry size mismatch");
        }
        output.flush()?;
        if library {
            files.insert(name, hash(&dest)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_corruption_and_missing_library_are_rejected() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("lib")).unwrap();
        let mut files = BTreeMap::new();
        for name in [CORE, "libonnxruntime_providers_cuda.so"] {
            let p = d.path().join("lib").join(name);
            fs::write(&p, b"fixture").unwrap();
            files.insert(name.into(), hash(&p).unwrap());
        }
        fs::write(
            d.path().join("receipt.json"),
            serde_json::to_vec(&Receipt {
                version: VERSION.into(),
                files,
            })
            .unwrap(),
        )
        .unwrap();
        assert!(verify(d.path()).is_ok());
        fs::write(d.path().join("lib").join(CORE), b"tampered").unwrap();
        assert!(verify(d.path()).is_err());
        fs::remove_file(d.path().join("lib").join(CORE)).unwrap();
        assert!(verify(d.path()).is_err());
    }
    #[test]
    fn archive_traversal_is_rejected() {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("../libevil.so", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"bad").unwrap();
        let reader = writer.finish().unwrap();
        let d = tempfile::tempdir().unwrap();
        assert!(extract(reader, d.path(), "test", &mut BTreeMap::new(), &mut 0).is_err());
        assert!(!d.path().join("libevil.so").exists());
    }
}

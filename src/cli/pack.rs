use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::OutputConfig;
use crate::config::{Config, EmbeddingBackend};
use crate::index::embedder;
use crate::storage::{MetadataStore, VectorStore};

const SCHEMA: &str = "https://github.com/scbrown/bobbin/index-pack/v1";
const MANIFEST: &str = "manifest.json";

#[derive(Args)]
pub struct PackArgs {
    #[command(subcommand)]
    command: PackCommand,
}

#[derive(Subcommand)]
enum PackCommand {
    /// Export the current local index as a compressed, checksummed artifact.
    Export(ExportArgs),
    /// Verify an artifact without changing the local index.
    Verify(VerifyArgs),
    /// Verify, atomically activate, then incrementally reindex to source HEAD.
    Import(ImportArgs),
}

#[derive(Args)]
struct ExportArgs {
    /// Bobbin home containing .bobbin/config.toml and index data.
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Indexed repository name (defaults to source directory name).
    #[arg(long)]
    repo: Option<String>,
    /// Indexed Git checkout (defaults to the stored repo_source mapping).
    #[arg(long)]
    source: Option<PathBuf>,
    /// Destination .bbpack file.
    #[arg(long, short)]
    output: PathBuf,
}

#[derive(Args)]
struct VerifyArgs {
    pack: PathBuf,
    /// Bobbin home whose embedding configuration must match the pack.
    #[arg(long, default_value = ".")]
    path: PathBuf,
}

#[derive(Args)]
struct ImportArgs {
    pack: PathBuf,
    /// Bobbin home to activate into.
    #[arg(long, default_value = ".")]
    path: PathBuf,
    /// Current Git checkout; defaults to the packed repo_source path.
    #[arg(long)]
    source: Option<PathBuf>,
    /// Verify and activate only; do not catch up from packed SHA to HEAD.
    #[arg(long)]
    no_reindex: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct EmbeddingIdentity {
    backend: String,
    model_id: String,
    model_version: String,
    dimensions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackManifest {
    schema: String,
    repo: String,
    sha: String,
    created_at: String,
    embedding: EmbeddingIdentity,
    bobbin_version: String,
    bobbin_git_sha: String,
    sqlite_version: String,
    lance_format_version: String,
    files: BTreeMap<String, String>,
    uncompressed_bytes: u64,
}

pub async fn run(args: PackArgs, output: OutputConfig) -> Result<()> {
    match args.command {
        PackCommand::Export(a) => export(a, output).await,
        PackCommand::Verify(a) => {
            let home = canonical_dir(&a.path)?;
            let tmp = tempfile::tempdir()?;
            let manifest = extract_and_verify(&a.pack, tmp.path(), Some(&home))?;
            print_result("verified", &a.pack, &manifest, &output)
        }
        PackCommand::Import(a) => import(a, output).await,
    }
}

async fn export(args: ExportArgs, output: OutputConfig) -> Result<()> {
    let home = canonical_dir(&args.path)?;
    let config = Config::load(&Config::config_path(&home))?;
    let db_path = Config::db_path(&home);
    let vectors = Config::lance_path(&home);
    if !db_path.is_file() || !vectors.is_dir() {
        bail!(
            "Bobbin index is incomplete; expected {} and {}",
            db_path.display(),
            vectors.display()
        );
    }
    let store = MetadataStore::open(&db_path)?;
    let source = args
        .source
        .or_else(|| {
            args.repo.as_ref().and_then(|r| {
                store
                    .get_meta(&format!("repo_source:{r}"))
                    .ok()
                    .flatten()
                    .map(PathBuf::from)
            })
        })
        .unwrap_or_else(|| home.clone());
    let source = canonical_dir(&source)?;
    let repo = args.repo.unwrap_or_else(|| {
        source
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("default")
            .to_string()
    });
    let indexed_repos = VectorStore::open_with_dim(
        &vectors,
        embedder::resolve_dimension(&config.embedding)? as i32,
    )
    .await?
    .get_all_repos()
    .await?;
    if indexed_repos.iter().any(|indexed| indexed != &repo) {
        bail!(
            "cannot export repo '{}' from a shared store containing {:?}; build packs in a dedicated per-repo Bobbin home",
            repo,
            indexed_repos
        );
    }
    let sha = git(&source, &["rev-parse", "HEAD"])?;

    let tmp = tempfile::tempdir()?;
    let data = tmp.path().join("data");
    fs::create_dir(&data)?;
    store.snapshot(&data.join("index.db"))?;
    copy_tree(&vectors, &data.join("vectors"))?;
    let (files, bytes) = checksums(&data)?;
    let manifest = PackManifest {
        schema: SCHEMA.into(),
        repo,
        sha,
        created_at: chrono::Utc::now().to_rfc3339(),
        embedding: embedding_identity(&config)?,
        bobbin_version: env!("CARGO_PKG_VERSION").into(),
        bobbin_git_sha: env!("BOBBIN_GIT_SHA").into(),
        sqlite_version: rusqlite::version().into(),
        lance_format_version: "lancedb-0.27/lance-4".into(),
        files,
        uncompressed_bytes: bytes,
    };
    fs::write(
        tmp.path().join(MANIFEST),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let partial = args.output.with_extension("bbpack.partial");
    let encoder = zstd::Encoder::new(BufWriter::new(File::create(&partial)?), 9)?;
    let mut tar = tar::Builder::new(encoder);
    tar.append_path_with_name(tmp.path().join(MANIFEST), MANIFEST)?;
    tar.append_dir_all("data", &data)?;
    tar.finish()?;
    tar.into_inner()?.finish()?;
    fs::rename(&partial, &args.output)?;
    print_result("exported", &args.output, &manifest, &output)
}

async fn import(args: ImportArgs, output: OutputConfig) -> Result<()> {
    let home = canonical_dir(&args.path)?;
    let data_dir = Config::data_dir(&home);
    fs::create_dir_all(&data_dir)?;
    let stage = tempfile::Builder::new()
        .prefix("pack-import-")
        .tempdir_in(&data_dir)?;
    let manifest = match extract_and_verify(&args.pack, stage.path(), Some(&home)) {
        Ok(m) => m,
        Err(e) => {
            let _ = fs::remove_file(&args.pack);
            return Err(e.context("invalid pack deleted before activation"));
        }
    };
    let source_arg = args.source.unwrap_or_else(|| home.clone());
    let source_check = (|| -> Result<(PathBuf, String)> {
        let source = canonical_dir(&source_arg)?;
        git(
            &source,
            &["cat-file", "-e", &format!("{}^{{commit}}", manifest.sha)],
        )
        .context("packed source commit is not present in the target repository")?;
        let head = git(&source, &["rev-parse", "HEAD"])?;
        git(
            &source,
            &["merge-base", "--is-ancestor", &manifest.sha, &head],
        )
        .context("packed SHA is not an ancestor of target HEAD")?;
        Ok((source, head))
    })();
    let (source, head) = match source_check {
        Ok(checked) => checked,
        Err(error) => {
            let _ = fs::remove_file(&args.pack);
            return Err(error.context("incompatible pack deleted before activation"));
        }
    };

    activate_payload(&stage.path().join("data"), &data_dir)?;
    if !args.no_reindex && head != manifest.sha {
        super::pack_reindex::run(home.clone(), source, manifest.repo.clone(), output.clone())
            .await?;
    }
    print_result(
        if head == manifest.sha || args.no_reindex {
            "imported"
        } else {
            "imported_and_reindexed"
        },
        &args.pack,
        &manifest,
        &output,
    )
}

fn extract_and_verify(
    pack: &Path,
    destination: &Path,
    home: Option<&Path>,
) -> Result<PackManifest> {
    let decoder = zstd::Decoder::new(BufReader::new(
        File::open(pack).with_context(|| format!("Cannot open pack {}", pack.display()))?,
    ))?;
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(destination)
        .context("Pack contains an invalid or unsafe archive path")?;
    let manifest: PackManifest = serde_json::from_slice(&fs::read(destination.join(MANIFEST))?)?;
    if manifest.schema != SCHEMA {
        bail!("unsupported pack schema: {}", manifest.schema);
    }
    if let Some(home) = home {
        let config = Config::load(&Config::config_path(home))?;
        let actual = embedding_identity(&config)?;
        if manifest.embedding != actual {
            bail!(
                "embedding mismatch: pack {:?}, local {:?}",
                manifest.embedding,
                actual
            );
        }
    }
    let (actual, bytes) = checksums(&destination.join("data"))?;
    if actual != manifest.files || bytes != manifest.uncompressed_bytes {
        bail!("pack checksum verification failed");
    }
    if !destination.join("data/index.db").is_file() || !destination.join("data/vectors").is_dir() {
        bail!("pack payload is incomplete");
    }
    Ok(manifest)
}

fn embedding_identity(config: &Config) -> Result<EmbeddingIdentity> {
    let backend = match config.embedding.backend {
        EmbeddingBackend::Onnx => "onnx",
        EmbeddingBackend::OpenaiApi => "openai-api",
    };
    let version = if let Some((_, v)) = config.embedding.model.rsplit_once('@') {
        v.to_string()
    } else if let Some((_, v)) = config.embedding.model.rsplit_once("-v") {
        format!("v{v}")
    } else {
        "unversioned".into()
    };
    Ok(EmbeddingIdentity {
        backend: backend.into(),
        model_id: config.embedding.model.clone(),
        model_version: version,
        dimensions: embedder::resolve_dimension(&config.embedding)?,
    })
}

fn checksums(root: &Path) -> Result<(BTreeMap<String, String>, u64)> {
    let mut out = BTreeMap::new();
    let mut bytes = 0;
    let mut paths: Vec<_> = walkdir::WalkDir::new(root)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    paths.sort_by_key(|e| e.path().to_path_buf());
    for entry in paths {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let mut f = File::open(entry.path())?;
        let mut h = Sha256::new();
        let n = std::io::copy(&mut f, &mut h)?;
        bytes += n;
        out.insert(rel, format!("sha256:{:x}", h.finalize()));
    }
    Ok((out, bytes))
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for e in walkdir::WalkDir::new(from) {
        let e = e?;
        let rel = e.path().strip_prefix(from)?;
        let dst = to.join(rel);
        if e.file_type().is_dir() {
            fs::create_dir_all(dst)?
        } else if e.file_type().is_file() {
            fs::copy(e.path(), dst)?;
        }
    }
    Ok(())
}

fn activate_payload(staged: &Path, data_dir: &Path) -> Result<()> {
    let backup = data_dir.join("pack-backup");
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    fs::create_dir(&backup)?;
    for name in ["index.db", "vectors"] {
        let current = data_dir.join(name);
        if current.exists() {
            fs::rename(&current, backup.join(name))?;
        }
    }
    for name in ["index.db", "vectors"] {
        let current = data_dir.join(name);
        if let Err(error) = fs::rename(staged.join(name), &current) {
            for installed in ["index.db", "vectors"] {
                let path = data_dir.join(installed);
                if path.is_dir() {
                    let _ = fs::remove_dir_all(path);
                } else if path.exists() {
                    let _ = fs::remove_file(path);
                }
                let old = backup.join(installed);
                if old.exists() {
                    let _ = fs::rename(old, data_dir.join(installed));
                }
            }
            return Err(error).context("Failed to activate pack; previous index restored");
        }
    }
    fs::remove_dir_all(backup)?;
    Ok(())
}
fn canonical_dir(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("Invalid directory: {}", path.display()))
}
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let o = Command::new("git").args(args).current_dir(dir).output()?;
    if !o.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        )
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().into())
}
fn print_result(status: &str, path: &Path, m: &PackManifest, o: &OutputConfig) -> Result<()> {
    if o.json {
        println!(
            "{}",
            serde_json::json!({"status":status,"pack":path,"manifest":m})
        );
    } else if !o.quiet {
        println!(
            "{} {} (repo {}, sha {}, {} bytes)",
            status,
            path.display(),
            m.repo,
            m.sha,
            m.uncompressed_bytes
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_pack() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let build = root.path().join("build");
        fs::create_dir_all(build.join("data/vectors")).unwrap();
        fs::write(build.join("data/index.db"), b"sqlite").unwrap();
        fs::write(build.join("data/vectors/table"), b"lance").unwrap();
        let (files, uncompressed_bytes) = checksums(&build.join("data")).unwrap();
        let manifest = PackManifest {
            schema: SCHEMA.into(),
            repo: "fixture".into(),
            sha: "0123456789abcdef".into(),
            created_at: "2026-09-03T00:00:00Z".into(),
            embedding: embedding_identity(&Config::default()).unwrap(),
            bobbin_version: "test".into(),
            bobbin_git_sha: "test".into(),
            sqlite_version: "test".into(),
            lance_format_version: "test".into(),
            files,
            uncompressed_bytes,
        };
        fs::write(
            build.join(MANIFEST),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let pack = root.path().join("fixture.bbpack");
        let encoder = zstd::Encoder::new(BufWriter::new(File::create(&pack).unwrap()), 1).unwrap();
        let mut archive = tar::Builder::new(encoder);
        archive
            .append_path_with_name(build.join(MANIFEST), MANIFEST)
            .unwrap();
        archive.append_dir_all("data", build.join("data")).unwrap();
        archive.finish().unwrap();
        archive.into_inner().unwrap().finish().unwrap();
        (root, pack)
    }
    #[test]
    fn checksum_detects_change() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("x"), b"one").unwrap();
        let a = checksums(d.path()).unwrap();
        fs::write(d.path().join("x"), b"two").unwrap();
        assert_ne!(a, checksums(d.path()).unwrap());
    }
    #[test]
    fn identity_contains_dimensions_and_version() {
        let c = Config::default();
        let i = embedding_identity(&c).unwrap();
        assert_eq!(i.dimensions, 384);
        assert_eq!(i.model_version, "v2");
    }

    #[test]
    fn activation_replaces_both_stores() {
        let d = tempfile::tempdir().unwrap();
        let staged = d.path().join("staged");
        let live = d.path().join("live");
        fs::create_dir_all(staged.join("vectors")).unwrap();
        fs::create_dir_all(live.join("vectors")).unwrap();
        fs::write(staged.join("index.db"), b"new-db").unwrap();
        fs::write(staged.join("vectors/new"), b"new-vectors").unwrap();
        fs::write(live.join("index.db"), b"old-db").unwrap();
        fs::write(live.join("vectors/old"), b"old-vectors").unwrap();
        activate_payload(&staged, &live).unwrap();
        assert_eq!(fs::read(live.join("index.db")).unwrap(), b"new-db");
        assert!(live.join("vectors/new").is_file());
        assert!(!live.join("vectors/old").exists());
        assert!(!live.join("pack-backup").exists());
    }

    #[test]
    fn verify_refuses_embedding_mismatch() {
        let (_fixture, pack) = fixture_pack();
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join(".bobbin")).unwrap();
        let mut config = Config::default();
        config.embedding.dimensions = Some(768);
        config.save(&Config::config_path(home.path())).unwrap();
        let extract = tempfile::tempdir().unwrap();
        let error = extract_and_verify(&pack, extract.path(), Some(home.path())).unwrap_err();
        assert!(error.to_string().contains("embedding mismatch"));
    }
}

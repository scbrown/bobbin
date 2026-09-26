use super::*;
use std::fs;
use std::process::Command;

// Environment-sensitive checks run in child processes, never race other tests.
fn child(case: &str, home: &Path) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "gpu_runtime::tests::probe",
        "--ignored",
        "--nocapture",
    ])
    .env("BOBBIN_GPU_TEST_CASE", case)
    .env("HOME", home)
    .env("BOBBIN_GPU_CACHE", home.join("runtime"))
    .env_remove("SHANTY_ROOT")
    .env_remove("BOBBIN_GPU_HOLD_COMMAND")
    .env_remove("BOBBIN_GPU_HOLD_FILE")
    .env_remove("BOBBIN_GPU")
    .env_remove("ORT_DYLIB_PATH");
    cmd
}

#[test]
fn cpu_and_api_do_not_probe_or_download() {
    let d = tempfile::tempdir().unwrap();
    for case in ["cpu", "api"] {
        let output = child(case, d.path()).env("PATH", "").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!d.path().join("runtime").exists());
    }
}

#[cfg(unix)]
#[test]
fn startup_hold_selects_cpu_without_download() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let probe = d.path().join("nvidia-smi");
    fs::write(
        &probe,
        "#!/bin/sh\nprintf 'Fixture NVIDIA GPU, fixture-id, 999\\n'\n",
    )
    .unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();
    let hold = d.path().join("hold");
    fs::write(&hold, "held").unwrap();
    let output = child("startup-hold", d.path())
        .env("PATH", d.path())
        .env("BOBBIN_GPU_HOLD_FILE", hold)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("using CPU"));
    assert!(!d.path().join("runtime").exists());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn detected_device_requires_cuda_registration() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let probe = d.path().join("nvidia-smi");
    fs::write(
        &probe,
        "#!/bin/sh\nprintf 'Fixture NVIDIA GPU, fixture-id, 999\\n'\n",
    )
    .unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();
    let core = d.path().join("libonnxruntime.so");
    fs::write(&core, b"fixture").unwrap();
    fs::write(
        d.path().join("libonnxruntime_providers_cuda.so"),
        b"fixture",
    )
    .unwrap();
    let output = child("require-cuda", d.path())
        .env("PATH", d.path())
        .env("ORT_DYLIB_PATH", core)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!d.path().join("runtime").exists());
}

#[cfg(unix)]
#[test]
fn command_probe_is_coalesced_but_file_hold_is_immediate() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let script = d.path().join("probe");
    let count = d.path().join("calls");
    fs::write(&script, "#!/bin/sh\nprintf x >> \"$1\"\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let output = child("coalesce", d.path())
        .env(
            "BOBBIN_GPU_HOLD_COMMAND",
            serde_json::to_string(&[&script, &count]).unwrap(),
        )
        .env("BOBBIN_GPU_HOLD_FILE", d.path().join("hold"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(count).unwrap(), "xx");
}

#[test]
fn missing_hold_command_is_not_clear() {
    let d = tempfile::tempdir().unwrap();
    let output = child("hold-error", d.path())
        .env("BOBBIN_GPU_HOLD_COMMAND", "[\"/nonexistent-hold-probe\"]")
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn active_hold_pauses_then_resumes() {
    let d = tempfile::tempdir().unwrap();
    let hold = d.path().join("hold");
    fs::write(&hold, b"held").unwrap();
    let mut proc = child("wait", d.path())
        .env("BOBBIN_GPU_HOLD_FILE", &hold)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(proc.try_wait().unwrap().is_none());
    fs::remove_file(&hold).unwrap();
    let start = Instant::now();
    while proc.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(5) {
            let _ = proc.kill();
            panic!("hold did not resume");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(proc.wait().unwrap().success());
}

#[test]
#[ignore = "subprocess helper"]
fn probe() {
    match std::env::var("BOBBIN_GPU_TEST_CASE").unwrap().as_str() {
        "cpu" => {
            std::env::set_var("BOBBIN_GPU", "0");
            prepare(&EmbeddingConfig::default()).unwrap();
        }
        "api" => {
            let cfg = EmbeddingConfig {
                backend: EmbeddingBackend::OpenaiApi,
                ..Default::default()
            };
            prepare(&cfg).unwrap();
        }
        "startup-hold" => {
            prepare(&EmbeddingConfig::default()).unwrap();
            assert!(force_cpu());
        }
        "require-cuda" => {
            prepare(&EmbeddingConfig::default()).unwrap();
            assert_eq!(std::env::var("BOBBIN_GPU").unwrap(), "1");
        }
        "coalesce" => {
            assert!(!hold::held().unwrap());
            assert!(!hold::held().unwrap());
            let path = std::env::var("BOBBIN_GPU_HOLD_FILE").unwrap();
            fs::write(&path, "held").unwrap();
            assert!(hold::held().unwrap());
            fs::remove_file(path).unwrap();
            std::thread::sleep(Duration::from_millis(1100));
            assert!(!hold::held().unwrap());
        }
        "hold-error" => assert!(hold::held().is_err()),
        "wait" => wait_for_hold(),
        _ => panic!("unknown probe"),
    }
}

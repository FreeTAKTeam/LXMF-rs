#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::Command;

const TARGET_RNS: &str = "7f2b3b9b524c9386316379af1313b43a5e4f7a5d";
const TARGET_LXMF: &str = "c3ff2d6dc2f256daab896dadc044dd5a913ecbb7";

#[test]
#[ignore = "requires clean exact-target Python RNS/LXMF checkouts and LXMF_PYTHON_BIN"]
fn rust_autoconnects_qualified_python_discovery_by_default() {
    run("rust-default");
}

#[test]
#[ignore = "requires clean exact-target Python RNS/LXMF checkouts and LXMF_PYTHON_BIN"]
fn python_rejects_honest_rust_discovery_with_free_slots_and_positive_control() {
    run("python-default");
}

#[test]
#[ignore = "requires clean exact-target Python RNS/LXMF checkouts and LXMF_PYTHON_BIN"]
fn python_override_autoconnects_honest_rust_discovery() {
    run("python-override");
}

fn run(case: &str) {
    let python = std::env::var("LXMF_PYTHON_BIN").expect("set LXMF_PYTHON_BIN");
    let rns = clean_reference("RETICULUM_PY_REPO", Some(TARGET_RNS));
    let lxmf = clean_reference("LXMF_PY_REPO", Some(TARGET_LXMF));
    let root =
        tempfile::Builder::new().prefix("discovery-python-").tempdir().expect("fresh topology");
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/discovery_python_process.py");
    let output = Command::new(python)
        .arg("-u")
        .arg(script)
        .arg("--case")
        .arg(case)
        .arg("--root")
        .arg(root.path())
        .arg("--daemon")
        .arg(env!("CARGO_BIN_EXE_reticulumd"))
        .env("PYTHONPATH", std::env::join_paths([&rns, &lxmf]).expect("reference import paths"))
        .env("DISCOVERY_RNS_ROOT", &rns)
        .env("DISCOVERY_LXMF_ROOT", &lxmf)
        .env("DISCOVERY_RUST_VERSION", env!("CARGO_PKG_VERSION"))
        .output()
        .expect("run native discovery process case");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("{case}\n{stdout}\n{stderr}");
    if !output.status.success() {
        let kept = root.keep();
        panic!("{case} failed: {}; artifacts retained at {}", output.status, kept.display());
    }
    assert!(stdout.contains("\"result\": \"passed\""), "missing completed case evidence");
}

fn clean_reference(variable: &str, expected: Option<&str>) -> PathBuf {
    let path = PathBuf::from(std::env::var(variable).unwrap_or_else(|_| panic!("set {variable}")))
        .canonicalize()
        .expect("reference directory exists");
    let revision = Command::new("git")
        .args(["-C", path.to_str().expect("UTF-8 reference path"), "rev-parse", "HEAD"])
        .output()
        .expect("read reference revision");
    assert!(revision.status.success());
    let revision = String::from_utf8(revision.stdout).expect("Git revision text");
    if let Some(expected) = expected {
        assert_eq!(revision.trim(), expected, "wrong {variable} target");
    }
    let status = Command::new("git")
        .args(["-C", path.to_str().expect("UTF-8 reference path"), "status", "--porcelain"])
        .output()
        .expect("check reference cleanliness");
    assert!(status.status.success());
    assert!(status.stdout.is_empty(), "{variable} must be clean");
    println!("{variable}: {} {}", path.display(), revision.trim());
    path
}

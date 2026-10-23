#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore = "requires exact clean Python references and daemon runtime; three fresh recovery runs"]
fn three_rust_relays_replace_middle_with_bidirectional_direct_packet_and_resource() {
    let rns = reference("RETICULUM_PY_REPO", "99de23c040d507e3fefca19e87b182302902725d");
    let lxmf = reference("LXMF_PY_REPO", "c3ff2d6dc2f256daab896dadc044dd5a913ecbb7");
    let python = std::env::var("LXMF_PYTHON_BIN").expect("explicit Python interpreter");
    let root =
        tempfile::Builder::new().prefix("deep-relay-recovery-").tempdir().expect("fresh roots");
    let daemon = daemon_snapshot(root.path());
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/deep_relay_recovery.py");
    let output = Command::new(python)
        .arg("-u")
        .arg(script)
        .arg("--root")
        .arg(root.path())
        .arg("--daemon")
        .arg(daemon)
        .env("PYTHONPATH", std::env::join_paths([&rns, &lxmf]).expect("reference paths"))
        .env("DEEP_RNS_ROOT", rns)
        .env("DEEP_LXMF_ROOT", lxmf)
        .output()
        .expect("run predeclared recovery matrix");
    println!("{}", String::from_utf8_lossy(&output.stdout));
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        let kept = root.keep();
        panic!("recovery matrix failed: {}; artifacts at {}", output.status, kept.display());
    }
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"verified_messages\": 24"));
}

fn daemon_snapshot(root: &std::path::Path) -> PathBuf {
    let workspace =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).expect("workspace");
    let built = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["build", "-p", "reticulumd", "--bin", "reticulumd"])
        .current_dir(workspace)
        .output()
        .expect("build current daemon");
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let test_exe = std::env::current_exe().expect("test executable");
    let target = test_exe.parent().and_then(std::path::Path::parent).expect("target directory");
    let snapshot = root.join("reticulumd");
    std::fs::copy(target.join("reticulumd"), &snapshot).expect("freeze one daemon for all runs");
    snapshot
}

fn reference(variable: &str, expected: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var(variable).unwrap_or_else(|_| panic!("set {variable}")))
        .canonicalize()
        .expect("reference directory");
    let revision = Command::new("git")
        .arg("-C")
        .arg(&path)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("read revision");
    assert!(revision.status.success());
    assert_eq!(String::from_utf8_lossy(&revision.stdout).trim(), expected);
    let status = Command::new("git")
        .arg("-C")
        .arg(&path)
        .args(["status", "--porcelain"])
        .output()
        .expect("reference status");
    assert!(status.status.success() && status.stdout.is_empty(), "{variable} must be clean");
    println!("{variable}: {} {expected}", path.display());
    path
}

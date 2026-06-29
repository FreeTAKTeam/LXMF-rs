use std::process::Command;

#[test]
fn rnpath_help_exposes_path_discovery_scaffold() {
    let output = Command::new(rnpath_bin()).arg("--help").output().expect("run rnpath-rs help");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Validate Reticulum path discovery arguments"));
    assert!(stdout.contains("not implemented yet"));
    assert!(stdout.contains("DESTINATION_HASH"));
    assert!(stdout.contains("--rpc"));
    assert!(stdout.contains("--timeout"));
}

#[test]
fn rnpath_rejects_malformed_destination_hash_before_backend_work() {
    let output =
        Command::new(rnpath_bin()).arg("not-a-destination").output().expect("run rnpath-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("destination hash must be 32 hexadecimal characters"));
}

#[test]
fn rnpath_documents_missing_daemon_path_backend_without_connecting() {
    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--rpc")
        .arg("127.0.0.1:1")
        .arg("--timeout")
        .arg("5")
        .output()
        .expect("run rnpath-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("path discovery for 00112233445566778899aabbccddeeff"));
    assert!(stderr.contains("is not wired to daemon RPC yet"));
    assert!(stderr.contains("timeout=5s"));
}

fn rnpath_bin() -> String {
    env!("CARGO_BIN_EXE_rnpath-rs").to_string()
}

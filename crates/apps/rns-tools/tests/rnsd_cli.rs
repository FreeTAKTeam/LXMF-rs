use std::process::Command;

#[test]
fn rnsd_honors_reticulumd_bin_and_forwards_help_output() {
    let output = Command::new(rnsd_bin())
        .env("RETICULUMD_BIN", rnpath_bin())
        .arg("--help")
        .output()
        .expect("run rnsd delegated help");

    assert!(output.status.success(), "status={:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Request Reticulum path discovery through daemon RPC."));
    let delegated_name = if cfg!(windows) { "rnpath-rs.exe" } else { "rnpath-rs" };
    assert!(stdout.contains(&format!("Usage: {delegated_name} [OPTIONS] <DESTINATION_HASH>")));
}

#[test]
fn rnsd_preserves_delegated_failure_status_and_stderr() {
    let output = Command::new(rnsd_bin())
        .env("RETICULUMD_BIN", rnpath_bin())
        .arg("not-a-destination")
        .output()
        .expect("run rnsd delegated failure");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("destination hash must be 32 hexadecimal characters"));
}

fn rnsd_bin() -> String {
    env!("CARGO_BIN_EXE_rnsd").to_string()
}

fn rnpath_bin() -> String {
    env!("CARGO_BIN_EXE_rnpath-rs").to_string()
}

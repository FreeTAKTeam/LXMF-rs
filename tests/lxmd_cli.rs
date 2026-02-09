#[test]
fn lxmd_help_has_expected_flags() {
    let output = std::process::Command::new("cargo")
        .args(["run", "--bin", "lxmd", "--", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--config"));
    assert!(stdout.contains("--propagation-node"));
    assert!(stdout.contains("status"));
}

#[test]
fn lxmd_status_command_runs() {
    let output = std::process::Command::new("cargo")
        .args(["run", "--bin", "lxmd", "--", "status"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("lxmd status"));
}

#[test]
fn lxmd_service_mode_runs_with_max_ticks_env() {
    let output = std::process::Command::new("cargo")
        .env("LXMD_SERVICE_MAX_TICKS", "2")
        .args(["run", "--bin", "lxmd", "--", "--service"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("lxmd service done"));
}

use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore = "requires the pinned Python Reticulum checkout"]
fn rnsd_example_config_matches_pinned_python_without_starting_daemon() {
    let repo = PathBuf::from(
        std::env::var_os("RETICULUM_PY_REPO")
            .expect("RETICULUM_PY_REPO must point to the pinned Python checkout"),
    );
    let script = repo.join("RNS/Utilities/rnsd.py");
    assert!(script.is_file(), "pinned Python rnsd script missing: {}", script.display());

    let python = Command::new(std::env::var_os("PYTHON").unwrap_or_else(|| "python3".into()))
        .arg(script)
        .arg("--exampleconfig")
        .env("PYTHONPATH", &repo)
        .output()
        .expect("run pinned Python rnsd --exampleconfig");
    assert!(
        python.status.success(),
        "pinned Python rnsd failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );

    let rust = Command::new(env!("CARGO_BIN_EXE_rnsd"))
        .arg("--exampleconfig")
        .env("RETICULUMD_BIN", "/nonexistent/reticulumd-for-example-config-test")
        .output()
        .expect("run Rust rnsd --exampleconfig");

    assert_eq!(rust.status.code(), Some(0));
    assert_eq!(
        rust.stdout, python.stdout,
        "Rust and pinned Python example configuration output differ"
    );
    assert!(rust.stderr.is_empty(), "Rust stderr: {}", String::from_utf8_lossy(&rust.stderr));
}

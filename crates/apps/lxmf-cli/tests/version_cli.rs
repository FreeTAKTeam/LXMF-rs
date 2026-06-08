use std::path::Path;
use std::process::Command;

const PYTHON_LXMF_VERSION: &str = "0.9.6";
const PYTHON_LXMF_REF: &str = "727830cefda83d9c6e3982b48675425f3f988f9c";
const PYTHON_RETICULUM_VERSION: &str = "1.2.2";
const PYTHON_RETICULUM_REF: &str = "15320e4d2cfabb143c1db20ca887e275fd521585";

fn project_version() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../VERSION");
    std::fs::read_to_string(path).expect("read project VERSION").trim().to_string()
}

fn assert_version_output(binary: &str) {
    let output = Command::new(binary).arg("--version").output().expect("run binary --version");
    assert!(
        output.status.success(),
        "--version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("version output is UTF-8");
    assert_eq!(
        stdout,
        format!(
            "lxmf-rs {}\n\
             lxmf-cli crate {}\n\
             python-reticulum reference {PYTHON_RETICULUM_VERSION} {PYTHON_RETICULUM_REF}\n\
             python-lxmf reference {PYTHON_LXMF_VERSION} {PYTHON_LXMF_REF}\n",
            project_version(),
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn lxmf_prints_project_crate_and_reference_versions() {
    assert_version_output(
        &std::env::var("CARGO_BIN_EXE_lxmf").expect("CARGO_BIN_EXE_lxmf set by cargo"),
    );
}

#[test]
fn lxmd_prints_project_crate_and_reference_versions() {
    assert_version_output(
        &std::env::var("CARGO_BIN_EXE_lxmd").expect("CARGO_BIN_EXE_lxmd set by cargo"),
    );
}

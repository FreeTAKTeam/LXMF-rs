use std::path::{Path, PathBuf};
use std::process::Command;

const PYTHON_LXMF_VERSION: &str = "0.9.6";
const PYTHON_LXMF_REF: &str = "727830cefda83d9c6e3982b48675425f3f988f9c";
const PYTHON_RETICULUM_VERSION: &str = "1.5.2";
const PYTHON_RETICULUM_REF: &str = "ea98db4f53dcf0defc0e71a16e60d28b1229c4e6";

fn project_version() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../VERSION");
    std::fs::read_to_string(path).expect("read project VERSION").trim().to_string()
}

fn expected_version_output() -> String {
    format!(
        "lxmf-rs {}\n\
         lxmf-cli crate {}\n\
         python-reticulum reference {PYTHON_RETICULUM_VERSION} {PYTHON_RETICULUM_REF}\n\
         python-lxmf reference {PYTHON_LXMF_VERSION} {PYTHON_LXMF_REF}\n",
        project_version(),
        env!("CARGO_PKG_VERSION")
    )
}

fn resolve_test_binary(name: &str, provided: Option<&str>) -> PathBuf {
    if let Some(path) = provided.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }

    let current_exe = std::env::current_exe().expect("current test executable path");
    let deps_dir = current_exe.parent().expect("test executable parent");
    let target_dir = deps_dir.parent().expect("target debug dir");
    let mut candidates = vec![target_dir.join(name)];
    if !std::env::consts::EXE_SUFFIX.is_empty() {
        candidates.push(target_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)));
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| panic!("failed to locate {name} test binary"))
}

fn assert_version_output(binary: &Path) {
    let output = Command::new(binary).arg("--version").output().expect("run binary --version");
    assert!(
        output.status.success(),
        "--version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("version output is UTF-8");
    assert_eq!(stdout, expected_version_output());
}

#[test]
fn lxmf_prints_project_crate_and_reference_versions() {
    let binary = resolve_test_binary("lxmf", option_env!("CARGO_BIN_EXE_lxmf"));
    assert_version_output(&binary);
}

#[test]
fn lxmd_prints_project_crate_and_reference_versions() {
    let binary = resolve_test_binary("lxmd", option_env!("CARGO_BIN_EXE_lxmd"));
    assert_version_output(&binary);
}

#[test]
fn lxmf_does_not_print_version_when_token_is_argument_data() {
    let binary = resolve_test_binary("lxmf", option_env!("CARGO_BIN_EXE_lxmf"));
    let output = Command::new(binary)
        .args(["--rpc", "--version", "snapshot"])
        .output()
        .expect("run binary with version-looking rpc value");

    assert_ne!(
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        expected_version_output()
    );
}

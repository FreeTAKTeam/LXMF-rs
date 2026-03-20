use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompatibilityCase {
    id: &'static str,
    description: &'static str,
}

const HARNESS_CASES: [CompatibilityCase; 3] = [
    CompatibilityCase { id: "direct", description: "Direct mixed Rust/Python delivery path" },
    CompatibilityCase {
        id: "opportunistic",
        description: "Opportunistic mixed Rust/Python delivery path",
    },
    CompatibilityCase {
        id: "propagated_resource_lxm",
        description: "Propagation path with resource-sized payload and .lxm lifecycle checks",
    },
];

#[test]
fn compatibility_matrix_covers_first_slice() {
    assert_eq!(HARNESS_CASES.len(), 3);
    assert_case_present("direct");
    assert_case_present("opportunistic");
    assert_case_present("propagated_resource_lxm");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_direct() {
    run_case("direct");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_opportunistic() {
    run_case("opportunistic");
}

#[test]
#[ignore = "pending remaining propagation, resource, and .lxm parity work"]
fn python_compat_propagated_resource_lxm() {
    run_case("propagated_resource_lxm");
}

fn run_case(case_id: &str) {
    let script = smoke_script_path();
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let python_path = effective_python_path();
    ensure_environment(&script, &python_bin, python_path.as_deref()).unwrap_or_else(|reason| {
        panic!("python compatibility harness unavailable for '{case_id}': {reason}")
    });

    let mut cmd = Command::new("bash");
    cmd.arg(&script).arg("--scenario").arg(case_id);
    cmd.env("PYTHON_BIN", &python_bin);
    cmd.env(
        "TIMEOUT_SECS",
        env::var("LXMF_PY_COMPAT_TIMEOUT").unwrap_or_else(|_| "90".to_string()),
    );
    if let Some(path) = python_path {
        cmd.env("PYTHONPATH", path);
    }

    let output = cmd.output().expect("failed to execute smoke script");
    if !output.status.success() {
        panic!(
            "python compatibility case '{}' failed\nstdout:\n{}\nstderr:\n{}",
            case_id,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn smoke_script_path() -> PathBuf {
    env::var("LXMF_PY_COMPAT_SMOKE").map(PathBuf::from).unwrap_or_else(|_| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tools/scripts/python-lxmd-rust-lxmd-smoke.sh")
    })
}

fn effective_python_path() -> Option<String> {
    let from_env = env::var("PYTHONPATH").ok().filter(|v| !v.trim().is_empty());
    let repo_root = match Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize() {
        Ok(path) => path,
        Err(_) => return from_env,
    };
    let parent = match repo_root.parent() {
        Some(parent) => parent,
        None => return from_env,
    };
    let reticulum = parent.join("Reticulum");
    let lxmf = parent.join("LXMF");
    if !reticulum.exists() || !lxmf.exists() {
        return from_env;
    }
    Some(match from_env {
        Some(existing) => format!("{existing}:{}:{}", reticulum.display(), lxmf.display()),
        None => format!("{}:{}", reticulum.display(), lxmf.display()),
    })
}

fn ensure_environment(
    script: &Path,
    python_bin: &str,
    python_path: Option<&str>,
) -> Result<(), String> {
    if !script.exists() {
        return Err(format!("missing script at {}", script.display()));
    }

    let mut cmd = Command::new(python_bin);
    cmd.arg("-c")
        .arg("import importlib.util,sys;missing=[m for m in ('RNS','LXMF') if importlib.util.find_spec(m) is None];sys.exit(0 if not missing else 1)");
    if let Some(path) = python_path {
        cmd.env("PYTHONPATH", path);
    }
    let status = cmd
        .status()
        .map_err(|error| format!("unable to run python '{}': {}", python_bin, error))?;
    if !status.success() {
        return Err(
            "missing Python modules RNS/LXMF; set PYTHONPATH or install editable checkouts"
                .to_string(),
        );
    }
    Ok(())
}

fn assert_case_present(case_id: &str) {
    assert!(
        HARNESS_CASES.iter().any(|case| case.id == case_id && !case.description.is_empty()),
        "missing compatibility case '{}'",
        case_id
    );
}

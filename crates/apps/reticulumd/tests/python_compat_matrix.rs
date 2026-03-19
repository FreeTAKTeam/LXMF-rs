use std::env;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompatibilityMode {
    Direct,
    Opportunistic,
    Propagated,
    Resource,
    LxmInterchange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompatibilityCase {
    id: &'static str,
    mode: CompatibilityMode,
    description: &'static str,
}

const COMPATIBILITY_CASES: [CompatibilityCase; 5] = [
    CompatibilityCase {
        id: "direct_delivery",
        mode: CompatibilityMode::Direct,
        description: "Rust node can deliver to Python node using direct mode",
    },
    CompatibilityCase {
        id: "opportunistic_delivery",
        mode: CompatibilityMode::Opportunistic,
        description: "Rust node can deliver to Python node using opportunistic mode",
    },
    CompatibilityCase {
        id: "propagated_delivery",
        mode: CompatibilityMode::Propagated,
        description: "Rust node can deliver to Python node through selected propagation node",
    },
    CompatibilityCase {
        id: "resource_transfer",
        mode: CompatibilityMode::Resource,
        description: "Python-originated resource payload is accepted by Rust daemon",
    },
    CompatibilityCase {
        id: "lxm_interchange",
        mode: CompatibilityMode::LxmInterchange,
        description: "Python .lxm storage payload round-trips through Rust decode/encode path",
    },
];

#[test]
fn compatibility_matrix_covers_required_modes() {
    assert_eq!(COMPATIBILITY_CASES.len(), 5, "matrix should include first five required modes");
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Direct));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Opportunistic));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Propagated));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Resource));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::LxmInterchange));
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_direct_delivery() {
    run_case("direct_delivery");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_opportunistic_delivery() {
    run_case("opportunistic_delivery");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_propagated_delivery() {
    run_case("propagated_delivery");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_resource_transfer() {
    run_case("resource_transfer");
}

#[test]
#[ignore = "requires live Python compatibility harness environment"]
fn python_compat_lxm_interchange() {
    run_case("lxm_interchange");
}

fn run_case(case_id: &str) {
    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let harness = env::var("LXMF_PY_COMPAT_HARNESS").expect(
        "set LXMF_PY_COMPAT_HARNESS to a Python harness script path before running ignored tests",
    );

    let output = Command::new(python_bin)
        .arg(harness)
        .arg(case_id)
        .output()
        .expect("failed to execute python compatibility harness");

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "python compatibility case '{}' failed\nstdout:\n{}\nstderr:\n{}",
            case_id, stdout, stderr
        );
    }
}

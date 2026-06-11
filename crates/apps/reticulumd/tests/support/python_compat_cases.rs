use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

static PY_COMPAT_HARNESS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompatibilityMode {
    Direct,
    Opportunistic,
    Propagated,
    PropagationControl,
    LinkLifecycle,
    Resource,
    LxmInterchange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompatibilityCase {
    id: &'static str,
    mode: CompatibilityMode,
    description: &'static str,
}

const COMPATIBILITY_CASES: [CompatibilityCase; 13] = [
    CompatibilityCase {
        id: "direct_rust_to_python",
        mode: CompatibilityMode::Direct,
        description: "Rust node can deliver to Python node using direct mode",
    },
    CompatibilityCase {
        id: "direct_python_to_rust",
        mode: CompatibilityMode::Direct,
        description: "Python node can deliver to Rust node using direct mode",
    },
    CompatibilityCase {
        id: "opportunistic_python_to_rust",
        mode: CompatibilityMode::Opportunistic,
        description: "Python node can deliver to Rust node using opportunistic mode",
    },
    CompatibilityCase {
        id: "opportunistic_rust_to_python",
        mode: CompatibilityMode::Opportunistic,
        description: "Rust node can deliver to Python node using opportunistic mode",
    },
    CompatibilityCase {
        id: "propagated_rust_to_python",
        mode: CompatibilityMode::Propagated,
        description: "Rust node can deliver to Python node through selected propagation node",
    },
    CompatibilityCase {
        id: "propagated_python_to_rust",
        mode: CompatibilityMode::Propagated,
        description: "Python node can deliver to Rust node through a Rust propagation node",
    },
    CompatibilityCase {
        id: "propagation_remote_status_bidir",
        mode: CompatibilityMode::PropagationControl,
        description: "Python can resolve Rust propagation control and Rust can query Python propagation status",
    },
    CompatibilityCase {
        id: "link_liveness_rust_to_python",
        mode: CompatibilityMode::LinkLifecycle,
        description: "Rust-initiated direct links stay alive with adaptive keepalives and time out like Python",
    },
    CompatibilityCase {
        id: "link_liveness_python_to_rust",
        mode: CompatibilityMode::LinkLifecycle,
        description: "Python-initiated direct links stay alive with adaptive keepalives and time out like Rust",
    },
    CompatibilityCase {
        id: "link_teardown_rust_to_python",
        mode: CompatibilityMode::LinkLifecycle,
        description: "Rust watchdog teardown emits a protocol close packet Python accepts as a remote close",
    },
    CompatibilityCase {
        id: "link_teardown_python_to_rust",
        mode: CompatibilityMode::LinkLifecycle,
        description: "Python manual teardown closes the Rust link through the protocol close path",
    },
    CompatibilityCase {
        id: "resource_transfer",
        mode: CompatibilityMode::Resource,
        description: "Rust and Python nodes can exchange resource payloads on shared links",
    },
    CompatibilityCase {
        id: "lxm_interchange",
        mode: CompatibilityMode::LxmInterchange,
        description: "Python .lxm storage payload round-trips through Rust decode/encode path",
    },
];

pub(crate) fn assert_required_modes_covered() {
    assert!(
        COMPATIBILITY_CASES.len() >= 13,
        "matrix should cover the documented required scenarios"
    );
    assert_case_present("direct_rust_to_python");
    assert_case_present("direct_python_to_rust");
    assert_case_present("opportunistic_python_to_rust");
    assert_case_present("opportunistic_rust_to_python");
    assert_case_present("propagated_rust_to_python");
    assert_case_present("propagated_python_to_rust");
    assert_case_present("propagation_remote_status_bidir");
    assert_case_present("link_liveness_rust_to_python");
    assert_case_present("link_liveness_python_to_rust");
    assert_case_present("link_teardown_rust_to_python");
    assert_case_present("link_teardown_python_to_rust");
    assert_case_present("resource_transfer");
    assert_case_present("lxm_interchange");
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Direct));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Opportunistic));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Propagated));
    assert!(COMPATIBILITY_CASES
        .iter()
        .any(|case| case.mode == CompatibilityMode::PropagationControl));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::LinkLifecycle));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Resource));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::LxmInterchange));
}

pub(crate) fn run_case(case_id: &str) {
    let _guard = PY_COMPAT_HARNESS_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
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

pub(crate) fn assert_cases_are_dispatchable_by_harness_and_smoke_script() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let harness = fs::read_to_string(repo_root.join("tools/scripts/python_compat_harness.py"))
        .expect("python harness should be readable");
    let smoke = fs::read_to_string(repo_root.join("tools/scripts/python-lxmd-rust-lxmd-smoke.sh"))
        .expect("smoke dispatcher should be readable");

    for case in COMPATIBILITY_CASES {
        let case_literal = format!("\"{}\"", case.id);
        assert!(
            harness.contains(&case_literal),
            "python harness does not advertise compatibility case '{}'",
            case.id
        );
        assert!(
            smoke.contains(case.id),
            "smoke dispatcher does not reference compatibility case '{}'",
            case.id
        );
    }
}

pub(crate) fn assert_smoke_rpc_call_retries_transient_connection_refusals() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let smoke = fs::read_to_string(repo_root.join("tools/scripts/python-lxmd-rust-lxmd-smoke.sh"))
        .expect("smoke dispatcher should be readable");

    assert!(
        smoke.contains("except OSError as exc:"),
        "smoke rpc_call should catch transient socket failures before exhausting retries"
    );
    assert!(
        smoke.contains("ConnectionRefusedError"),
        "smoke rpc_call should retry connection refusals while Rust RPC starts accepting"
    );
}

fn assert_case_present(case_id: &str) {
    assert!(
        COMPATIBILITY_CASES.iter().any(|case| case.id == case_id && !case.description.is_empty()),
        "missing compatibility case '{}'",
        case_id
    );
}

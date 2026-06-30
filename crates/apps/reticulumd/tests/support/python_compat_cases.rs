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
    PathDiscovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompatibilityCase {
    id: &'static str,
    mode: CompatibilityMode,
    description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalEvidenceCase {
    id: &'static str,
    test_target: &'static str,
    harness_filter: &'static str,
    description: &'static str,
}

const COMPATIBILITY_CASES: [CompatibilityCase; 23] = [
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
        id: "propagation_remote_fetch_rust_to_python",
        mode: CompatibilityMode::PropagationControl,
        description: "Rust remote fetch can import Python propagation-node payloads",
    },
    CompatibilityCase {
        id: "propagation_remote_download_rust_to_python",
        mode: CompatibilityMode::PropagationControl,
        description: "Rust remote download can import Python propagation-node payloads",
    },
    CompatibilityCase {
        id: "propagation_remote_sync_rust_to_python",
        mode: CompatibilityMode::PropagationControl,
        description: "Rust remote sync can trigger a seeded Python LXMRouter peer sync transfer",
    },
    CompatibilityCase {
        id: "propagation_get_haves_python_to_rust",
        mode: CompatibilityMode::PropagationControl,
        description: "Python-origin propagation get haves exercise Rust purge and retry suppression",
    },
    CompatibilityCase {
        id: "propagation_offer_python_to_rust",
        mode: CompatibilityMode::PropagationControl,
        description: "Python-origin propagation offers exercise Rust offer side effects and throttling",
    },
    CompatibilityCase {
        id: "propagation_offer_queue_python_to_rust",
        mode: CompatibilityMode::PropagationControl,
        description: "Python-origin propagation offers exercise Rust peer queue lifecycle state",
    },
    CompatibilityCase {
        id: "propagation_offer_duplicate_wanted_source_completed_python_to_rust",
        mode: CompatibilityMode::PropagationControl,
        description: "Python-origin propagation offers exercise Rust duplicate wanted-ID and source-completed state",
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
    CompatibilityCase {
        id: "rns_path_request_rust_to_python",
        mode: CompatibilityMode::PathDiscovery,
        description: "Rust daemon path RPC resolves a Python Reticulum destination over loopback TCP",
    },
    CompatibilityCase {
        id: "rns_path_request_rust_to_python_scoped_refresh",
        mode: CompatibilityMode::PathDiscovery,
        description: "Rust rnpath-rs reissues a scoped tagged path request for a learned Python route",
    },
    CompatibilityCase {
        id: "rns_path_request_python_to_rust",
        mode: CompatibilityMode::PathDiscovery,
        description: "Python Reticulum path request resolves a Rust daemon destination over loopback TCP",
    },
];

const LOCAL_EVIDENCE_CASES: [LocalEvidenceCase; 6] = [
    LocalEvidenceCase {
        id: "rns_path_request_transport_policy",
        test_target: "transport_policy_evidence",
        harness_filter: "transport_policy_evidence",
        description: "Deterministic local transport evidence for scoped path-request dispatch and known-path PATH_RESPONSE ordering",
    },
    LocalEvidenceCase {
        id: "rns_path_request_roaming_transport_policy",
        test_target: "transport_policy_evidence",
        harness_filter: "roaming_same_iface_known_path_request_is_suppressed_at_transport_boundary",
        description: "Deterministic local transport evidence for Transport.py roaming same-iface path-response suppression",
    },
    LocalEvidenceCase {
        id: "rns_path_request_roaming_grace_transport_policy",
        test_target: "transport_policy_evidence",
        harness_filter: "roaming_diff_iface_known_path_response_waits_extra_grace_at_transport_boundary",
        description: "Deterministic local transport evidence for Transport.py roaming different-iface path-response grace",
    },
    LocalEvidenceCase {
        id: "rns_announce_rebroadcast_transport_policy",
        test_target: "transport_policy_evidence",
        harness_filter: "announce_rebroadcast_policy_uses_learned_next_hop_mode_at_transport_boundary",
        description: "Deterministic local transport evidence for Transport.py announce rebroadcast interface-mode policy",
    },
    LocalEvidenceCase {
        id: "rns_unknown_announce_ingress_policy",
        test_target: "reticulum-rs-transport",
        harness_filter: "held_announces_release_one_lowest_hop_entry_per_interface",
        description: "Deterministic local transport evidence for Transport.py per-interface unknown-announce holding and lowest-hop release policy",
    },
    LocalEvidenceCase {
        id: "rns_link_request_mtu_transport_policy",
        test_target: "reticulum-rs-transport",
        harness_filter: "mtu_signalling",
        description: "Deterministic local transport evidence for Transport.py intermediate LINKREQUEST MTU signalling rewrite policy",
    },
];

pub(crate) fn assert_required_modes_covered() {
    assert!(
        COMPATIBILITY_CASES.len() >= 23,
        "matrix should cover the documented required scenarios"
    );
    assert_case_present("direct_rust_to_python");
    assert_case_present("direct_python_to_rust");
    assert_case_present("opportunistic_python_to_rust");
    assert_case_present("opportunistic_rust_to_python");
    assert_case_present("propagated_rust_to_python");
    assert_case_present("propagated_python_to_rust");
    assert_case_present("propagation_remote_status_bidir");
    assert_case_present("propagation_remote_fetch_rust_to_python");
    assert_case_present("propagation_remote_download_rust_to_python");
    assert_case_present("propagation_remote_sync_rust_to_python");
    assert_case_present("propagation_get_haves_python_to_rust");
    assert_case_present("propagation_offer_python_to_rust");
    assert_case_present("propagation_offer_queue_python_to_rust");
    assert_case_present("propagation_offer_duplicate_wanted_source_completed_python_to_rust");
    assert_case_present("link_liveness_rust_to_python");
    assert_case_present("link_liveness_python_to_rust");
    assert_case_present("link_teardown_rust_to_python");
    assert_case_present("link_teardown_python_to_rust");
    assert_case_present("resource_transfer");
    assert_case_present("lxm_interchange");
    assert_case_present("rns_path_request_rust_to_python");
    assert_case_present("rns_path_request_rust_to_python_scoped_refresh");
    assert_case_present("rns_path_request_python_to_rust");
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Direct));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Opportunistic));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Propagated));
    assert!(COMPATIBILITY_CASES
        .iter()
        .any(|case| case.mode == CompatibilityMode::PropagationControl));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::LinkLifecycle));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::Resource));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::LxmInterchange));
    assert!(COMPATIBILITY_CASES.iter().any(|case| case.mode == CompatibilityMode::PathDiscovery));
}

pub(crate) fn assert_local_evidence_cases_are_dispatchable_by_harness() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let harness = fs::read_to_string(repo_root.join("tools/scripts/python_compat_harness.py"))
        .expect("python harness should be readable");

    for case in LOCAL_EVIDENCE_CASES {
        let case_literal = format!("\"{}\"", case.id);
        let test_target_literal = format!("\"{}\"", case.test_target);
        let harness_filter_literal = format!("\"{}\"", case.harness_filter);
        assert!(
            !case.description.is_empty(),
            "local evidence case '{}' should describe the deterministic evidence",
            case.id
        );
        assert!(
            harness.contains(&case_literal),
            "python harness does not advertise local evidence case '{}'",
            case.id
        );
        let case_start = harness.find(&case_literal).unwrap_or_else(|| {
            panic!("python harness does not advertise local evidence case '{}'", case.id)
        });
        let case_block = &harness[case_start..harness.len().min(case_start + 512)];
        assert!(
            case_block.contains(&test_target_literal),
            "python harness does not dispatch local evidence case '{}' to '{}'",
            case.id,
            case.test_target
        );
        assert!(
            case_block.contains(&harness_filter_literal),
            "python harness does not dispatch local evidence case '{}' with filter '{}'",
            case.id,
            case.harness_filter
        );
    }
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

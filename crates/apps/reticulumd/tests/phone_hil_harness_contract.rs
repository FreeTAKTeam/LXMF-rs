use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn two_phone_hil_harness_preserves_required_field_test_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/phone-reticulumd-hil.sh");
    let script = fs::read_to_string(&script_path).expect("read phone HIL harness");

    for required in [
        "target/phone-hil",
        "adb devices -l",
        "adb reverse",
        "tcp:37429",
        "SIDE_BAND_REVERSE_PORT",
        "PIXEL_REVERSE_PORT",
        "AUTO_DISCOVER_PHONE_HASHES",
        "RUST_LOG=\"reticulumd=trace,reticulum_rs_transport=trace\"",
        "propagation destination hash=",
        "DAEMON_PROPAGATION_HASH",
        "get_outbound_propagation_cost",
        "message_delivery_trace",
        "sdk_status_v2",
        "sdk_snapshot_v2",
        "list_messages",
        "list_peers",
        "wait_for_phone_peer",
        "phone_peer_readiness",
        "propagation_peer_maintenance",
        "unsupported-by-phone-app",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "phone HIL harness should include required contract token {required:?}"
        );
    }
}

#[test]
fn two_phone_hil_runbook_documents_manual_phone_inputs_and_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/two-phone-reticulumd-hil.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read two-phone HIL runbook");

    for required in [
        "S8",
        "Sideband",
        "Pixel",
        "Columba",
        "SIDE_BAND_HASH",
        "COLUMBA_HASH",
        "target/phone-hil/<timestamp>/",
        "unsupported-by-phone-app",
    ] {
        assert!(
            runbook.contains(required),
            "two-phone HIL runbook should document required token {required:?}"
        );
    }
}

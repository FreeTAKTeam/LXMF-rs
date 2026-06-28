use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn i2p_prepared_host_smoke_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/i2p-prepared-host-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read I2P prepared-host smoke script");

    for required in [
        "target/i2p-hil",
        "SAM_HOST",
        "SAM_PORT",
        "I2P_PEERS",
        "HELLO VERSION MIN=3.0 MAX=3.3",
        "[[interfaces]]",
        "I2PInterface",
        "connectable = true",
        "peers = [",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnstatus_json",
        "evidence_scope",
        "sam_connectable_only",
        "sam_connectable_with_outbound_peers",
        "product_boundary",
        "not outbound peer production parity",
        "reachable_endpoint",
        "private_key_persisted",
        "accept_state",
        "listening",
        "configured_peer_count",
        "expected_outbound_peers",
        "connected_outbound_peers",
        "direction",
        "outbound",
        "connected",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "I2P prepared-host smoke should include required token {required:?}"
        );
    }
}

#[test]
fn i2p_prepared_host_pair_smoke_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/i2p-prepared-host-pair-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read I2P prepared-host pair script");

    for required in [
        "target/i2p-hil-pair",
        "SAM_HOST",
        "SAM_PORT",
        "I2P_PAIR_SOAK_SECS",
        "I2P_PAIR_SOAK_POLL_SECS",
        "HELLO VERSION MIN=3.0 MAX=3.3",
        "i2p-pair-acceptor",
        "i2p-pair-dialer",
        "I2PInterface",
        "connectable = true",
        "peers = [",
        "--strict-interface-startup",
        "rnstatus-rs",
        "acceptor_rnstatus_json",
        "dialer_rnstatus_json",
        "sam_connectable_with_outbound_peers_real_pair",
        "sam_connectable_with_outbound_peers_real_pair_soak",
        "soak_requested_secs",
        "soak_samples",
        "soak_samples_jsonl",
        "product_boundary",
        "broader public",
        "I2P peer-set",
        "long-running production evidence",
        "reachable_endpoint",
        "private_key_persisted",
        "accept_state",
        "listening",
        "configured_peer_count",
        "connected_outbound_peers",
        "direction",
        "incoming",
        "outbound",
        "connected",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "I2P prepared-host pair smoke should include required token {required:?}"
        );
    }
}

#[test]
fn i2p_fake_sam_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/i2p-fake-sam-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read I2P fake-SAM smoke script");

    for required in [
        "target/i2p-fake-sam-smoke",
        "I2P_PEERS",
        "HELLO VERSION MIN=3.0 MAX=3.3",
        "DEST GENERATE SIGNATURE_TYPE=7",
        "SESSION CREATE",
        "NAMING LOOKUP NAME=",
        "STREAM CONNECT",
        "STREAM ACCEPT",
        "[[interfaces]]",
        "I2PInterface",
        "i2p-fake-sam",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnstatus_json",
        "rnstatus_human",
        "software_fake_sam_i2p_runtime",
        "product_boundary",
        "prepared I2P router",
        "reachable_endpoint",
        "private_key_persisted",
        "accept_state",
        "listening",
        "configured_peer_count",
        "connected_outbound_peers",
        "recovered_outbound_peers",
        "connected_incoming_peers",
        "fake-remote-destination",
        "transient-lookup-failure",
        "reconnect_backoff_ms",
        "reconnect_attempts",
        "last_error",
        "incoming=1",
        "direction",
        "outbound",
        "incoming",
        "connected",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "I2P fake-SAM smoke should include required token {required:?}"
        );
    }
}

#[test]
fn nightly_hil_workflow_exposes_i2p_prepared_host_job() {
    let root = repo_root();
    let workflow_path = root.join(".github/workflows/nightly-embedded-hil.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "i2p-prepared-host",
        "i2p-prepared-host-pair",
        "HIL_I2P_ENABLED",
        "HIL_I2P_PAIR_ENABLED",
        "HIL_I2P_SAM_HOST",
        "HIL_I2P_SAM_PORT",
        "HIL_I2P_PEERS",
        "HIL_I2P_TIMEOUT_SECS",
        "HIL_I2P_PAIR_SAM_HOST",
        "HIL_I2P_PAIR_SAM_PORT",
        "HIL_I2P_PAIR_TIMEOUT_SECS",
        "HIL_I2P_PAIR_SOAK_SECS",
        "HIL_I2P_PAIR_SOAK_POLL_SECS",
        "./tools/scripts/i2p-prepared-host-smoke.sh",
        "./tools/scripts/i2p-prepared-host-pair-smoke.sh",
        "i2p-prepared-host-artifacts",
        "i2p-prepared-host-pair-artifacts",
        "target/i2p-hil/report.json",
        "target/i2p-hil/run.*",
        "target/i2p-hil-pair/report.json",
        "target/i2p-hil-pair/run.*",
    ] {
        assert!(
            workflow.contains(required),
            "nightly HIL workflow should include required token {required:?}"
        );
    }
}

#[test]
fn i2p_runbook_documents_prepared_host_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-i2p-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read I2P runbook");

    for required in [
        "Prepared-Host Smoke",
        "Software Fake-SAM Smoke",
        "./tools/scripts/i2p-prepared-host-smoke.sh",
        "./tools/scripts/i2p-prepared-host-pair-smoke.sh",
        "./tools/scripts/i2p-fake-sam-smoke.sh",
        "SAM_HOST=127.0.0.1",
        "I2P_PEERS=peer-one.b32.i2p",
        "--strict-interface-startup",
        "_runtime.i2p.reachable_endpoint",
        "_runtime.i2p.tunnel_status.accept_state = \"listening\"",
        "_runtime.i2p.tunnel_status.configured_peer_count",
        "connected outbound peer rows",
        "recovered outbound peer rows",
        "reconnect_attempts >= 1",
        "last_error = null",
        "fails the first",
        "`NAMING LOOKUP`",
        "connected incoming peer row",
        "incoming=1",
        "target/i2p-fake-sam-smoke/",
        "software_fake_sam_i2p_runtime",
        "target/i2p-hil/",
        "report.json",
        "evidence_scope",
        "sam_connectable_only",
        "sam_connectable_with_outbound_peers",
        "sam_connectable_with_outbound_peers_real_pair",
        "sam_connectable_with_outbound_peers_real_pair_soak",
        "target/i2p-hil-pair/",
        "I2P_PAIR_SOAK_SECS",
        "HIL_I2P_PAIR_ENABLED",
        "HIL_I2P_PAIR_SAM_HOST",
        "HIL_I2P_PAIR_SOAK_SECS",
        "i2p-prepared-host-pair-artifacts",
        "soak_samples",
        "connected incoming peer row",
        "broader public peer-set parity",
        "product_boundary",
        "not outbound peer production parity",
        "HIL_I2P_ENABLED",
    ] {
        assert!(
            runbook.contains(required),
            "I2P runbook should document required token {required:?}"
        );
    }
}

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
        "I2PInterface",
        "connectable = true",
        "peers = [",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnstatus_json",
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
fn nightly_hil_workflow_exposes_i2p_prepared_host_job() {
    let root = repo_root();
    let workflow_path = root.join(".github/workflows/nightly-embedded-hil.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "i2p-prepared-host",
        "HIL_I2P_ENABLED",
        "HIL_I2P_SAM_HOST",
        "HIL_I2P_SAM_PORT",
        "HIL_I2P_PEERS",
        "HIL_I2P_TIMEOUT_SECS",
        "./tools/scripts/i2p-prepared-host-smoke.sh",
        "i2p-prepared-host-artifacts",
        "target/i2p-hil/report.json",
        "target/i2p-hil/run.*",
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
        "./tools/scripts/i2p-prepared-host-smoke.sh",
        "SAM_HOST=127.0.0.1",
        "I2P_PEERS=peer-one.b32.i2p",
        "--strict-interface-startup",
        "_runtime.i2p.reachable_endpoint",
        "_runtime.i2p.tunnel_status.accept_state = \"listening\"",
        "_runtime.i2p.tunnel_status.configured_peer_count",
        "connected outbound peer rows",
        "target/i2p-hil/",
        "report.json",
        "HIL_I2P_ENABLED",
    ] {
        assert!(
            runbook.contains(required),
            "I2P runbook should document required token {required:?}"
        );
    }
}

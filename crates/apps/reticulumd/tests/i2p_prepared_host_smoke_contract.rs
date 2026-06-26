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
        "HELLO VERSION MIN=3.0 MAX=3.3",
        "I2PInterface",
        "connectable = true",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnstatus_json",
        "reachable_endpoint",
        "private_key_persisted",
        "accept_state",
        "listening",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "I2P prepared-host smoke should include required token {required:?}"
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
        "--strict-interface-startup",
        "_runtime.i2p.reachable_endpoint",
        "_runtime.i2p.tunnel_status.accept_state = \"listening\"",
        "target/i2p-hil/",
        "report.json",
    ] {
        assert!(
            runbook.contains(required),
            "I2P runbook should document required token {required:?}"
        );
    }
}

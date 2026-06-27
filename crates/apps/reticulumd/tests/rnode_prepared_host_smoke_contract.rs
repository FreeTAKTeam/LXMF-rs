use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn rnode_prepared_host_smoke_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/rnode-prepared-host-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read RNode prepared-host smoke script");

    for required in [
        "target/rnode-hil",
        "RNODE_PORT",
        "RNODE_BAUD_RATE",
        "RNODE_SPEED",
        "RNODE_REGION",
        "RNODE_FREQUENCY",
        "RNODE_BANDWIDTH",
        "RNODE_SPREADING_FACTOR",
        "RNODE_CODING_RATE",
        "RNODE_TX_POWER",
        "RNODE_BITRATE",
        "RNODE_COMMAND_TIMEOUT_MS",
        "RNODE_BLE_ADAPTER",
        "RNODE_BLE_SCAN_TIMEOUT_MS",
        "RNODE_BLE_CONNECT_TIMEOUT_MS",
        "RNODE_BLE_MAX_WRITE_LEN",
        "ble://",
        "--features rnode-ble",
        "[[interfaces]]",
        "RNodeInterface",
        "rnode-prepared-host",
        "baud_rate",
        "bitrate",
        "command_timeout_ms",
        "state_path",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnstatus_json",
        "evidence_scope",
        "prepared_host_{transport_kind}_rnode",
        "product_boundary",
        "broader hardware parity",
        "lora",
        "rnode_status",
        "probe_status",
        "radio_status",
        "detected",
        "firmware_version",
        "platform",
        "mcu",
        "online",
        "last_command_error",
        "hardware_errors",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "RNode prepared-host smoke should include required token {required:?}"
        );
    }
}

#[test]
fn nightly_hil_workflow_exposes_rnode_prepared_host_job() {
    let root = repo_root();
    let workflow_path = root.join(".github/workflows/nightly-embedded-hil.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "rnode-prepared-host",
        "HIL_RNODE_ENABLED",
        "HIL_RNODE_PORT",
        "HIL_RNODE_BAUD_RATE",
        "HIL_RNODE_REGION",
        "HIL_RNODE_FREQUENCY",
        "HIL_RNODE_BANDWIDTH",
        "HIL_RNODE_SPREADING_FACTOR",
        "HIL_RNODE_CODING_RATE",
        "HIL_RNODE_TX_POWER",
        "HIL_RNODE_BITRATE",
        "HIL_RNODE_COMMAND_TIMEOUT_MS",
        "HIL_RNODE_BLE_ADAPTER",
        "HIL_RNODE_BLE_SCAN_TIMEOUT_MS",
        "HIL_RNODE_BLE_CONNECT_TIMEOUT_MS",
        "HIL_RNODE_BLE_MAX_WRITE_LEN",
        "HIL_RNODE_TIMEOUT_SECS",
        "./tools/scripts/rnode-prepared-host-smoke.sh",
        "rnode-prepared-host-artifacts",
        "target/rnode-hil/report.json",
        "target/rnode-hil/run.*",
    ] {
        assert!(
            workflow.contains(required),
            "nightly HIL workflow should include required token {required:?}"
        );
    }
}

#[test]
fn lora_runbook_documents_rnode_prepared_host_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-lora-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read LoRa/RNode runbook");

    for required in [
        "Prepared-Host Smoke",
        "./tools/scripts/rnode-prepared-host-smoke.sh",
        "RNODE_PORT=/dev/ttyACM0",
        "RNODE_PORT=ble://RNode 1234",
        "--strict-interface-startup",
        "_runtime.lora.rnode_status.probe_status.detected = true",
        "_runtime.lora.rnode_status.online = true",
        "_runtime.lora.rnode_status.last_command_error = null",
        "target/rnode-hil/",
        "report.json",
        "evidence_scope",
        "prepared_host_serial_rnode",
        "prepared_host_tcp_rnode",
        "prepared_host_ble_rnode",
        "product_boundary",
        "broader hardware parity",
        "HIL_RNODE_ENABLED",
    ] {
        assert!(
            runbook.contains(required),
            "LoRa/RNode runbook should document required token {required:?}"
        );
    }
}

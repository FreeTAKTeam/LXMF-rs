use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn vrn76_prepared_host_smoke_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/vrn76-kiss-ble-prepared-host-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read VR-N76 prepared-host smoke script");

    for required in [
        "target/vrn76-hil",
        "VRN76_PERIPHERAL_ID",
        "VRN76_DEVICE_NAME_FILTER",
        "VRN76_ADAPTER",
        "VRN76_MTU",
        "VRN76_MAX_WRITE_LEN",
        "VRN76_FRAME_MODE",
        "VRN76_KISS_FLOW_CONTROL",
        "VRN76_SCAN_TIMEOUT_MS",
        "VRN76_CONNECT_TIMEOUT_MS",
        "[[interfaces]]",
        "vrn76_kiss_ble",
        "vrn76-prepared-host",
        "--features vrn76-kiss-ble",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnstatus_json",
        "rnstatus_human",
        "startup_status",
        "runtime_iface",
        "connected",
        "subscribed",
        "interface_ready",
        "startup_write_failures",
        "pending_payloads",
        "pending_writes",
        "pending_packets",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "VR-N76 prepared-host smoke should include required token {required:?}"
        );
    }
}

#[test]
fn nightly_hil_workflow_exposes_vrn76_prepared_host_job() {
    let root = repo_root();
    let workflow_path = root.join(".github/workflows/nightly-embedded-hil.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "vrn76-prepared-host",
        "HIL_VRN76_ENABLED",
        "HIL_VRN76_PERIPHERAL_ID",
        "HIL_VRN76_ADAPTER",
        "HIL_VRN76_MTU",
        "HIL_VRN76_MAX_WRITE_LEN",
        "HIL_VRN76_FRAME_MODE",
        "HIL_VRN76_KISS_FLOW_CONTROL",
        "HIL_VRN76_SCAN_TIMEOUT_MS",
        "HIL_VRN76_CONNECT_TIMEOUT_MS",
        "HIL_VRN76_TIMEOUT_SECS",
        "./tools/scripts/vrn76-kiss-ble-prepared-host-smoke.sh",
        "vrn76-prepared-host-artifacts",
        "target/vrn76-hil/report.json",
        "target/vrn76-hil/run.*",
    ] {
        assert!(
            workflow.contains(required),
            "nightly HIL workflow should include required token {required:?}"
        );
    }
}

#[test]
fn vrn76_docs_document_prepared_host_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-vrn76-kiss-ble.md");
    let interface_path = root.join("docs/interfaces/vrn76-kiss-ble.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read VR-N76 runbook");
    let interface_doc = fs::read_to_string(&interface_path).expect("read VR-N76 interface doc");

    for required in [
        "Prepared-Host Smoke",
        "./tools/scripts/vrn76-kiss-ble-prepared-host-smoke.sh",
        "VRN76_PERIPHERAL_ID=VR-N76",
        "--strict-interface-startup",
        "_runtime.vrn76.status.connected = true",
        "_runtime.vrn76.status.subscribed = true",
        "_runtime.vrn76.status.interface_ready = true",
        "target/vrn76-hil/",
        "report.json",
        "HIL_VRN76_ENABLED",
    ] {
        assert!(runbook.contains(required), "VR-N76 runbook should document {required:?}");
    }

    for required in [
        "Prepared-host smoke evidence",
        "vrn76-kiss-ble-prepared-host-smoke.sh",
        "target/vrn76-hil/",
    ] {
        assert!(
            interface_doc.contains(required),
            "VR-N76 interface doc should document {required:?}"
        );
    }
}

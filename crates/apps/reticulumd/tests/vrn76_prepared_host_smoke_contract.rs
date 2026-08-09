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
        "evidence_scope",
        "prepared_host_vrn76_ble_readiness",
        "product_boundary",
        "broader hardware parity",
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
    let workflow_path = root.join(".github/workflows/hil-nightly.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "- vr-n76",
        "Run repository-native nightly profile",
        "cargo xtask hil run --level nightly --profile ${{ matrix.profile }}",
        "Create or update profile failure issue",
        "[HIL] ${{ matrix.profile }} nightly failure",
        "hil-nightly-${{ matrix.profile }}-${{ github.run_id }}",
        "target/hil/runs/${{ matrix.profile }}-${{ github.run_id }}/**",
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
        "evidence_scope",
        "prepared_host_vrn76_ble_readiness",
        "product_boundary",
        "broader hardware parity",
        "through the `vr-n76` profile",
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

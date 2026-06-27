use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn reticulum_interface_parity_audit_preserves_384_385_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/reticulum-interface-parity-audit.sh");
    let script =
        fs::read_to_string(&script_path).expect("read Reticulum interface parity audit script");

    for required in [
        "target/reticulum-interface-parity-audit",
        "--require-full",
        "RNODE_HIL_REPORTS",
        "RNODE_HIL_ARTIFACT_MANIFEST",
        "RIF_HIL_ARTIFACT_MANIFEST",
        "RIF_ARTIFACT_MANIFEST",
        "reticulum_interfaces_384_385_parity_audit",
        "reticulum_interface_hil_matrix_artifacts.v1",
        "artifact_manifest",
        "rnode_serial_report",
        "rnode_tcp_report",
        "rnode_ble_report",
        "sha256 mismatch",
        "manifest_report_paths",
        "existing.get(\"status\") != \"pass\"",
        "local-interface-smoke",
        "local-interface-unix-smoke",
        "local-interface-python-shared-smoke",
        "python_shared_instance_tcp_unix_attach_and_announce_forward",
        "software_unix_shared_instance_local",
        "local-python-tcp-attach",
        "local-python-unix-attach",
        "python_rns_revision",
        "local_client_rxb_total",
        "local_client_txb_total",
        "announced_count",
        "rnode-ble-software-smoke",
        "software_rnode_ble_fallback_management",
        "rnode-fake-tcp-smoke",
        "software_fake_tcp_rnode_prepared_host_management",
        "radio_query_seen",
        "management_blink_seen",
        "prepared_host_serial_rnode",
        "prepared_host_tcp_rnode",
        "prepared_host_ble_rnode",
        "rnode_prepared_host_smoke.v1",
        "captured_at_utc missing",
        "captured_by_host missing",
        "firmware_version.label missing",
        "platform missing",
        "mcu missing",
        "transport_kind",
        "management_commands",
        "post_management_status",
        "missing_full_parity",
        "Software-only RNode evidence is recorded but",
        "does not replace the hardware matrix",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "Reticulum interface parity audit should include required token {required:?}"
        );
    }
}

#[test]
fn reticulum_interface_hil_matrix_preserves_hardware_collection_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/reticulum-interface-hil-matrix.sh");
    let script =
        fs::read_to_string(&script_path).expect("read Reticulum interface HIL matrix script");

    for required in [
        "target/reticulum-interface-hil-matrix",
        "target/rnode-hil/matrix",
        "reticulum_interfaces_384_385_hil_matrix",
        "reticulum-interface-parity-audit.sh --require-full",
        "--allow-partial",
        "--audit-existing",
        "--run-local-smokes",
        "RIF_RUN_LOCAL_SMOKES",
        "RIF_RUN_LOCAL_SMOKES:-auto",
        "Strict matrix runs refresh LocalInterface #384 smokes by default",
        "run_local_smokes",
        "RIF_RNODE_SERIAL_PORT",
        "RIF_RNODE_TCP_PORT",
        "RIF_RNODE_BLE_PORT",
        "RIF_RNODE_SERIAL_FREQUENCY",
        "RIF_RNODE_TCP_FREQUENCY",
        "RIF_RNODE_BLE_FREQUENCY",
        "RIF_RNODE_BLE_ADAPTER",
        "per_bearer_overrides",
        "bearer_env_value",
        "RNODE_SERIAL_PORT",
        "RNODE_TCP_PORT",
        "RNODE_BLE_PORT",
        "serial.report.json",
        "tcp.report.json",
        "ble.report.json",
        "prepared-host RNode reports",
        "required_hardware_identity_fields",
        "captured_at_utc",
        "captured_by_host",
        "firmware_version.label",
        "Software-only RNode evidence is recorded but",
        "does not replace the hardware matrix",
        "RNODE_HIL_REPORTS",
        "parity-audit-report.json",
        "artifact-manifest.json",
        "reticulum_interface_hil_matrix_artifacts.v1",
        "artifact_manifest_path",
        "sha256",
        "missing required endpoint env vars for strict matrix",
    ] {
        assert!(
            script.contains(required),
            "Reticulum interface HIL matrix should include required token {required:?}"
        );
    }
}

#[test]
fn runbooks_document_reticulum_interface_parity_audit() {
    let root = repo_root();
    let local_runbook =
        fs::read_to_string(root.join("docs/runbooks/reticulumd-local-interface.md"))
            .expect("read LocalInterface runbook");
    let lora_runbook = fs::read_to_string(root.join("docs/runbooks/reticulumd-lora-interface.md"))
        .expect("read LoRa/RNode runbook");

    for required in [
        "Reticulum Interface Parity Audit",
        "./tools/scripts/reticulum-interface-parity-audit.sh",
        "./tools/scripts/reticulum-interface-hil-matrix.sh",
        "--require-full",
        "--audit-existing",
        "target/reticulum-interface-parity-audit/report.json",
        "target/reticulum-interface-hil-matrix/report.json",
        "target/reticulum-interface-hil-matrix/artifact-manifest.json",
        "RNODE_HIL_ARTIFACT_MANIFEST",
        "reticulum_interface_hil_matrix_artifacts.v1",
        "reticulum_interfaces_384_385_parity_audit",
        "reticulum_interfaces_384_385_hil_matrix",
        "missing_full_parity",
    ] {
        assert!(
            local_runbook.contains(required),
            "LocalInterface runbook should document audit token {required:?}"
        );
        assert!(
            lora_runbook.contains(required),
            "LoRa/RNode runbook should document audit token {required:?}"
        );
    }
}

#[test]
fn nightly_hil_workflow_exposes_reticulum_interface_matrix_job() {
    let root = repo_root();
    let workflow_path = root.join(".github/workflows/nightly-embedded-hil.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "reticulum-interface-hil-matrix",
        "HIL_RNODE_MATRIX_ENABLED",
        "HIL_RNODE_MATRIX_SERIAL_PORT",
        "HIL_RNODE_MATRIX_TCP_PORT",
        "HIL_RNODE_MATRIX_BLE_PORT",
        "HIL_RNODE_MATRIX_RUN_LOCAL_SMOKES",
        "HIL_RNODE_MATRIX_SERIAL_FREQUENCY",
        "HIL_RNODE_MATRIX_TCP_FREQUENCY",
        "HIL_RNODE_MATRIX_BLE_FREQUENCY",
        "HIL_RNODE_MATRIX_BLE_ADAPTER",
        "HIL_RNODE_MATRIX_BLE_MAX_WRITE_LEN",
        "HIL_RNODE_MATRIX_BLE_TIMEOUT_SECS",
        "RIF_RNODE_SERIAL_PORT",
        "RIF_RNODE_TCP_PORT",
        "RIF_RNODE_BLE_PORT",
        "RIF_RUN_LOCAL_SMOKES",
        "RIF_RNODE_SERIAL_FREQUENCY",
        "RIF_RNODE_TCP_FREQUENCY",
        "RIF_RNODE_BLE_FREQUENCY",
        "RIF_RNODE_BLE_ADAPTER",
        "RIF_RNODE_BLE_MAX_WRITE_LEN",
        "RIF_RNODE_BLE_TIMEOUT_SECS",
        "Install Linux BLE build dependencies",
        "pkg-config libdbus-1-dev",
        "./tools/scripts/reticulum-interface-hil-matrix.sh",
        "reticulum-interface-hil-matrix-artifacts",
        "target/reticulum-interface-hil-matrix/report.json",
        "target/reticulum-interface-hil-matrix/parity-audit-report.json",
        "target/reticulum-interface-hil-matrix/artifact-manifest.json",
        "target/rnode-hil/matrix/*.report.json",
    ] {
        assert!(
            workflow.contains(required),
            "nightly HIL workflow should expose Reticulum interface matrix token {required:?}"
        );
    }
}

#[test]
fn status_docs_track_reticulum_interface_parity_audit() {
    let root = repo_root();
    let roadmap =
        fs::read_to_string(root.join("docs/status/current-roadmap.md")).expect("read roadmap");
    let matrix = fs::read_to_string(root.join("docs/status/reticulum-parity-matrix.md"))
        .expect("read Reticulum parity matrix");

    for required in [
        "reticulum_interfaces_384_385_parity_audit",
        "target/reticulum-interface-parity-audit/report.json",
        "RNODE_HIL_ARTIFACT_MANIFEST",
        "reticulum_interface_hil_matrix_artifacts.v1",
        "LocalInterface #384",
        "RNode BLE #385",
        "serial, TCP/Wi-Fi, and BLE prepared-host RNode hardware reports",
    ] {
        assert!(
            roadmap.contains(required),
            "roadmap should document Reticulum interface audit token {required:?}"
        );
        assert!(
            matrix.contains(required),
            "Reticulum parity matrix should document Reticulum interface audit token {required:?}"
        );
    }
}

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
        "RNODE_MAX_PAYLOAD_BYTES",
        "RNODE_BLE_ADAPTER",
        "RNODE_BLE_SCAN_TIMEOUT_MS",
        "RNODE_BLE_CONNECT_TIMEOUT_MS",
        "RNODE_BLE_MAX_WRITE_LEN",
        "RNODE_MANAGEMENT_TIMEOUT_SECS",
        "RNODE_BLINK_PATTERN",
        "ble://",
        "--features rnode-ble",
        "[[interfaces]]",
        "RNodeInterface",
        "rnode-prepared-host",
        "baud_rate",
        "bitrate",
        "command_timeout_ms",
        "state_path",
        "max_payload_bytes",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnodeconf-rs",
        "query-radio-state",
        "blink",
        "rnodeconf_query_radio_state_json",
        "rnodeconf_blink_json",
        "post_management_rnstatus_json",
        "management_commands",
        "post_management_status",
        "rnstatus_json",
        "report_schema",
        "rnode_prepared_host_smoke.v1",
        "captured_at_utc",
        "captured_by_host",
        "tools/scripts/rnode-prepared-host-smoke.sh",
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
fn rnode_fake_tcp_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/rnode-fake-tcp-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read RNode fake TCP smoke script");

    for required in [
        "target/rnode-fake-tcp-smoke",
        "./tools/scripts/rnode-prepared-host-smoke.sh",
        "software_fake_tcp_rnode_prepared_host_management",
        "prepared_host_tcp_rnode",
        "fake KISS TCP peer",
        "rnodeconf-query-radio-state.json",
        "rnodeconf-blink.json",
        "management_commands",
        "post_management_status",
        "radio_query_seen",
        "management_blink_seen",
        "CMD_RADIO_STATE",
        "CMD_BLINK",
        "product_boundary",
        "not prepared hardware",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "RNode fake TCP smoke should include required token {required:?}"
        );
    }
}

#[test]
fn rnode_ble_software_smoke_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/rnode-ble-software-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read RNode BLE software smoke script");

    for required in [
        "target/rnode-ble-software-smoke",
        "cargo test -p reticulum-rs-transport --features rnode-ble --test rnode_ble",
        "cargo test -p reticulum-rs-transport closed_tx_queue_stops_and_cleans_up_iface",
        "cargo test -p rns-tools --test rnodeconf_cli",
        "cargo test -p reticulumd --features rnode-ble bridge_dispatches_native_rnode_ble_management_commands",
        "evidence_scope",
        "software_rnode_ble_fallback_management",
        "product_boundary",
        "software ",
        "configured Android peripheral exclusion during fallback scan",
        "RNode BLE management handle queueing",
        "reticulumd daemon RnodeBle management bridge dispatch",
        "rnodeconf-rs extended management command-to-RPC matrix",
        "persistent and destructive RNode management CLI guard enforcement",
        "shared transport cleanup of closed TX queues",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "RNode BLE software smoke should include required token {required:?}"
        );
    }
}

#[test]
fn nightly_hil_workflow_exposes_rnode_prepared_host_job() {
    let root = repo_root();
    let workflow_path = root.join(".github/workflows/hil-nightly.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "- rnode-prepared",
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
fn lora_runbook_documents_rnode_ble_software_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-lora-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read LoRa/RNode runbook");

    for required in [
        "RNode BLE Software Smoke",
        "./tools/scripts/rnode-ble-software-smoke.sh",
        "target/rnode-ble-software-smoke/",
        "evidence_scope = \"software_rnode_ble_fallback_management\"",
        "configured Android peripheral exclusion during fallback scan",
        "RNode BLE management handle queueing",
        "reticulumd daemon RnodeBle management bridge dispatch",
        "shared transport cleanup of closed TX queues",
        "software regressions only",
        "BLE hardware, firmware, radio, and management operation evidence",
    ] {
        assert!(
            runbook.contains(required),
            "LoRa/RNode runbook should document RNode BLE software smoke token {required:?}"
        );
    }
}

#[test]
fn lora_runbook_documents_rnode_fake_tcp_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-lora-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read LoRa/RNode runbook");

    for required in [
        "RNode Fake TCP Smoke",
        "./tools/scripts/rnode-fake-tcp-smoke.sh",
        "target/rnode-fake-tcp-smoke/",
        "evidence_scope = \"software_fake_tcp_rnode_prepared_host_management\"",
        "prepared_host_tcp_rnode",
        "fake KISS TCP peer",
        "rnodeconf-query-radio-state.json",
        "rnodeconf-blink.json",
        "radio_query_seen",
        "management_blink_seen",
        "not prepared hardware",
    ] {
        assert!(
            runbook.contains(required),
            "LoRa/RNode runbook should document RNode fake TCP smoke token {required:?}"
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
        "`rnodeconf-rs query-radio-state --interface rnode-prepared-host`",
        "`rnodeconf-rs blink --interface rnode-prepared-host --pattern`",
        "rnodeconf-query-radio-state.json",
        "rnodeconf-blink.json",
        "rnstatus-post-management.json",
        "management_commands",
        "post_management_status",
        "target/rnode-hil/",
        "report.json",
        "evidence_scope",
        "prepared_host_serial_rnode",
        "prepared_host_tcp_rnode",
        "prepared_host_ble_rnode",
        "product_boundary",
        "broader hardware parity",
        "through the `rnode-prepared` profile",
    ] {
        assert!(
            runbook.contains(required),
            "LoRa/RNode runbook should document required token {required:?}"
        );
    }
}

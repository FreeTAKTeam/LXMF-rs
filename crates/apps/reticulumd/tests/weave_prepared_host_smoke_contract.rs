use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn weave_prepared_host_smoke_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/weave-prepared-host-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read Weave prepared-host smoke script");

    for required in [
        "target/weave-hil",
        "WEAVE_PORT",
        "WEAVE_DEVICE",
        "WEAVE_BAUD_RATE",
        "WEAVE_SPEED",
        "WEAVE_MTU",
        "WEAVE_CONFIGURED_BITRATE",
        "WEAVE_REQUIRE_CONNECTED",
        "WEAVE_REMOTE_DISPLAY_CONTROL",
        "WEAVE_TIMEOUT_SECS",
        "[[interfaces]]",
        "WeaveInterface",
        "weave-prepared-host",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnstatus_json",
        "evidence_scope",
        "prepared_host_connected_serial",
        "prepared_host_serial_discovery_only",
        "product_boundary",
        "broader production parity",
        "weave",
        "status",
        "runtime_iface",
        "link_state",
        "connected",
        "discovering",
        "wdcl_connected",
        "remote_switch_id",
        "last_error",
        "frames_tx",
        "bytes_tx",
        "weaveconf-rs",
        "enable-remote-display",
        "disable-remote-display",
        "remote_display_control_requested",
        "remote_display_control_result",
        "enable_disable_ok",
        "display",
        "device_stats",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "Weave prepared-host smoke should include required token {required:?}"
        );
    }
}

#[test]
fn weave_fake_pty_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/weave-fake-pty-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read Weave fake PTY smoke script");

    for required in [
        "target/weave-fake-pty-smoke",
        "WeaveInterface",
        "weave-fake-pty",
        "pty.openpty",
        "cryptography.hazmat.primitives.asymmetric.ed25519",
        "WDCL_T_DISCOVER",
        "WDCL_T_CONNECT",
        "WDCL_T_CMD",
        "WDCL_T_LOG",
        "WDCL_T_DISP",
        "WDCL_CMD_REMOTE_DISPLAY",
        "ET_PROTO_WDCL_CONNECTION",
        "--strict-interface-startup",
        "rnstatus-rs",
        "--weave-display weave-fake-pty",
        "weaveconf-rs",
        "enable-remote-display",
        "disable-remote-display",
        "remote_display_enable_seen",
        "remote_display_disable_seen",
        "display_frame_sent",
        "device_stats_sent",
        "buffer_hex",
        "aabbccdd",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "Weave fake PTY smoke should include required token {required:?}"
        );
    }
}

#[test]
fn nightly_hil_workflow_exposes_weave_prepared_host_job() {
    let root = repo_root();
    let workflow_path = root.join(".github/workflows/nightly-embedded-hil.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "weave-prepared-host",
        "HIL_WEAVE_ENABLED",
        "HIL_WEAVE_PORT",
        "HIL_WEAVE_BAUD_RATE",
        "HIL_WEAVE_MTU",
        "HIL_WEAVE_CONFIGURED_BITRATE",
        "HIL_WEAVE_REQUIRE_CONNECTED",
        "HIL_WEAVE_REMOTE_DISPLAY_CONTROL",
        "HIL_WEAVE_TIMEOUT_SECS",
        "./tools/scripts/weave-prepared-host-smoke.sh",
        "weave-prepared-host-artifacts",
        "target/weave-hil/report.json",
        "target/weave-hil/run.*",
    ] {
        assert!(
            workflow.contains(required),
            "nightly HIL workflow should include required token {required:?}"
        );
    }
}

#[test]
fn weave_runbook_documents_prepared_host_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-weave-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read Weave runbook");

    for required in [
        "Prepared-Host Smoke",
        "./tools/scripts/weave-prepared-host-smoke.sh",
        "WEAVE_PORT=/dev/ttyACM0",
        "WEAVE_REQUIRE_CONNECTED=true",
        "WEAVE_REMOTE_DISPLAY_CONTROL=true",
        "--strict-interface-startup",
        "_runtime.weave.status.link_state = \"connected\"",
        "_runtime.weave.status.wdcl_connected = true",
        "_runtime.weave.status.remote_switch_id",
        "_runtime.iface",
        "weaveconf-rs enable-remote-display",
        "weaveconf-rs disable-remote-display",
        "target/weave-hil/",
        "report.json",
        "evidence_scope",
        "prepared_host_connected_serial",
        "prepared_host_serial_discovery_only",
        "product_boundary",
        "broader production parity",
        "HIL_WEAVE_ENABLED",
    ] {
        assert!(
            runbook.contains(required),
            "Weave runbook should document required token {required:?}"
        );
    }
}

#[test]
fn weave_runbook_documents_fake_pty_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-weave-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read Weave runbook");

    for required in [
        "Software Fake-PTY Smoke",
        "./tools/scripts/weave-fake-pty-smoke.sh",
        "target/weave-fake-pty-smoke/",
        "signed WDCL discovery response",
        "_runtime.weave.status.link_state = \"connected\"",
        "_runtime.weave.status.display.buffer_hex = \"aabbccdd\"",
        "rnstatus-rs --weave-display weave-fake-pty",
        "`weaveconf-rs enable-remote-display --interface weave-fake-pty`",
        "`weaveconf-rs disable-remote-display --interface weave-fake-pty`",
        "remote_display_enable_seen",
        "remote_display_disable_seen",
        "device_stats_sent",
    ] {
        assert!(
            runbook.contains(required),
            "Weave runbook should document fake PTY token {required:?}"
        );
    }
}

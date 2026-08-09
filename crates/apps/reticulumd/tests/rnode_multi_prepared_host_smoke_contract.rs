use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn rnode_multi_prepared_host_smoke_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/rnode-multi-prepared-host-smoke.sh");
    let script =
        fs::read_to_string(&script_path).expect("read RNodeMulti prepared-host smoke script");

    for required in [
        "target/rnode-multi-hil",
        "RNODE_MULTI_PORT",
        "RNODE_MULTI_BAUD_RATE",
        "RNODE_MULTI_VPORTS",
        "RNODE_MULTI_FREQUENCIES",
        "RNODE_MULTI_BANDWIDTHS",
        "RNODE_MULTI_SPREADING_FACTORS",
        "RNODE_MULTI_CODING_RATES",
        "RNODE_MULTI_TX_POWERS",
        "RNODE_MULTI_OUTGOING",
        "[[interfaces]]",
        "RNodeMultiInterface",
        "rnode-multi-prepared-host",
        "radio{index}",
        "vport",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnstatus_json",
        "evidence_scope",
        "prepared_host_single_device_vport_probe",
        "product_boundary",
        "not broad production parity",
        "rnode_multi",
        "radio_status",
        "stream_state",
        "running",
        "last_error",
        "selected_vport",
        "vports",
        "startup_probe",
        "firmware_version",
        "platform",
        "mcu",
        "interfaces",
        "subinterfaces",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "RNodeMulti prepared-host smoke should include required token {required:?}"
        );
    }
}

#[test]
fn rnode_multi_fake_tcp_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/rnode-multi-fake-tcp-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read RNodeMulti fake TCP smoke script");

    for required in [
        "target/rnode-multi-fake-tcp-smoke",
        "RNodeMultiInterface",
        "rnode-multi-fake-tcp",
        "tcp://127.0.0.1",
        "radio0",
        "radio1",
        "vport = 2",
        "vport = 3",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnodeconf-rs",
        "blink",
        "--vport 2",
        "--pattern 3",
        "evidence_scope",
        "software_fake_tcp_rnode_multi",
        "product_boundary",
        "not prepared-host or broad hardware production parity",
        "CMD_INTERFACES",
        "CMD_SEL_INT",
        "CMD_BLINK",
        "startup_status",
        "stream_state",
        "running",
        "startup_probe",
        "firmware_version",
        "interface_summary",
        "management_blink_seen",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "RNodeMulti fake TCP smoke should include required token {required:?}"
        );
    }
}

#[test]
fn rnode_multi_fake_pty_smoke_preserves_serial_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/rnode-multi-fake-pty-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read RNodeMulti fake PTY smoke script");

    for required in [
        "target/rnode-multi-fake-pty-smoke",
        "RNodeMultiInterface",
        "rnode-multi-fake-pty",
        "pty.openpty",
        "tty.setraw",
        "speed = 115200",
        "radio0",
        "radio1",
        "vport = 2",
        "vport = 3",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnodeconf-rs",
        "blink",
        "--vport 2",
        "--pattern 3",
        "evidence_scope",
        "software_fake_pty_rnode_multi",
        "product_boundary",
        "not prepared-host or broad hardware production parity",
        "CMD_INTERFACES",
        "CMD_SEL_INT",
        "CMD_BLINK",
        "startup_status",
        "stream_state",
        "running",
        "startup_probe",
        "firmware_version",
        "interface_summary",
        "management_blink_seen",
        "pty_raw_mode",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "RNodeMulti fake PTY smoke should include required token {required:?}"
        );
    }
}

#[test]
fn nightly_hil_workflow_exposes_rnode_multi_prepared_host_job() {
    let root = repo_root();
    let workflow_path = root.join(".github/workflows/hil-nightly.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read nightly HIL workflow");

    for required in [
        "- rnode-multi",
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
fn rnode_multi_runbook_documents_fake_tcp_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-rnode-multi-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read RNodeMulti runbook");

    for required in [
        "Software Fake-TCP Smoke",
        "./tools/scripts/rnode-multi-fake-tcp-smoke.sh",
        "target/rnode-multi-fake-tcp-smoke/",
        "fake TCP RNodeMulti peer",
        "`CMD_INTERFACES`",
        "`CMD_SEL_INT` before a blink management command",
        "`rnodeconf-rs blink --interface rnode-multi-fake-tcp --vport 2 --pattern 3`",
        "evidence_scope = \"software_fake_tcp_rnode_multi\"",
        "product_boundary",
        "not prepared-host or broad hardware production parity",
        "_runtime.rnode_multi.radio_status.stream_state = \"running\"",
        "_runtime.rnode_multi.radio_status.startup_probe.interface_summary",
        "management_blink_seen",
    ] {
        assert!(
            runbook.contains(required),
            "RNodeMulti runbook should document fake TCP token {required:?}"
        );
    }
}

#[test]
fn rnode_multi_runbook_documents_fake_pty_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-rnode-multi-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read RNodeMulti runbook");

    for required in [
        "Software Fake-PTY Smoke",
        "./tools/scripts/rnode-multi-fake-pty-smoke.sh",
        "target/rnode-multi-fake-pty-smoke/",
        "raw pseudo-terminal",
        "`speed = 115200`",
        "`CMD_INTERFACES`",
        "`CMD_SEL_INT` before a blink management command",
        "`rnodeconf-rs blink --interface rnode-multi-fake-pty --vport 2 --pattern 3`",
        "evidence_scope = \"software_fake_pty_rnode_multi\"",
        "product_boundary",
        "not prepared-host or broad hardware production parity",
        "_runtime.rnode_multi.radio_status.stream_state = \"running\"",
        "_runtime.rnode_multi.radio_status.startup_probe.interface_summary",
        "management_blink_seen",
        "serial software path",
    ] {
        assert!(
            runbook.contains(required),
            "RNodeMulti runbook should document fake PTY token {required:?}"
        );
    }
}

#[test]
fn rnode_multi_runbook_documents_prepared_host_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-rnode-multi-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read RNodeMulti runbook");

    for required in [
        "Prepared-Host Smoke",
        "./tools/scripts/rnode-multi-prepared-host-smoke.sh",
        "RNODE_MULTI_PORT=/dev/ttyACM0",
        "RNODE_MULTI_VPORTS=0,1",
        "--strict-interface-startup",
        "_runtime.rnode_multi.radio_status.stream_state = \"running\"",
        "_runtime.rnode_multi.radio_status.vports",
        "_runtime.rnode_multi.radio_status.startup_probe.firmware_version.label",
        "_runtime.rnode_multi.radio_status.startup_probe.platform",
        "_runtime.rnode_multi.radio_status.startup_probe.mcu",
        "_runtime.rnode_multi.radio_status.startup_probe.interfaces",
        "target/rnode-multi-hil/",
        "report.json",
        "evidence_scope",
        "prepared_host_single_device_vport_probe",
        "product_boundary",
        "not broad production parity",
        "through the `rnode-multi` profile",
    ] {
        assert!(
            runbook.contains(required),
            "RNodeMulti runbook should document required token {required:?}"
        );
    }
}

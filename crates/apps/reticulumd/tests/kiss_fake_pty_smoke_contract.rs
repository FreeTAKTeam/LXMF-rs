use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn kiss_fake_pty_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/kiss-fake-pty-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read KISS fake PTY smoke script");

    for required in [
        "target/kiss-fake-pty-smoke",
        "KISSInterface",
        "AX25KISSInterface",
        "kiss-fake-pty",
        "ax25-kiss-fake-pty",
        "pty.openpty",
        "tty.setraw",
        "speed = 9600",
        "speed = 1200",
        "flow_control = true",
        "callsign = \"N0CALL\"",
        "ssid = 1",
        "--strict-interface-startup",
        "rnstatus-rs",
        "CMD_TXDELAY",
        "CMD_TXTAIL",
        "CMD_P",
        "CMD_SLOTTIME",
        "CMD_READY",
        "ready_response_sent",
        "init_commands_seen",
        "startup_status",
        "link_state",
        "running",
        "bearer",
        "serial",
        "interface_ready",
        "command_frames_rx",
        "ready_frames_rx",
        "init_frames_tx",
        "ax25",
        "pty_raw_mode",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "KISS fake PTY smoke should include required token {required:?}"
        );
    }
}

#[test]
fn kiss_runbook_documents_fake_pty_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-kiss-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read KISS runbook");

    for required in [
        "Software Fake-PTY Smoke",
        "./tools/scripts/kiss-fake-pty-smoke.sh",
        "target/kiss-fake-pty-smoke/",
        "raw pseudo-terminal",
        "`KISSInterface`",
        "`AX25KISSInterface`",
        "`CMD_TXDELAY`",
        "`CMD_TXTAIL`",
        "`CMD_P`",
        "`CMD_SLOTTIME`",
        "`CMD_READY`",
        "_runtime.kiss.status.link_state = \"running\"",
        "_runtime.kiss.status.bearer = \"serial\"",
        "_runtime.kiss.status.interface_ready = true",
        "_runtime.kiss.status.ready_frames_rx >= 1",
        "_runtime.kiss.status.init_frames_tx >= 5",
        "fake peer recording all KISS startup command frames",
        "not a substitute for real TNC or modem hardware evidence",
    ] {
        assert!(
            runbook.contains(required),
            "KISS runbook should document fake PTY token {required:?}"
        );
    }
}

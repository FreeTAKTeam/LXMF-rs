use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn kiss_fake_tcp_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/kiss-fake-tcp-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read KISS fake TCP smoke script");

    for required in [
        "target/kiss-fake-tcp-smoke",
        "TCPClientInterface",
        "kiss_framing = true",
        "kiss-fake-tcp",
        "socket.socket",
        "listen",
        "accept",
        "target_host = \"127.0.0.1\"",
        "target_port = ${KISS_TCP_PORT}",
        "fixed_mtu = 512",
        "flow_control = true",
        "--strict-interface-startup",
        "rnstatus-rs",
        "CMD_TXDELAY",
        "CMD_TXTAIL",
        "CMD_P",
        "CMD_SLOTTIME",
        "CMD_READY",
        "ready_response_sent",
        "accepted_connections",
        "init_commands_seen",
        "startup_status",
        "kiss_tcp",
        "link_state",
        "running",
        "bearer",
        "tcp",
        "endpoint",
        "interface_ready",
        "command_frames_rx",
        "ready_frames_rx",
        "init_frames_tx",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "KISS fake TCP smoke should include required token {required:?}"
        );
    }
}

#[test]
fn kiss_runbook_documents_fake_tcp_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-kiss-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read KISS runbook");

    for required in [
        "Software Fake-TCP Smoke",
        "./tools/scripts/kiss-fake-tcp-smoke.sh",
        "target/kiss-fake-tcp-smoke/",
        "fake TCP KISS server",
        "`TCPClientInterface`",
        "`kiss_framing = true`",
        "`kiss_tcp_client`",
        "`CMD_TXDELAY`",
        "`CMD_TXTAIL`",
        "`CMD_P`",
        "`CMD_SLOTTIME`",
        "`CMD_READY`",
        "_runtime.kiss_tcp.status.link_state = \"running\"",
        "_runtime.kiss_tcp.status.bearer = \"tcp\"",
        "_runtime.kiss_tcp.status.interface_ready = true",
        "_runtime.kiss_tcp.status.ready_frames_rx >= 1",
        "_runtime.kiss_tcp.status.init_frames_tx >= 5",
        "fake server recording all KISS startup command frames",
        "not a substitute for real Wi-Fi KISS bridge or modem",
    ] {
        assert!(
            runbook.contains(required),
            "KISS runbook should document fake TCP token {required:?}"
        );
    }
}

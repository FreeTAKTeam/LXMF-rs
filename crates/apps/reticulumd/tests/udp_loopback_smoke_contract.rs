use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn udp_loopback_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/udp-loopback-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read UDP loopback smoke script");

    for required in [
        "target/udp-loopback-smoke",
        "UDPInterface",
        "udp-loopback",
        "listen_ip",
        "listen_port",
        "forward_ip",
        "forward_port",
        "--strict-interface-startup",
        "rnstatus-rs",
        "sent_malformed_datagram",
        "not-a-reticulum-packet",
        "startup_status",
        "link_state",
        "bound",
        "role",
        "peer",
        "bind_addr",
        "forward_addr",
        "bytes_rx",
        "decode_errors",
        "couldn't decode packet",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "UDP loopback smoke should include required token {required:?}"
        );
    }
}

#[test]
fn udp_runbook_documents_loopback_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-udp-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read UDP runbook");

    for required in [
        "Software Loopback Smoke",
        "./tools/scripts/udp-loopback-smoke.sh",
        "target/udp-loopback-smoke/",
        "Python-style `UDPInterface`",
        "`listen_ip`",
        "`listen_port`",
        "`forward_ip`",
        "`forward_port`",
        "_runtime.udp.status.link_state = \"bound\"",
        "_runtime.udp.status.role = \"peer\"",
        "_runtime.udp.status.bytes_rx",
        "_runtime.udp.status.decode_errors >= 1",
        "_runtime.udp.status.last_error = \"couldn't decode packet\"",
        "loopback probe payload metadata",
        "not a substitute for multi-host multicast",
    ] {
        assert!(
            runbook.contains(required),
            "UDP runbook should document loopback smoke token {required:?}"
        );
    }
}

#[test]
fn configured_udp_packet_trace_preserves_reference_and_restart_contract() {
    let root = repo_root();
    let script = fs::read_to_string(root.join("tools/scripts/udp-configured-packet-smoke.sh"))
        .expect("read configured UDP packet trace");
    let peer = fs::read_to_string(root.join("tools/scripts/udp-reference-loopback-peer.py"))
        .expect("read UDP reference loopback peer");
    let runbook = fs::read_to_string(root.join("docs/runbooks/reticulumd-udp-interface.md"))
        .expect("read UDP runbook");

    for required in [
        "UDPInterface",
        "--strict-interface-startup",
        "--announce-interval-secs 1",
        "packets_tx",
        "packets_rx",
        "restart_same_ports",
        "configured_udp_valid_packet_loopback_status_and_restart",
        "99de23c040d507e3fefca19e87b182302902725d",
    ] {
        assert!(script.contains(required), "UDP trace should include {required:?}");
    }
    for required in ["recvfrom", "sendto", "payload_sha256", "echoed_to_daemon"] {
        assert!(peer.contains(required), "UDP peer should include {required:?}");
    }
    for required in [
        "Configured Packet Loopback and Restart Trace",
        "./tools/scripts/udp-configured-packet-smoke.sh",
        "configured_udp_valid_packet_loopback_status_and_restart",
        "does not prove multicast, multi-host,",
    ] {
        assert!(runbook.contains(required), "UDP runbook should include {required:?}");
    }
}

#[test]
fn configured_udp_packet_trace_runs_in_linux_ci_and_uploads_report() {
    let root = repo_root();
    let workflow =
        fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read CI workflow");

    for required in [
        "linux-udp-runtime:",
        "Linux configured UDP runtime software smoke",
        "./tools/scripts/udp-configured-packet-smoke.sh",
        "target/udp-configured-packet-smoke/report.json",
        "udp-configured-packet-smoke-${{ github.run_id }}",
    ] {
        assert!(
            workflow.contains(required),
            "CI workflow should include configured UDP runtime evidence token {required:?}"
        );
    }
}

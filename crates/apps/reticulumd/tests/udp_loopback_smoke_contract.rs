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

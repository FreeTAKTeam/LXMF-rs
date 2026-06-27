use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn local_interface_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/local-interface-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read LocalInterface smoke script");

    for required in [
        "target/local-interface-smoke",
        "LocalInterface",
        "LocalClientInterface",
        "local-tcp-listener",
        "local-tcp-attach",
        "shared_instance_type = \"tcp\"",
        "host = \"127.0.0.1\"",
        "fixed_mtu = 262144",
        "force_shared_instance_bitrate = 1000000",
        "--strict-interface-startup",
        "rnstatus-rs",
        "accepted_connections",
        "startup_status",
        "active",
        "attached",
        "runtime_iface",
        "shared_instance_type",
        "force_shared_instance_bitrate",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "LocalInterface smoke should include required token {required:?}"
        );
    }
}

#[test]
fn local_runbook_documents_tcp_shared_instance_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-local-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read LocalInterface runbook");

    for required in [
        "Software TCP Shared-Instance Smoke",
        "./tools/scripts/local-interface-smoke.sh",
        "target/local-interface-smoke/",
        "`LocalInterface`",
        "`LocalClientInterface`",
        "`shared_instance_type = \"tcp\"`",
        "`fixed_mtu = 262144`",
        "`force_shared_instance_bitrate = 1000000`",
        "_runtime.startup_status = \"active\"",
        "_runtime.startup_status = \"attached\"",
        "fake shared instance accepting the attach connection",
        "multi-process Python shared-instance",
    ] {
        assert!(
            runbook.contains(required),
            "LocalInterface runbook should document smoke token {required:?}"
        );
    }
}

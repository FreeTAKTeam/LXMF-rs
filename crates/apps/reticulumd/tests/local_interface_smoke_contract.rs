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
fn local_interface_unix_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/local-interface-unix-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read LocalInterface Unix smoke script");

    for required in [
        "target/local-interface-unix-smoke",
        "LocalInterface",
        "LocalClientInterface",
        "local-unix-filesystem-listener",
        "local-unix-abstract-listener",
        "local-unix-abstract-attach",
        "shared_instance_type = \"unix\"",
        "socket_path = \"${FILESYSTEM_SOCKET}\"",
        "socket_path = \"@rns/${ABSTRACT_ATTACH_INSTANCE}\"",
        "instance_name = \"${ABSTRACT_LISTENER_INSTANCE}\"",
        "fixed_mtu = 262144",
        "force_shared_instance_bitrate = 1000000",
        "--strict-interface-startup",
        "rnstatus-rs",
        "AF_UNIX",
        "accepted_connections",
        "software_unix_shared_instance_local",
        "product_boundary",
        "not multi-process Python shared-instance interop evidence",
        "startup_status",
        "active",
        "attached",
        "runtime_iface",
        "shared_instance_type",
        "socket_path",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "LocalInterface Unix smoke should include required token {required:?}"
        );
    }
}

#[test]
fn local_interface_python_shared_smoke_preserves_interop_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/local-interface-python-shared-smoke.sh");
    let script =
        fs::read_to_string(&script_path).expect("read LocalInterface Python shared smoke script");

    for required in [
        "target/local-interface-python-shared-smoke",
        "RETICULUM_PY_REPO",
        "python_rns_revision",
        "RNS.Reticulum",
        "share_instance = yes",
        "shared_instance_type = tcp",
        "shared_instance_type = unix",
        "local-python-tcp-attach",
        "local-python-unix-attach",
        "LocalClientInterface",
        "socket_path = \"@rns/${PY_UNIX_INSTANCE}\"",
        "fixed_mtu = 262144",
        "force_shared_instance_bitrate = 1000000",
        "--strict-interface-startup",
        "rnstatus-rs",
        "local_client_count",
        "python_shared_instance_tcp_unix_attach_and_announce_forward",
        "python-traffic-tcp",
        "python-traffic-unix",
        "destination.announce",
        "announced_count",
        "local_client_rxb_total",
        "local_client_txb_total",
        "product_boundary",
        "Python-origin announces move across the shared",
        "does not prove broad application-level shared-instance traffic parity",
        "startup_status",
        "attached",
        "runtime_iface",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "LocalInterface Python shared smoke should include required token {required:?}"
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

#[test]
fn local_runbook_documents_unix_shared_instance_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-local-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read LocalInterface runbook");

    for required in [
        "Software Unix Shared-Instance Smoke",
        "./tools/scripts/local-interface-unix-smoke.sh",
        "target/local-interface-unix-smoke/",
        "`LocalInterface`",
        "`LocalClientInterface`",
        "`shared_instance_type = \"unix\"`",
        "filesystem Unix listener",
        "Linux abstract Unix listener",
        "Linux abstract Unix client attach",
        "_runtime.startup_status = \"active\"",
        "_runtime.startup_status = \"attached\"",
        "evidence_scope = \"software_unix_shared_instance_local\"",
        "product_boundary",
        "not multi-process Python shared-instance interop evidence",
    ] {
        assert!(
            runbook.contains(required),
            "LocalInterface runbook should document Unix smoke token {required:?}"
        );
    }
}

#[test]
fn local_runbook_documents_python_shared_instance_smoke_artifacts() {
    let root = repo_root();
    let runbook_path = root.join("docs/runbooks/reticulumd-local-interface.md");
    let runbook = fs::read_to_string(&runbook_path).expect("read LocalInterface runbook");

    for required in [
        "Pinned Python Shared-Instance Smoke",
        "./tools/scripts/local-interface-python-shared-smoke.sh",
        "target/local-interface-python-shared-smoke/",
        "`RETICULUM_PY_REPO`",
        "`LocalClientInterface`",
        "real pinned Python Reticulum shared instances",
        "TCP shared instance",
        "Linux abstract Unix shared instance",
        "_runtime.startup_status = \"attached\"",
        "python_rns_revision",
        "local_client_count",
        "announced_count",
        "local_client_rxb_total",
        "local_client_txb_total",
        "evidence_scope = \"python_shared_instance_tcp_unix_attach_and_announce_forward\"",
        "product_boundary",
        "Python-origin announce fanout",
        "does not prove broad application-level shared-instance traffic parity",
    ] {
        assert!(
            runbook.contains(required),
            "LocalInterface runbook should document Python shared smoke token {required:?}"
        );
    }
}

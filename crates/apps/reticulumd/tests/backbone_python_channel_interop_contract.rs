use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn python_channel_interop_preserves_backbone_variants() {
    let root = repo_root();
    let support_path = root.join("crates/apps/reticulumd/tests/support/python_channel_process.rs");
    let prelude_path =
        root.join("crates/apps/reticulumd/tests/python_channel_interop_parts/module_prelude.rs");
    let python_to_rust_path = root.join(
        "crates/apps/reticulumd/tests/python_channel_interop_parts/rust_to_python_raw_resource_roundtri.rs",
    );
    let workflow_path = root.join(".github/workflows/python-interop.yml");

    let support = fs::read_to_string(&support_path).expect("read Python channel process support");
    let prelude = fs::read_to_string(&prelude_path).expect("read Python channel interop prelude");
    let python_to_rust =
        fs::read_to_string(&python_to_rust_path).expect("read Python-to-Rust interop part");
    let workflow = fs::read_to_string(&workflow_path).expect("read Python interop workflow");

    for required in [
        "PythonInteropInterfaceKind",
        "BackboneInterface",
        "BackboneClientInterface",
        "write_python_config_for_kind",
        "write_python_client_config_for_kind",
    ] {
        assert!(
            support.contains(required),
            "Python channel support should preserve Backbone token {required:?}"
        );
    }

    for required in [
        "rust_to_python_backbone_channel_roundtrip",
        "rust_to_python_backbone_link_data_roundtrip",
        "rust_to_python_backbone_request_response_roundtrip",
        "wait_for_reply_tuple",
        "rust-backbone",
        "reply:hello-python-backbone",
        "rust_client_for_python_interop",
        "rust_server_for_python_interop",
        "TcpSocketTuning::backbone",
        "with_backbone_liveness",
        "with_backbone_client_liveness",
        "with_mtu(1_048_576)",
        "with_client_mtu(1_048_576)",
    ] {
        assert!(
            prelude.contains(required),
            "channel interop prelude should preserve Backbone token {required:?}"
        );
    }

    for required in [
        "rust_to_python_backbone_raw_resource_roundtrip",
        "python_to_rust_backbone_channel_roundtrip",
        "python_to_rust_backbone_link_data_roundtrip",
        "python_to_rust_backbone_request_response_roundtrip",
        "python_to_rust_backbone_resource_backed_request_response_roundtrip",
        "rust_server_for_python_interop",
    ] {
        assert!(
            python_to_rust.contains(required),
            "Python-to-Rust interop should preserve Backbone token {required:?}"
        );
    }

    assert!(
        workflow.contains(
            "cargo test -p reticulumd --test python_channel_interop -- --ignored --nocapture"
        ),
        "Python interop workflow should continue running the ignored channel interop matrix"
    );
}

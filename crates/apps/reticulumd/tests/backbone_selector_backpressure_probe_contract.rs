use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn backbone_selector_probe_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/backbone_selector_backpressure_probe.py");
    let script = fs::read_to_string(&script_path).expect("read Backbone selector probe");

    for required in [
        "selectors.DefaultSelector",
        "EpollSelector",
        "require_epoll",
        "non_writable_attempt",
        "blocking_error_attempt",
        "bytes_sent_before_backpressure",
        "sends_before_backpressure",
        "result",
        "backpressured",
        "json_out",
    ] {
        assert!(
            script.contains(required),
            "Backbone selector probe should include required token {required:?}"
        );
    }
}

#[test]
fn python_interop_workflow_runs_backbone_selector_probe() {
    let root = repo_root();
    let matrix_path = root.join("tests/hil/cases/interop.toml");
    let matrix = fs::read_to_string(&matrix_path).expect("read HIL interop case matrix");
    let workflow_path = root.join(".github/workflows/verify.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read PR HIL workflow");

    for required in [
        "python-backbone-selector",
        "python-backbone-reference-selector",
        "tools/scripts/backbone_selector_backpressure_probe.py",
        "tools/scripts/backbone_python_reference_backpressure_probe.py",
        "--python-rns-path",
        "--require-epoll",
        "program = \"cargo\"",
        "reticulum-rs-transport",
        "backbone_hdlc_stream_backpressures_when_peer_stops_reading",
    ] {
        assert!(
            matrix.contains(required) || workflow.contains(required),
            "repository-native HIL controller should include required token {required:?}"
        );
    }

    assert!(
        workflow.contains("cargo xtask hil run --level pr --all"),
        "PR verification should invoke the HIL controller"
    );
}

#[test]
fn backbone_python_reference_probe_preserves_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/backbone_python_reference_backpressure_probe.py");
    let script = fs::read_to_string(&script_path).expect("read Python Backbone reference probe");

    for required in [
        "BackboneClientInterface",
        "BackboneInterface",
        "PYTHON_RNS_PATH",
        "RETICULUM_PY_REPO",
        "require_epoll",
        "pending_transmit_buffer_after_stable_wait",
        "txb_after_stable_wait",
        "parent_txb_after_stable_wait",
        "python_rns_revision",
        "result",
        "backpressured",
        "json_out",
    ] {
        assert!(
            script.contains(required),
            "Python Backbone reference probe should include required token {required:?}"
        );
    }
}

#[test]
fn parity_matrix_documents_backbone_selector_probe() {
    let root = repo_root();
    let matrix_path = root.join("docs/status/reticulum-parity-matrix.md");
    let matrix = fs::read_to_string(&matrix_path).expect("read parity matrix");

    for required in [
        "Python selector/epoll slow-reader probe",
        "backbone_selector_backpressure_probe.py",
        "backbone_python_reference_backpressure_probe.py",
        "BackboneClientInterface",
        "EpollSelector",
        "backbone_hdlc_stream_backpressures_when_peer_stops_reading",
        "live Python Reticulum BackboneClientInterface",
    ] {
        assert!(
            matrix.contains(required),
            "parity matrix should document required token {required:?}"
        );
    }
}

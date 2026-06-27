use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repo root")
}

#[test]
fn pipe_fake_subprocess_smoke_preserves_software_evidence_contract() {
    let root = repo_root();
    let script_path = root.join("tools/scripts/pipe-fake-subprocess-smoke.sh");
    let script = fs::read_to_string(&script_path).expect("read Pipe fake subprocess smoke script");

    for required in [
        "target/pipe-fake-subprocess-smoke",
        "PipeInterface",
        "pipe-fake-subprocess",
        "command = \"cat\"",
        "respawn_delay = 0.1",
        "--strict-interface-startup",
        "rnstatus-rs",
        "rnstatus_json",
        "rnstatus_human",
        "startup_status",
        "runtime_iface",
        "process_state",
        "running",
        "pipe_is_open",
        "respawn_attempts",
        "last_error",
        "pipe state=running open=true respawns=0",
        "report.json",
    ] {
        assert!(
            script.contains(required),
            "Pipe fake subprocess smoke should include required token {required:?}"
        );
    }
}

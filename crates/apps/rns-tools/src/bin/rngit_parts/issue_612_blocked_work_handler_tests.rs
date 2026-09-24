fn issue_612_blocked_work_list_response(
    group_path: &std::path::Path,
    remote: [u8; 16],
) -> Vec<u8> {
    std::fs::create_dir_all(group_path).expect("group directory");
    let repository_path = group_path.join("repo");
    assert!(std::process::Command::new("git")
        .args(["init", "--bare", "--quiet", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("initialize bare repository")
        .success());
    std::fs::write(group_path.with_extension("allowed"), "read:all\n")
        .expect("write broad group read permission");

    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", group_path).expect("load repository group");
    node.blocked_identities.insert(remote);
    node.handle_work_request(
        &[
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from("list")),
            (rmpv::Value::from("scope"), rmpv::Value::from("active")),
        ],
        remote,
    )
}

#[test]
fn blocked_identity_is_rejected_by_production_work_handler_despite_group_read_grant() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let response = issue_612_blocked_work_list_response(&temp.path().join("group"), [0x44; 16]);

    assert_eq!(response[0], ReticulumGitNode::RES_NOT_FOUND);
    assert_eq!(&response[1..], b"Not found");
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn blocked_work_handler_response_matches_pinned_python() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const REMOTE: [u8; 16] = [0x44; 16];
    let reference = std::env::var_os("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .expect("RETICULUM_PY_REPO must point to the pinned Python checkout");
    let revision = std::process::Command::new("git")
        .arg("-C")
        .arg(&reference)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("read Python reference revision");
    assert!(revision.status.success(), "could not resolve Python reference HEAD");
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        PYTHON_REFERENCE_REVISION,
        "differential must use the pinned Python reference"
    );

    let rust_temp = tempfile::tempdir().expect("Rust fixture directory");
    let rust_response = issue_612_blocked_work_list_response(&rust_temp.path().join("group"), REMOTE);

    let python_temp = tempfile::tempdir().expect("Python fixture directory");
    let python_group = python_temp.path().join("group");
    std::fs::create_dir_all(&python_group).expect("Python group directory");
    let python_repository = python_group.join("repo");
    assert!(std::process::Command::new("git")
        .args(["init", "--bare", "--quiet", python_repository.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python bare repository")
        .success());
    std::fs::write(python_group.with_extension("allowed"), "read:all\n")
        .expect("write Python broad group read permission");

    let script = r#"
import json, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
group_path, remote_hex = sys.argv[1:]
remote = bytes.fromhex(remote_hex)
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {remote: True}
node.identity_aliases = {}
node.config = {}
node.log_request = lambda *_args: None
node.load_repository_group("group", group_path)
response = node.handle_work(
    "/mgmt/work",
    {0: "group/repo", "operation": "list", "scope": "active"},
    1,
    SimpleNamespace(hash=remote),
    0,
)
print(json.dumps({"response": response.hex()}))
"#;
    let python = std::process::Command::new(
        std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
    )
    .env("PYTHONPATH", &reference)
    .arg("-c")
    .arg(script)
    .arg(&python_group)
    .arg(hex::encode(REMOTE))
    .output()
    .expect("run pinned Python production work handler");
    assert!(
        python.status.success(),
        "Python work handler failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&python.stdout)
        .unwrap_or_else(|error| panic!("Python JSON: {error}; stdout={:?}", String::from_utf8_lossy(&python.stdout)));
    let python_response = hex::decode(
        result["response"].as_str().expect("Python response hex"),
    )
    .expect("decode Python response");

    assert_eq!(rust_response, python_response);
    assert_eq!(rust_response[0], ReticulumGitNode::RES_NOT_FOUND);
    assert_eq!(&rust_response[1..], b"Not found");
}

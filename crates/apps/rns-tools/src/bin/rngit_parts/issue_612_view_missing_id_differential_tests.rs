#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_view_missing_document_id_matches_pinned_python_error() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const REMOTE: [u8; 16] = [0x42; 16];
    let reference = std::env::var_os("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .expect("RETICULUM_PY_REPO must point to the pinned Python checkout");
    let revision = Command::new("git")
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

    let rust_temp = tempfile::tempdir().expect("Rust fixture");
    let rust_group = rust_temp.path().join("group");
    let rust_repo = rust_group.join("repo");
    fs::create_dir_all(&rust_group).expect("Rust group");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", rust_repo.to_string_lossy().as_ref()])
        .status()
        .expect("Rust git init")
        .success());
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &rust_group).expect("load Rust group");
    let group = rust_node.groups.get_mut("group").expect("Rust group state");
    group.permissions.read.add(PermissionTarget::All);
    group
        .repositories
        .get_mut("repo")
        .expect("Rust repository state")
        .permissions
        .read
        .add(PermissionTarget::All);

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    let python_repo = python_group.join("repo");
    fs::create_dir_all(&python_group).expect("Python group");

    let script = r#"
import json, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo_path, remote_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo_path}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
response = node.handle_work(
    "/mgmt/work",
    {0: "group/repo", "operation": "view"},
    1,
    SimpleNamespace(hash=bytes.fromhex(remote_hex)),
    0,
)
print(json.dumps({"status": response[0], "body": response[1:].decode("utf-8", "replace")}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&python_repo)
        .arg(hex::encode(REMOTE))
        .output()
        .expect("run pinned Python production work handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let python_result: serde_json::Value = serde_json::from_slice(&python.stdout)
        .unwrap_or_else(|error| panic!("Python JSON: {error}; stdout={:?}", String::from_utf8_lossy(&python.stdout)));

    let request = [
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("view")),
    ];
    let rust_response = rust_node.handle_work_request(&request, REMOTE);
    assert_eq!(python_result["status"], ReticulumGitNode::RES_INVALID_REQ);
    assert_eq!(python_result["body"], "No document ID specified");
    assert_eq!(rust_response[0], python_result["status"].as_u64().expect("Python status") as u8);
    assert_eq!(
        std::str::from_utf8(&rust_response[1..]).expect("Rust error text"),
        python_result["body"].as_str().expect("Python error text")
    );
    assert!(!rust_group.join("repo.work").exists());
    assert!(!python_group.join("repo.work").exists());
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_list_unknown_scope_matches_pinned_python_empty_result() {
    use std::path::PathBuf;

    const PYTHON_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    let reference = PathBuf::from(
        std::env::var_os("RETICULUM_PY_REPO")
            .expect("RETICULUM_PY_REPO must point to the pinned Python checkout"),
    );
    let revision = Command::new("git")
        .arg("-C")
        .arg(&reference)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("read Python reference revision");
    assert!(revision.status.success());
    assert_eq!(String::from_utf8_lossy(&revision.stdout).trim(), PYTHON_REVISION);

    let rust_temp = tempfile::tempdir().expect("Rust fixture");
    let rust_group = rust_temp.path().join("group");
    fs::create_dir_all(&rust_group).expect("Rust group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", rust_group.join("repo").to_string_lossy().as_ref()])
        .status()
        .expect("initialize Rust repository")
        .success());
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &rust_group).expect("load Rust group");
    rust_node
        .groups
        .get_mut("group")
        .expect("Rust group state")
        .permissions
        .read
        .add(PermissionTarget::All);

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    fs::create_dir_all(&python_group).expect("Python group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_group.join("repo").to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    let script = r#"
import json, sys
from RNS.Utilities.rngit.server import ReticulumGitNode
repo = sys.argv[1]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
response = node.handle_work("/mgmt/work", {0: "group/repo", "operation": "list", "scope": "unknown"}, 1, object(), 0)
print(json.dumps({"status": response[0], "body": response[1:].hex()}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(python_group.join("repo"))
        .output()
        .expect("run pinned Python production handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let python: serde_json::Value = serde_json::from_slice(&python.stdout).expect("Python JSON result");

    let request = [
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("list")),
        (rmpv::Value::from("scope"), rmpv::Value::from("unknown")),
    ];
    let rust = rust_node.handle_work_request(&request, [0x42; 16]);
    assert_eq!(python["status"], rust[0]);
    assert_eq!(python["body"], hex::encode(&rust[1..]));
    assert_eq!(rust[0], ReticulumGitNode::RES_OK);
    let payload = rmpv::decode::read_value(&mut std::io::Cursor::new(&rust[1..]))
        .expect("decode Rust list result");
    let payload = payload.as_map().expect("list result map");
    for scope in ["active", "completed", "proposed"] {
        let values = map_value(payload, &rmpv::Value::from(scope)).expect("scope result");
        assert!(values.as_array().is_some_and(Vec::is_empty));
    }
}

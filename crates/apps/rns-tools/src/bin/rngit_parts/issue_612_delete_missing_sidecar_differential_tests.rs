#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_delete_missing_permission_sidecar_matches_pinned_python() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const REMOTE: [u8; 16] = [0x11; 16];
    const DOCUMENT_ID: u64 = 7;

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
        "differential must use the requested pinned Python reference"
    );

    let python_temp = tempfile::tempdir().expect("Python fixture directory");
    let python_group = python_temp.path().join("group");
    let python_repo = python_group.join("repo");
    let python_work = python_group.join("repo.work");
    let python_document = python_work.join("active").join(DOCUMENT_ID.to_string());
    fs::create_dir_all(&python_group).expect("Python group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("Python git init")
        .success());
    fs::create_dir_all(&python_document).expect("Python document directory");
    let seed_python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg("import msgpack, sys; msgpack.pack({'content': 'body', 'meta': {'author': bytes.fromhex('11111111111111111111111111111111')}}, open(sys.argv[1], 'wb'))")
        .arg(python_document.join("root"))
        .status()
        .expect("seed Python work document");
    assert!(seed_python.success(), "could not seed Python document");

    let script = r#"
import json, os, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo_path, doc_id, remote_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo_path}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
request = {0: "group/repo", "operation": "delete", "doc_id": int(doc_id)}
response = node.handle_work("/mgmt/work", request, 1, SimpleNamespace(hash=bytes.fromhex(remote_hex)), 0)
document = os.path.join(repo_path + ".work", "active", doc_id)
print(json.dumps({"status": response[0], "body": response[1:].decode("utf-8", "replace"), "document_exists": os.path.isdir(document)}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&python_repo)
        .arg(DOCUMENT_ID.to_string())
        .arg(hex::encode(REMOTE))
        .output()
        .expect("run pinned Python production work handler");
    assert!(
        python.status.success(),
        "Python work handler failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    let json_line = python
        .stdout
        .split(|byte| *byte == b'\n')
        .rfind(|line| !line.is_empty())
        .expect("Python JSON output line");
    let python_result: serde_json::Value = serde_json::from_slice(json_line)
        .unwrap_or_else(|error| panic!("Python work-handler JSON: {error}; line={:?}", String::from_utf8_lossy(json_line)));

    let rust_temp = tempfile::tempdir().expect("Rust fixture directory");
    let rust_group = rust_temp.path().join("group");
    let rust_repo = rust_group.join("repo");
    fs::create_dir_all(&rust_group).expect("Rust group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", rust_repo.to_string_lossy().as_ref()])
        .status()
        .expect("Rust git init")
        .success());
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &rust_group).expect("load Rust group");
    let permissions = &mut rust_node.groups.get_mut("group").expect("Rust group").permissions;
    permissions.read.add(PermissionTarget::All);
    permissions.write.add(PermissionTarget::All);
    permissions.interact.add(PermissionTarget::All);
    let rust_document = rust_group
        .join("repo.work/active")
        .join(DOCUMENT_ID.to_string());
    fs::create_dir_all(&rust_document).expect("Rust document directory");
    let document = rmpv::Value::Map(vec![
        (rmpv::Value::from("content"), rmpv::Value::from("body")),
        (
            rmpv::Value::from("meta"),
            rmpv::Value::Map(vec![(
                rmpv::Value::from("author"),
                rmpv::Value::Binary(REMOTE.to_vec()),
            )]),
        ),
    ]);
    fs::write(rust_document.join("root"), rngit_work_fixture(&document))
        .expect("write Rust document root");
    let request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("delete")),
        (rmpv::Value::from("doc_id"), rmpv::Value::from(DOCUMENT_ID)),
    ];
    let rust_response = rust_node.handle_work_request(&request, REMOTE);
    let rust_result = serde_json::json!({
        "status": rust_response[0],
        "body": String::from_utf8_lossy(&rust_response[1..]),
        "document_exists": rust_document.is_dir(),
    });

    assert_eq!(
        rust_result["status"],
        ReticulumGitNode::RES_REMOTE_FAIL,
        "missing sidecar must fail before document removal"
    );
    assert_eq!(rust_result["body"], "Remote error");
    assert_eq!(rust_result["document_exists"], true);
    assert_eq!(
        python_result["status"], rust_result["status"],
        "pinned Python and Rust statuses differ"
    );
    assert_eq!(
        python_result["body"], rust_result["body"],
        "pinned Python and Rust response bodies differ"
    );
    assert_eq!(python_result["document_exists"], true);
}

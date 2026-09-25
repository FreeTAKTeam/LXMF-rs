#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn activating_completed_work_without_destination_scope_matches_pinned_python() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const AUTHOR: [u8; 16] = [0x11; 16];
    const ADMIN: [u8; 16] = [0x22; 16];
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
    let group_path = rust_temp.path().join("group");
    fs::create_dir_all(&group_path).expect("Rust group");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", group_path.join("repo").to_string_lossy().as_ref()])
        .status()
        .expect("Rust git init")
        .success());
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &group_path).expect("load Rust group");
    let permissions = &mut rust_node.groups.get_mut("group").expect("Rust group").permissions;
    for permission in [&mut permissions.read, &mut permissions.write, &mut permissions.interact] {
        permission.add(PermissionTarget::All);
    }
    let document_dir = group_path.join("repo.work/completed/4");
    fs::create_dir_all(&document_dir).expect("Rust completed document");
    let document = rmpv::Value::Map(vec![
        (rmpv::Value::from("content"), rmpv::Value::from("authored work")),
        (
            rmpv::Value::from("meta"),
            rmpv::Value::Map(vec![(
                rmpv::Value::from("author"),
                rmpv::Value::Binary(AUTHOR.to_vec()),
            )]),
        ),
    ]);
    fs::write(document_dir.join("root"), rngit_work_fixture(&document))
        .expect("write Rust document");
    fs::write(group_path.join("repo.work/4.allowed"), "write:all\ninteract:all\nadmin:all\n")
        .expect("document admin permissions");

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    let python_repo = python_group.join("repo");
    fs::create_dir_all(&python_group).expect("Python group");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("Python git init")
        .success());
    let python_document = python_group.join("repo.work/completed/4");
    fs::create_dir_all(&python_document).expect("Python completed document");

    let script = r#"
import json, msgpack, os, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo_path, author_hex, admin_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo_path}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
# Isolate activation's destination-scope behavior; resolver parity is tested elsewhere.
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
completed = os.path.join(repo_path + ".work", "completed", "4")
os.makedirs(completed, exist_ok=True)
with open(os.path.join(completed, "root"), "wb") as stream:
    msgpack.pack({"content": "authored work", "meta": {"author": bytes.fromhex(author_hex)}}, stream)
response = node.handle_work(
    "/mgmt/work",
    {0: "group/repo", "operation": "activate", "doc_id": 4},
    1,
    SimpleNamespace(hash=bytes.fromhex(admin_hex)),
    0,
)
print(json.dumps({
    "status": response[0],
    "body": response[1:].hex(),
    "completed_exists": os.path.isdir(completed),
    "active_exists": os.path.isdir(os.path.join(repo_path + ".work", "active", "4")),
}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&python_repo)
        .arg(hex::encode(AUTHOR))
        .arg(hex::encode(ADMIN))
        .output()
        .expect("run pinned Python production work handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let python_result: serde_json::Value = serde_json::from_slice(&python.stdout)
        .unwrap_or_else(|error| panic!("Python JSON: {error}; stdout={:?}", String::from_utf8_lossy(&python.stdout)));
    assert_eq!(python_result["status"], ReticulumGitNode::RES_OK);
    assert_eq!(python_result["completed_exists"], false);
    assert_eq!(python_result["active_exists"], true);

    let request = [
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("activate")),
        (rmpv::Value::from("doc_id"), rmpv::Value::from(4_u64)),
    ];
    let rust_response = rust_node.handle_work_request(&request, ADMIN);
    let rust_completed = document_dir.is_dir();
    let rust_active = group_path.join("repo.work/active/4").is_dir();
    assert_eq!(rust_response[0], python_result["status"].as_u64().expect("Python response status") as u8);
    assert_eq!(hex::encode(&rust_response[1..]), python_result["body"]);
    assert_eq!(rust_completed, python_result["completed_exists"]);
    assert_eq!(rust_active, python_result["active_exists"]);
}

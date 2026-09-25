#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_view_document_id_coercion_matches_pinned_python() {
    use std::path::PathBuf;
    use std::process::Command;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const REMOTE: [u8; 16] = [0x42; 16];
    const DOCUMENT_IDS: [u64; 2] = [0, 7];
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
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        PYTHON_REFERENCE_REVISION
    );

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
    let permissions = &mut rust_node.groups.get_mut("group").expect("Rust group").permissions;
    permissions.read.add(PermissionTarget::All);
    permissions.admin.add(PermissionTarget::All);
    let repository_permissions = &mut rust_node
        .groups
        .get_mut("group")
        .expect("Rust group")
        .repositories
        .get_mut("repo")
        .expect("Rust repository")
        .permissions;
    repository_permissions.read.add(PermissionTarget::All);
    repository_permissions.admin.add(PermissionTarget::All);
    for document_id in DOCUMENT_IDS {
        let rust_document = rust_group.join("repo.work/active").join(document_id.to_string());
        fs::create_dir_all(&rust_document).expect("Rust work directory");
        let rust_value = rmpv::Value::Map(vec![
            (rmpv::Value::from("content"), rmpv::Value::from(format!("document {document_id}"))),
            (
                rmpv::Value::from("meta"),
                rmpv::Value::Map(vec![
                    (rmpv::Value::from("author"), rmpv::Value::Binary(REMOTE.to_vec())),
                    (rmpv::Value::from("created"), rmpv::Value::from(1_u64)),
                    (rmpv::Value::from("edited"), rmpv::Value::from(2_u64)),
                ]),
            ),
        ]);
        let mut rust_bytes = Vec::new();
        rmpv::encode::write_value(&mut rust_bytes, &rust_value).expect("encode Rust fixture");
        fs::write(rust_document.join("root"), rust_bytes).expect("seed Rust work document");
    }

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    fs::create_dir_all(&python_group).expect("Python group directory");
    let python_repo = python_group.join("repo");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    for document_id in DOCUMENT_IDS {
        let python_document = python_group.join("repo.work/active").join(document_id.to_string());
        fs::create_dir_all(&python_document).expect("Python work directory");
        let seed = Command::new(
        std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
    )
    .env("PYTHONPATH", &reference)
    .args([
        "-c",
        "import msgpack, sys; msgpack.pack({'content':'document '+sys.argv[2],'meta':{'author':bytes.fromhex(sys.argv[3]),'created':1,'edited':2}}, open(sys.argv[1], 'wb'))",
    ])
            .arg(python_document.join("root"))
            .arg(document_id.to_string())
            .arg(hex::encode(REMOTE))
            .status()
            .expect("seed Python work document");
        assert!(seed.success());
    }

    let python_script = r#"
import json, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo, remote_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
identity = SimpleNamespace(hash=bytes.fromhex(remote_hex))
responses = [node.handle_work("/mgmt/work", {
    0: "group/repo", "operation": "view", "doc_id": doc_id,
}, 1, identity, 0) for doc_id in (7.9, -0.1, "not-an-id")]
print(json.dumps([{"status": response[0], "body": response[1:].hex()} for response in responses]))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(python_script)
        .arg(&python_repo)
        .arg(hex::encode(REMOTE))
        .output()
        .expect("run pinned Python production work handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let python: serde_json::Value = serde_json::from_slice(&python.stdout).expect("Python response JSON");

    for (doc_id, python) in [
        (rmpv::Value::F64(7.9), &python[0]),
        (rmpv::Value::F64(-0.1), &python[1]),
        (rmpv::Value::from("not-an-id"), &python[2]),
    ] {
        let request = [
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from("view")),
            (rmpv::Value::from("doc_id"), doc_id),
        ];
        let rust_response = rust_node.handle_work_request(&request, REMOTE);
        assert_eq!(rust_response[0], python["status"].as_u64().expect("Python status") as u8);
        assert_eq!(hex::encode(&rust_response[1..]), python["body"].as_str().expect("Python body"));
    }
    assert_eq!(python[0]["status"], ReticulumGitNode::RES_OK);
    assert_eq!(python[1]["status"], ReticulumGitNode::RES_OK);
    assert_eq!(python[2]["status"], ReticulumGitNode::RES_INVALID_REQ);
    assert_eq!(python[2]["body"], hex::encode("Invalid request"));
}

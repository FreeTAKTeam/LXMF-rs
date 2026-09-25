#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_comment_binary_content_matches_pinned_python_and_persists_bytes() {
    use std::path::PathBuf;
    use std::process::Command;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const REMOTE: [u8; 16] = [0x62; 16];
    const DOCUMENT_ID: u64 = 7;
    const CONTENT: &[u8] = b" \tbinary comment \t";
    const STORED_CONTENT: &[u8] = b"binary comment";
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
    assert!(revision.status.success(), "could not resolve Python reference HEAD");
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        PYTHON_REFERENCE_REVISION,
        "differential must use the pinned RNS 1.5.4 reference"
    );

    let rust_temp = tempfile::tempdir().expect("Rust fixture");
    let rust_group = rust_temp.path().join("group");
    let rust_repo = rust_group.join("repo");
    fs::create_dir_all(&rust_group).expect("Rust group");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", rust_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Rust repository")
        .success());
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &rust_group).expect("load Rust group");
    let permissions = &mut rust_node.groups.get_mut("group").expect("Rust group").permissions;
    permissions.read.add(PermissionTarget::All);
    permissions.write.add(PermissionTarget::All);
    permissions.interact.add(PermissionTarget::All);
    let rust_document = rust_group.join("repo.work/active").join(DOCUMENT_ID.to_string());
    fs::create_dir_all(&rust_document).expect("Rust document");
    let root = rmpv::Value::Map(vec![
        (rmpv::Value::from("content"), rmpv::Value::from("body")),
        (
            rmpv::Value::from("meta"),
            rmpv::Value::Map(vec![(
                rmpv::Value::from("author"),
                rmpv::Value::Binary(REMOTE.to_vec()),
            )]),
        ),
    ]);
    fs::write(rust_document.join("root"), rngit_work_fixture(&root)).expect("seed Rust root");

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    let python_repo = python_group.join("repo");
    let python_document = python_group
        .join("repo.work/active")
        .join(DOCUMENT_ID.to_string());
    fs::create_dir_all(&python_group).expect("Python group");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    fs::create_dir_all(&python_document).expect("Python document");
    let seed = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg("import msgpack, sys; msgpack.pack({'content':'body','meta':{'author':bytes.fromhex(sys.argv[2])}}, open(sys.argv[1], 'wb'))")
        .arg(python_document.join("root"))
        .arg(hex::encode(REMOTE))
        .status()
        .expect("seed Python root");
    assert!(seed.success(), "could not seed pinned Python fixture");

    let python_script = r#"
import json, os, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode, mp
repo, remote_hex, doc_id, content_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
request = {0: "group/repo", "operation": "comment", "doc_id": int(doc_id), "scope": "active", "content": bytes.fromhex(content_hex)}
response = node.handle_work("/mgmt/work", request, 1, SimpleNamespace(hash=bytes.fromhex(remote_hex)), 0)
with open(os.path.join(repo + ".work", "active", doc_id, "1"), "rb") as file:
    comment = mp.unpackb(file.read())
print(json.dumps({"status": response[0], "body": response[1:].hex(), "comment_content": comment["content"].hex() if isinstance(comment["content"], bytes) else comment["content"]}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(python_script)
        .arg(&python_repo)
        .arg(hex::encode(REMOTE))
        .arg(DOCUMENT_ID.to_string())
        .arg(hex::encode(CONTENT))
        .output()
        .expect("run pinned Python production handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let python: serde_json::Value = serde_json::from_slice(&python.stdout).expect("Python result JSON");

    let request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("comment")),
        (rmpv::Value::from("doc_id"), rmpv::Value::from(DOCUMENT_ID)),
        (rmpv::Value::from("scope"), rmpv::Value::from("active")),
        (rmpv::Value::from("content"), rmpv::Value::Binary(CONTENT.to_vec())),
    ];
    let response = rust_node.handle_work_request(&request, REMOTE);
    assert_eq!(response[0], python["status"].as_u64().expect("Python status") as u8);
    assert_eq!(hex::encode(&response[1..]), python["body"].as_str().expect("Python response body"));
    let mut comment_bytes = fs::read(rust_document.join("1")).expect("read Rust comment");
    let rust_comment = rmpv::decode::read_value(&mut std::io::Cursor::new(&mut comment_bytes))
        .expect("decode Rust comment");
    let rust_content = rust_comment
        .as_map()
        .and_then(|map| map.iter().find(|(key, _)| key.as_str() == Some("content")))
        .map(|(_, value)| value)
        .expect("comment content");
    assert_eq!(rust_content.as_slice(), Some(STORED_CONTENT));
    assert_eq!(python["status"], ReticulumGitNode::RES_OK);
    assert_eq!(python["comment_content"], hex::encode(STORED_CONTENT));
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_comment_malformed_content_matches_pinned_python_without_mutation() {
    use std::path::PathBuf;
    use std::process::Command;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const REMOTE: [u8; 16] = [0x61; 16];
    const DOCUMENT_ID: u64 = 7;
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
        "differential must use the pinned Python reference"
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
    let root_bytes = rngit_work_fixture(&document);
    fs::write(rust_document.join("root"), &root_bytes).expect("seed Rust document");

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
    let seed_python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg("import msgpack, sys; msgpack.pack({'content':'body','meta':{'author':bytes.fromhex(sys.argv[2])}}, open(sys.argv[1], 'wb'))")
        .arg(python_document.join("root"))
        .arg(hex::encode(REMOTE))
        .status()
        .expect("seed Python document");
    assert!(seed_python.success(), "could not seed Python document");

    let python_script = r#"
import json, os, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo, remote_hex, doc_id = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
request = {0: "group/repo", "operation": "comment", "doc_id": int(doc_id), "scope": "active", "content": ["not", "a", "string"]}
response = node.handle_work("/mgmt/work", request, 1, SimpleNamespace(hash=bytes.fromhex(remote_hex)), 0)
directory = os.path.join(repo + ".work", "active", doc_id)
files = sorted((name, open(os.path.join(directory, name), "rb").read().hex()) for name in os.listdir(directory))
print(json.dumps({"status": response[0], "body": response[1:].decode("utf-8", "replace"), "files": files}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(python_script)
        .arg(&python_repo)
        .arg(hex::encode(REMOTE))
        .arg(DOCUMENT_ID.to_string())
        .output()
        .expect("run pinned Python production work handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let json_line = python
        .stdout
        .split(|byte| *byte == b'\n')
        .rfind(|line| !line.is_empty())
        .expect("Python JSON output line");
    let python_result: serde_json::Value = serde_json::from_slice(json_line)
        .unwrap_or_else(|error| panic!("Python JSON: {error}; line={:?}", String::from_utf8_lossy(json_line)));
    assert_eq!(python_result["status"], ReticulumGitNode::RES_REMOTE_FAIL);
    assert_eq!(python_result["body"], "Remote error");
    assert_eq!(python_result["files"], serde_json::json!([["root", hex::encode(&root_bytes)]]));

    let request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("comment")),
        (rmpv::Value::from("doc_id"), rmpv::Value::from(DOCUMENT_ID)),
        (rmpv::Value::from("scope"), rmpv::Value::from("active")),
        (
            rmpv::Value::from("content"),
            rmpv::Value::Array(vec![rmpv::Value::from("not"), rmpv::Value::from("a"), rmpv::Value::from("string")]),
        ),
    ];
    let response = rust_node.handle_work_request(&request, REMOTE);
    assert_eq!(response[0], python_result["status"].as_u64().expect("Python status") as u8);
    assert_eq!(std::str::from_utf8(&response[1..]).expect("Rust response"), python_result["body"]);
    let rust_files = fs::read_dir(&rust_document)
        .expect("read Rust document files")
        .map(|entry| {
            let entry = entry.expect("Rust document entry");
            (entry.file_name().to_string_lossy().into_owned(), hex::encode(fs::read(entry.path()).expect("read Rust document file")))
        })
        .collect::<Vec<_>>();
    assert_eq!(rust_files, vec![("root".to_string(), hex::encode(&root_bytes))]);
}

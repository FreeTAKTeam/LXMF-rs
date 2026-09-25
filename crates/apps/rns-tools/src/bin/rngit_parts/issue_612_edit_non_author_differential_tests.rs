#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_edit_by_non_author_matches_pinned_python_without_mutation() {
    use std::path::PathBuf;
    use std::process::Command;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const AUTHOR: [u8; 16] = [0x41; 16];
    const REMOTE: [u8; 16] = [0x52; 16];
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
    let rust_document = rust_group.join("repo.work/active/7");
    fs::create_dir_all(&rust_document).expect("Rust work document");
    let root_bytes = rngit_work_fixture(&rmpv::Value::Map(vec![
        (rmpv::Value::from("content"), rmpv::Value::from("original body")),
        (
            rmpv::Value::from("meta"),
            rmpv::Value::Map(vec![(
                rmpv::Value::from("author"),
                rmpv::Value::Binary(AUTHOR.to_vec()),
            )]),
        ),
    ]));
    fs::write(rust_document.join("root"), &root_bytes).expect("seed Rust work document");
    fs::write(rust_group.join("repo.work/7.allowed"), "read:all\nwrite:all\ninteract:all\n")
        .expect("grant document access");

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    let python_repo = python_group.join("repo");
    fs::create_dir_all(&python_group).expect("Python group");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    let python_document = python_group.join("repo.work/active/7");
    fs::create_dir_all(&python_document).expect("Python work document");
    let seed_python = Command::new(
        std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
    )
    .env("PYTHONPATH", &reference)
    .arg("-c")
    .arg("import msgpack, sys; msgpack.pack({'content':'original body','meta':{'author':bytes.fromhex(sys.argv[2])}}, open(sys.argv[1], 'wb'))")
    .arg(python_document.join("root"))
    .arg(hex::encode(AUTHOR))
    .status()
    .expect("seed Python work document");
    assert!(seed_python.success(), "could not seed Python work document");

    let python_script = r#"
import json, os, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo, remote_hex, author_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
remote = SimpleNamespace(
    hash=bytes.fromhex(remote_hex),
    validate=lambda *_args: True,
    get_public_key=lambda: bytes(64),
)
request = {
    0: "group/repo", "operation": "edit", "doc_id": 7, "scope": "active",
    "title": "replacement title", "content": "replacement body",
    "signature": bytes([0x63] * 64),
}
response = node.handle_work("/mgmt/work", request, 1, remote, 0)
directory = os.path.join(repo + ".work", "active", "7")
print(json.dumps({
    "status": response[0],
    "body": response[1:].hex(),
    "files": sorted(os.listdir(directory)),
    "root": open(os.path.join(directory, "root"), "rb").read().hex(),
}))
"#;
    let python =
        Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
            .env("PYTHONPATH", &reference)
            .arg("-c")
            .arg(python_script)
            .arg(&python_repo)
            .arg(hex::encode(REMOTE))
            .arg(hex::encode(AUTHOR))
            .output()
            .expect("run pinned Python production work handler");
    assert!(
        python.status.success(),
        "Python handler failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    let python: serde_json::Value =
        serde_json::from_slice(&python.stdout).unwrap_or_else(|error| {
            panic!("Python JSON: {error}; stdout={:?}", String::from_utf8_lossy(&python.stdout))
        });

    let request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("edit")),
        (rmpv::Value::from("doc_id"), rmpv::Value::from(7_u64)),
        (rmpv::Value::from("scope"), rmpv::Value::from("active")),
        (rmpv::Value::from("title"), rmpv::Value::from("replacement title")),
        (rmpv::Value::from("content"), rmpv::Value::from("replacement body")),
        (rmpv::Value::from("signature"), rmpv::Value::Binary(vec![0x63; 64])),
    ];
    let rust_response = rust_node.handle_work_request(&request, REMOTE);
    let rust_files = fs::read_dir(&rust_document)
        .expect("read Rust work document files")
        .map(|entry| entry.expect("Rust entry").file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert_eq!(
        python["status"],
        ReticulumGitNode::RES_DISALLOWED,
        "pinned Python should deny a non-author edit"
    );
    assert_eq!(rust_response[0], python["status"].as_u64().expect("Python status") as u8);
    assert_eq!(hex::encode(&rust_response[1..]), python["body"]);
    assert_eq!(serde_json::json!(rust_files), python["files"]);
    assert_eq!(fs::read(rust_document.join("root")).expect("Rust root after request"), root_bytes);
    assert_eq!(hex::encode(&root_bytes), python["root"]);
}

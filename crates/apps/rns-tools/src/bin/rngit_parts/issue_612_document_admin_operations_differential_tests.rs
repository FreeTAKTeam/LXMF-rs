const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
const REMOTE: [u8; 16] = [0x33; 16];
const OTHER_AUTHOR: [u8; 16] = [0x44; 16];

struct WorkOperationOutcome {
    status: u8,
    body: Vec<u8>,
    document_exists: bool,
    comment_exists: bool,
    root_content: Option<String>,
    permission_content: Option<String>,
}

fn assert_pinned_python_revision() -> std::path::PathBuf {
    let reference = std::env::var_os("RETICULUM_PY_REPO")
        .map(std::path::PathBuf::from)
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
        PYTHON_REFERENCE_REVISION
    );
    reference
}

fn initialize_work_operation_fixture(
    group_path: &std::path::Path,
    group_permissions: &str,
    document_permissions: &str,
) {
    fs::create_dir_all(group_path).expect("create group directory");
    let repository_path = group_path.join("repo");
    assert!(std::process::Command::new("git")
        .args(["init", "--bare", "--quiet", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("initialize bare repository")
        .success());
    fs::write(group_path.with_extension("allowed"), group_permissions)
        .expect("write group permissions");
    let work_document = group_path.join("repo.work/active/7");
    fs::create_dir_all(&work_document).expect("create active work item");
    fs::write(
        work_document.join("root"),
        rngit_work_fixture(&rmpv::Value::Map(vec![
            (rmpv::Value::from("content"), rmpv::Value::from("original body")),
            (
                rmpv::Value::from("meta"),
                rmpv::Value::Map(vec![(
                    rmpv::Value::from("author"),
                    rmpv::Value::Binary(REMOTE.to_vec()),
                )]),
            ),
        ])),
    )
        .expect("write work root");
    fs::write(group_path.join("repo.work/7.allowed"), document_permissions)
        .expect("write document permissions");
}

fn request_for_work_operation(operation: &str) -> Vec<(rmpv::Value, rmpv::Value)> {
    let mut request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from(operation)),
        (rmpv::Value::from("doc_id"), rmpv::Value::from(7_u64)),
        (rmpv::Value::from("scope"), rmpv::Value::from("active")),
    ];
    match operation {
        "comment" => request.push((rmpv::Value::from("content"), rmpv::Value::from("admin comment"))),
        "edit" => request.extend([
            (rmpv::Value::from("title"), rmpv::Value::from("Edited title")),
            (rmpv::Value::from("content"), rmpv::Value::from("edited body")),
            (rmpv::Value::from("signature"), rmpv::Value::Binary(vec![0x55; 64])),
        ]),
        "perms" => request.push((rmpv::Value::from("step"), rmpv::Value::from("get"))),
        "delete" => {}
        _ => panic!("unsupported test operation {operation}"),
    }
    request
}

fn python_operation_outcome(
    reference: &std::path::Path,
    group_path: &std::path::Path,
    operation: &str,
    group_permissions: &str,
    document_permissions: &str,
) -> serde_json::Value {
    let script = r#"
import json, os, sys
from threading import Lock
from types import SimpleNamespace
sys.modules["msgpack"] = None
from RNS.vendor import umsgpack
from RNS.Utilities.rngit.server import ReticulumGitNode
group_path, operation, remote_hex, group_permissions, document_permissions = sys.argv[1:]
with open(group_path + ".allowed", "w") as stream:
    stream.write(group_permissions)
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {}
node.identity_aliases = {}
node.config = {}
node.perms_lock = Lock()
node.log_request = lambda *args: None
node.load_repository_group("group", group_path)
with open(os.path.join(group_path, "repo.work", "7.allowed"), "w") as stream:
    stream.write(document_permissions)
remote = SimpleNamespace(
    hash=bytes.fromhex(remote_hex),
    validate=lambda *_args: True,
    get_public_key=lambda: bytes(64),
)
request = {0: "group/repo", "operation": operation, "doc_id": 7, "scope": "active"}
if operation == "comment":
    request["content"] = "admin comment"
elif operation == "edit":
    request.update({"title": "Edited title", "content": "edited body", "signature": bytes([0x55] * 64)})
elif operation == "perms":
    request["step"] = "get"
response = node.handle_work("/mgmt/work", request, 1, remote, 0)
work_item = os.path.join(group_path, "repo.work", "active", "7")
root_content = None
root_path = os.path.join(work_item, "root")
if os.path.isfile(root_path):
    with open(root_path, "rb") as stream:
        root_content = umsgpack.unpackb(stream.read()).get("content")
permission_path = os.path.join(group_path, "repo.work", "7.allowed")
print(json.dumps({
    "status": response[0],
    "body": response[1:].hex(),
    "document_exists": os.path.isdir(work_item),
    "comment_exists": os.path.isfile(os.path.join(work_item, "1")),
    "root_content": root_content,
    "permission_content": open(permission_path).read() if os.path.isfile(permission_path) else None,
}))
"#;
    let output = std::process::Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", reference)
        .arg("-c")
        .arg(script)
        .arg(group_path)
        .arg(operation)
    .arg(hex::encode(REMOTE))
    .arg(group_permissions)
    .arg(document_permissions)
        .output()
        .expect("run pinned Python production work handler");
    assert!(output.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("Python operation JSON")
}

fn assert_work_operation_differential(
    operation: &str,
    group_permissions: &str,
    document_permissions: &str,
    document_author: [u8; 16],
) -> (WorkOperationOutcome, serde_json::Value) {
    let reference = assert_pinned_python_revision();
    let rust_temp = tempfile::tempdir().expect("Rust fixture root");
    let rust_group = rust_temp.path().join("group");
    initialize_work_operation_fixture(&rust_group, group_permissions, document_permissions);
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &rust_group).expect("load Rust group");
    if document_author != REMOTE {
        let root_path = rust_group.join("repo.work/active/7/root");
        let mut document = rmpv::decode::read_value(&mut std::io::Cursor::new(
            fs::read(&root_path).expect("read Rust work root"),
        ))
        .expect("decode Rust root");
        if let rmpv::Value::Map(document_map) = &mut document {
            if let Some((_, rmpv::Value::Map(metadata))) = document_map
                .iter_mut()
                .find(|(key, _)| key == &rmpv::Value::from("meta"))
            {
                if let Some((_, author)) = metadata
                    .iter_mut()
                    .find(|(key, _)| key == &rmpv::Value::from("author"))
                {
                    *author = rmpv::Value::Binary(document_author.to_vec());
                }
            }
        }
        fs::write(&root_path, rngit_work_fixture(&document)).expect("write Rust author fixture");
    }
    let rust_response = rust_node.handle_work_request(&request_for_work_operation(operation), REMOTE);
    let rust_work_item = rust_group.join("repo.work/active/7");
    let root_content = fs::read(rust_work_item.join("root"))
        .ok()
        .and_then(|bytes| rmpv::decode::read_value(&mut std::io::Cursor::new(bytes)).ok())
        .and_then(|document| {
            document
                .as_map()
                .and_then(|map| map_value(map, &rmpv::Value::from("content")))
                .and_then(rmpv::Value::as_str)
                .map(str::to_owned)
        });
    let rust_outcome = WorkOperationOutcome {
        status: rust_response[0],
        body: rust_response[1..].to_vec(),
        document_exists: rust_work_item.is_dir(),
        comment_exists: rust_work_item.join("1").is_file(),
        root_content,
        permission_content: fs::read_to_string(rust_group.join("repo.work/7.allowed")).ok(),
    };

    let python_temp = tempfile::tempdir().expect("Python fixture root");
    let python_group = python_temp.path().join("group");
    initialize_work_operation_fixture(&python_group, group_permissions, document_permissions);
    if document_author != REMOTE {
        let root_path = python_group.join("repo.work/active/7/root");
        let mut document = rmpv::decode::read_value(&mut std::io::Cursor::new(
            fs::read(&root_path).expect("read Python-shaped root"),
        ))
        .expect("decode Python-shaped root");
        if let rmpv::Value::Map(document_map) = &mut document {
            if let Some((_, rmpv::Value::Map(metadata))) = document_map
                .iter_mut()
                .find(|(key, _)| key == &rmpv::Value::from("meta"))
            {
                if let Some((_, author)) = metadata
                    .iter_mut()
                    .find(|(key, _)| key == &rmpv::Value::from("author"))
                {
                    *author = rmpv::Value::Binary(document_author.to_vec());
                }
            }
        }
        fs::write(&root_path, rngit_work_fixture(&document)).expect("write Python author fixture");
    }
    let python = python_operation_outcome(
        &reference,
        &python_group,
        operation,
        group_permissions,
        document_permissions,
    );
    assert_eq!(
        rust_outcome.status,
        python["status"].as_u64().expect("Python status") as u8,
        "{operation} status differs"
    );
    assert_eq!(
        hex::encode(&rust_outcome.body),
        python["body"].as_str().expect("Python response body"),
        "{operation} response differs"
    );
    assert_eq!(rust_outcome.document_exists, python["document_exists"], "{operation} document side effect differs");
    assert_eq!(rust_outcome.comment_exists, python["comment_exists"], "{operation} comment side effect differs");
    assert_eq!(rust_outcome.root_content, python["root_content"].as_str().map(str::to_owned), "{operation} content differs");
    assert_eq!(rust_outcome.permission_content, python["permission_content"].as_str().map(str::to_owned), "{operation} permission sidecar differs");
    (rust_outcome, python)
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn repository_admin_comment_uses_pinned_python_document_read_gate() {
    let permissions = format!(
        "read:all\nadmin:{}\ninteract:{}\nwrite:none\n",
        hex::encode(REMOTE),
        hex::encode(REMOTE)
    );
    let (rust, python) = assert_work_operation_differential("comment", &permissions, "read:none\n", REMOTE);
    assert_eq!(python["status"], ReticulumGitNode::RES_OK);
    assert_eq!(rust.status, ReticulumGitNode::RES_OK);
    assert!(rust.comment_exists);
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn repository_admin_edit_uses_pinned_python_document_read_gate() {
    let permissions = format!(
        "read:all\nadmin:{}\nwrite:{}\ninteract:{}\n",
        hex::encode(REMOTE),
        hex::encode(REMOTE),
        hex::encode(REMOTE)
    );
    let (rust, python) = assert_work_operation_differential("edit", &permissions, "read:none\n", REMOTE);
    assert_eq!(python["status"], ReticulumGitNode::RES_OK);
    assert_eq!(rust.status, ReticulumGitNode::RES_OK);
    assert_eq!(rust.root_content.as_deref(), Some("edited body"));
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn repository_admin_delete_uses_pinned_python_document_read_gate() {
    let permissions = format!(
        "read:all\nadmin:{}\nwrite:{}\ninteract:{}\n",
        hex::encode(REMOTE),
        hex::encode(REMOTE),
        hex::encode(REMOTE)
    );
    let (rust, python) = assert_work_operation_differential("delete", &permissions, "read:none\n", OTHER_AUTHOR);
    assert_eq!(python["status"], ReticulumGitNode::RES_OK);
    assert_eq!(rust.status, ReticulumGitNode::RES_OK);
    assert!(!rust.document_exists);
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn repository_admin_permissions_get_uses_pinned_python_document_read_gate() {
    let permissions = format!("read:all\nadmin:{}\n", hex::encode(REMOTE));
    let (rust, python) = assert_work_operation_differential("perms", &permissions, "read:none\n", REMOTE);
    assert_eq!(python["status"], ReticulumGitNode::RES_OK);
    assert_eq!(rust.status, ReticulumGitNode::RES_OK);
    assert_eq!(rust.permission_content.as_deref(), Some("read:none\n"));
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn document_author_permissions_get_requires_repository_admin_like_pinned_python() {
    let permissions = "read:all\nwrite:all\ninteract:all\nadmin:none\n";
    let document_permissions = format!(
        "read:{}\nwrite:{}\ninteract:{}\n",
        hex::encode(REMOTE),
        hex::encode(REMOTE),
        hex::encode(REMOTE)
    );
    let (rust, python) = assert_work_operation_differential(
        "perms",
        permissions,
        &document_permissions,
        REMOTE,
    );
    assert_eq!(python["status"], ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(rust.status, ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(rust.permission_content.as_deref(), Some(document_permissions.as_str()));
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn document_author_permissions_get_requires_repository_write_and_interact_like_pinned_python() {
    let permissions = "read:all\nwrite:none\ninteract:none\nadmin:none\n";
    let document_permissions = format!(
        "read:{}\nwrite:{}\ninteract:{}\n",
        hex::encode(REMOTE),
        hex::encode(REMOTE),
        hex::encode(REMOTE)
    );
    let (rust, python) = assert_work_operation_differential(
        "perms",
        permissions,
        &document_permissions,
        REMOTE,
    );
    assert_eq!(python["status"], ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(rust.status, ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(rust.permission_content.as_deref(), Some(document_permissions.as_str()));
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn repository_admin_author_permissions_get_still_requires_repository_write_and_interact() {
    let permissions = format!(
        "read:all\nwrite:none\ninteract:none\nadmin:{}\n",
        hex::encode(REMOTE)
    );
    let document_permissions = format!(
        "read:{}\nwrite:{}\ninteract:{}\n",
        hex::encode(REMOTE),
        hex::encode(REMOTE),
        hex::encode(REMOTE)
    );
    let (rust, python) = assert_work_operation_differential(
        "perms",
        &permissions,
        &document_permissions,
        REMOTE,
    );
    assert_eq!(python["status"], ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(rust.status, ReticulumGitNode::RES_DISALLOWED);
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn document_write_access_without_read_matches_pinned_python_edit_gate() {
    let permissions = "read:all\nwrite:all\ninteract:all\n";
    let document_permissions = format!(
        "read:none\nwrite:{}\ninteract:{}\n",
        hex::encode(REMOTE),
        hex::encode(REMOTE)
    );
    let (rust, python) = assert_work_operation_differential(
        "edit",
        permissions,
        &document_permissions,
        REMOTE,
    );
    assert_eq!(python["status"], ReticulumGitNode::RES_NOT_FOUND);
    assert_eq!(python["body"], hex::encode("Document not found"));
    assert_eq!(rust.status, ReticulumGitNode::RES_NOT_FOUND);
    assert_eq!(rust.root_content.as_deref(), Some("original body"));
}

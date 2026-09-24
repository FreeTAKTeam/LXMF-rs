#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_view_edit_comment_ignore_valid_scope_in_pinned_python_order() {
    use std::io::Cursor;
    use std::path::PathBuf;

    const PYTHON_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const DOC_ID: u64 = 41;
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

    let signer = rns_transport::identity::PrivateIdentity::new_from_name("issue-612-scope-order");
    let remote: [u8; 16] = signer.address_hash().as_slice().try_into().expect("identity hash");
    let document = rmpv::Value::Map(vec![
        (rmpv::Value::from("content"), rmpv::Value::from("original body")),
        (
            rmpv::Value::from("meta"),
            rmpv::Value::Map(vec![
                (rmpv::Value::from("author"), rmpv::Value::Binary(remote.to_vec())),
                (rmpv::Value::from("title"), rmpv::Value::from("Original")),
            ]),
        ),
    ]);

    let rust_temp = tempfile::tempdir().expect("Rust fixture root");
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
    for permission in [&mut permissions.read, &mut permissions.write, &mut permissions.interact] {
        permission.add(PermissionTarget::All);
    }
    let rust_work = rust_group.join("repo.work");
    let rust_active = rust_work.join(format!("active/{DOC_ID}"));
    fs::create_dir_all(&rust_active).expect("Rust active document directory");
    fs::write(rust_active.join("root"), rngit_work_fixture(&document))
        .expect("write Rust active document");
    fs::write(rust_work.join(format!("{DOC_ID}.allowed")), "write:all\ninteract:all\n")
        .expect("write Rust document permissions");

    let python_temp = tempfile::tempdir().expect("Python fixture root");
    let python_group = python_temp.path().join("group");
    fs::create_dir_all(&python_group).expect("Python group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_group.join("repo").to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    let public_key = [
        signer.as_identity().public_key_bytes().as_slice(),
        signer.as_identity().verifying_key_bytes().as_slice(),
    ]
    .concat();
    let edited = "signed edited body";
    let signature = signer.sign(edited.as_bytes()).to_bytes().to_vec();
    let python_script = r#"
import json, os, sys
from RNS.Utilities.rngit.server import ReticulumGitNode, mp
from RNS.Identity import Identity
repo, public_hex, doc_id, edited, signature_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
identity = Identity(create_keys=False)
assert identity.load_public_key(bytes.fromhex(public_hex))
work = repo + ".work"
active = os.path.join(work, "active", doc_id)
os.makedirs(active, exist_ok=True)
with open(os.path.join(active, "root"), "wb") as stream:
    mp.pack({"content": "original body", "meta": {"author": identity.hash, "title": "Original"}}, stream)
base = {0: "group/repo", "doc_id": int(doc_id), "scope": "completed"}
view = node.handle_work("/mgmt/work", dict(base, operation="view"), 1, identity, 0)
edit = node.handle_work("/mgmt/work", dict(base, operation="edit", content=edited,
    signature=bytes.fromhex(signature_hex)), 1, identity, 0)
comment = node.handle_work("/mgmt/work", dict(base, operation="comment", content="scope-order comment"), 1, identity, 0)
with open(os.path.join(active, "root"), "rb") as stream:
    active_doc = mp.unpackb(stream.read())
print(json.dumps({
    "statuses": [view[0], edit[0], comment[0]],
    "view": mp.unpackb(view[1:]),
    "edit_payload_length": len(edit[1:]),
    "comment": mp.unpackb(comment[1:]),
    "active_content": active_doc["content"],
    "completed_exists": os.path.exists(os.path.join(work, "completed", doc_id)),
    "active_comment_ids": sorted(name for name in os.listdir(active) if name.isdigit()),
}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(python_script)
        .arg(python_group.join("repo"))
        .arg(hex::encode(public_key))
        .arg(DOC_ID.to_string())
        .arg(edited)
        .arg(hex::encode(&signature))
        .output()
        .expect("run pinned Python production work handlers");
    assert!(
        python.status.success(),
        "Python handler failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    let python: serde_json::Value = serde_json::from_slice(&python.stdout).expect("Python result JSON");

    let request = |operation: &str| {
        vec![
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from(operation)),
            (rmpv::Value::from("doc_id"), rmpv::Value::from(DOC_ID)),
            (rmpv::Value::from("scope"), rmpv::Value::from("completed")),
        ]
    };
    let rust_view = rust_node.handle_work_request_with_peer_identity(
        &request("view"), remote, Some(*signer.as_identity()),
    );
    let mut edit_request = request("edit");
    edit_request.extend([
        (rmpv::Value::from("content"), rmpv::Value::from(edited)),
        (rmpv::Value::from("signature"), rmpv::Value::Binary(signature)),
    ]);
    let rust_edit = rust_node.handle_work_request_with_peer_identity(
        &edit_request, remote, Some(*signer.as_identity()),
    );
    let mut comment_request = request("comment");
    comment_request.push((rmpv::Value::from("content"), rmpv::Value::from("scope-order comment")));
    let rust_comment = rust_node.handle_work_request(&comment_request, remote);

    assert_eq!(python["statuses"], serde_json::json!([0, 0, 0]));
    assert_eq!([rust_view[0], rust_edit[0], rust_comment[0]], [0, 0, 0]);
    let rust_view_payload = rmpv::decode::read_value(&mut Cursor::new(&rust_view[1..]))
        .expect("Rust view payload");
    let python_view = &python["view"];
    assert_eq!(rust_view_payload.as_map().map(Vec::len), python_view.as_object().map(serde_json::Map::len));
    assert_eq!(rust_view_payload["id"].as_u64(), python_view["id"].as_u64());
    assert_eq!(rust_view_payload["scope"].as_str(), python_view["scope"].as_str());
    assert_eq!(rust_view_payload["content"].as_str(), python_view["content"].as_str());
    assert_eq!(
        rust_view_payload["comments"].as_array().map(Vec::len),
        python_view["comments"].as_array().map(Vec::len)
    );
    let rust_meta = &rust_view_payload["meta"];
    let python_meta = &python_view["meta"];
    assert_eq!(rust_meta.as_map().map(Vec::len), python_meta.as_object().map(serde_json::Map::len));
    assert_eq!(rust_meta["title"].as_str(), python_meta["title"].as_str());
    assert_eq!(rust_meta["author"].as_str(), python_meta["author"].as_str());
    assert_eq!(rust_meta["format"].as_str(), python_meta["format"].as_str());
    assert_eq!(rust_meta["created"].as_u64(), python_meta["created"].as_u64());
    assert_eq!(rust_meta["edited"].as_u64(), python_meta["edited"].as_u64());
    assert_eq!(rust_meta["identity"].is_nil(), python_meta["identity"].is_null());
    assert_eq!(rust_meta["signature"].is_nil(), python_meta["signature"].is_null());
    assert_eq!(
        rust_edit.len() - 1,
        python["edit_payload_length"]
            .as_u64()
            .expect("edit payload length") as usize
    );
    assert_eq!(rust_edit.len(), 1, "successful Python edit is status-only");
    let rust_comment_payload = rmpv::decode::read_value(&mut Cursor::new(&rust_comment[1..]))
        .expect("Rust comment payload");
    assert_eq!(rust_comment_payload.as_map().map(Vec::len), python["comment"].as_object().map(serde_json::Map::len));
    assert_eq!(rust_comment_payload["id"].as_u64(), python["comment"]["id"].as_u64());
    assert_eq!(python["active_content"], edited);
    assert_eq!(
        rust_node.work_load_document(&rust_active.join("root")).expect("active root")["content"].as_str(),
        Some(edited)
    );
    assert!(!rust_work.join(format!("completed/{DOC_ID}")).exists());
    assert!(!python["completed_exists"].as_bool().expect("completed path state"));
    assert!(rust_active.join("1").is_file(), "comment belongs to active document");
    assert_eq!(python["active_comment_ids"], serde_json::json!(["1"]));
}

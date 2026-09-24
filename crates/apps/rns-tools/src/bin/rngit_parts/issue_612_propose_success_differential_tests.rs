#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_propose_success_matches_pinned_python_response_and_owner_state() {
    use std::path::PathBuf;
    use std::process::Command;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
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

    let signer = rns_transport::identity::PrivateIdentity::new_from_name("issue-612-propose");
    let remote: [u8; 16] = signer
        .address_hash()
        .as_slice()
        .try_into()
        .expect("identity hash");
    let public_key = [
        signer.as_identity().public_key_bytes().as_slice(),
        signer.as_identity().verifying_key_bytes().as_slice(),
    ]
    .concat();
    let title = "Signed proposal";
    let content = "proposed work body";
    let signature = signer.sign(content.as_bytes()).to_bytes().to_vec();

    let rust_temp = tempfile::tempdir().expect("Rust fixture");
    let rust_group = rust_temp.path().join("group");
    fs::create_dir_all(&rust_group).expect("Rust group");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", rust_group.join("repo").to_string_lossy().as_ref()])
        .status()
        .expect("initialize Rust repository")
        .success());
    let mut rust_node = ReticulumGitNode::default();
    rust_node
        .load_repository_group("group", &rust_group)
        .expect("load Rust group");
    let permissions = &mut rust_node.groups.get_mut("group").expect("Rust group").permissions;
    permissions.read.add(PermissionTarget::All);
    permissions.write.add(PermissionTarget::All);
    permissions.interact.add(PermissionTarget::All);
    permissions.propose.add(PermissionTarget::All);

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    fs::create_dir_all(&python_group).expect("Python group");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_group.join("repo").to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    let script = r#"
import json, os, sys
from types import SimpleNamespace
from RNS.Identity import Identity
from RNS.Utilities.rngit.server import ReticulumGitNode
repo, public_hex, title, content, signature_hex = sys.argv[1:]
identity = Identity(create_keys=False)
assert identity.load_public_key(bytes.fromhex(public_hex))
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
response = node.handle_work("/mgmt/work", {
    0: "group/repo", "operation": "propose", "title": title,
    "content": content, "format": "markdown", "signature": bytes.fromhex(signature_hex),
}, 1, identity, 0)
work = repo + ".work"
with open(os.path.join(work, "proposed", "1", "root"), "rb") as stream:
    import msgpack
    document = msgpack.unpack(stream, raw=False)
with open(os.path.join(work, "1.allowed"), encoding="utf-8") as stream:
    sidecar = stream.read()
print(json.dumps({
    "status": response[0], "body": response[1:].hex(),
    "proposed": os.path.isdir(os.path.join(work, "proposed", "1")),
    "active": os.path.isdir(os.path.join(work, "active", "1")),
    "content": document["content"], "meta": {k: (v.hex() if isinstance(v, bytes) else v)
        for k, v in document["meta"].items() if k in ("title", "format", "author", "signature", "identity")},
    "sidecar": sidecar,
}))
"#;
    let python = Command::new(
        std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
    )
    .env("PYTHONPATH", &reference)
    .arg("-c")
    .arg(script)
    .arg(python_group.join("repo"))
    .arg(hex::encode(&public_key))
    .arg(title)
    .arg(content)
    .arg(hex::encode(&signature))
    .output()
    .expect("run pinned Python production handler");
    assert!(
        python.status.success(),
        "Python handler failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    let python: serde_json::Value =
        serde_json::from_slice(&python.stdout).expect("Python result JSON");

    let request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("propose")),
        (rmpv::Value::from("title"), rmpv::Value::from(title)),
        (rmpv::Value::from("content"), rmpv::Value::from(content)),
        (rmpv::Value::from("format"), rmpv::Value::from("markdown")),
        (rmpv::Value::from("signature"), rmpv::Value::Binary(signature.clone())),
    ];
    let rust_response = rust_node.handle_work_request_with_peer_identity(
        &request,
        remote,
        Some(*signer.as_identity()),
    );
    assert_eq!(rust_response[0], python["status"].as_u64().expect("Python status") as u8);
    assert_eq!(hex::encode(&rust_response[1..]), python["body"]);
    assert_eq!(rust_response[0], ReticulumGitNode::RES_OK);

    let rust_root = rust_group.join("repo.work/proposed/1/root");
    let rust_document = rust_node.work_load_document(&rust_root).expect("Rust proposal");
    let rust_meta = map_value(
        rust_document.as_map().expect("Rust document map"),
        &rmpv::Value::from("meta"),
    )
    .and_then(rmpv::Value::as_map)
    .expect("Rust metadata");
    assert_eq!(map_value(rust_document.as_map().expect("Rust document"), &rmpv::Value::from("content")).and_then(rmpv::Value::as_str), Some(content));
    assert_eq!(map_value(rust_meta, &rmpv::Value::from("title")).and_then(rmpv::Value::as_str), Some(title));
    assert_eq!(map_value(rust_meta, &rmpv::Value::from("format")).and_then(rmpv::Value::as_str), Some("markdown"));
    assert_eq!(map_value(rust_meta, &rmpv::Value::from("author")).and_then(rmpv::Value::as_slice), Some(remote.as_slice()));
    assert_eq!(map_value(rust_meta, &rmpv::Value::from("signature")).and_then(rmpv::Value::as_slice), Some(signature.as_slice()));
    assert_eq!(map_value(rust_meta, &rmpv::Value::from("identity")).and_then(rmpv::Value::as_slice), Some(public_key.as_slice()));
    assert_eq!(python["content"], content);
    assert_eq!(python["meta"]["title"], title);
    assert_eq!(python["meta"]["format"], "markdown");
    assert_eq!(python["meta"]["author"], hex::encode(remote));
    assert_eq!(python["meta"]["signature"], hex::encode(&signature));
    assert_eq!(python["meta"]["identity"], hex::encode(&public_key));
    assert!(!rust_group.join("repo.work/active/1").exists());
    assert!(python["proposed"].as_bool().expect("Python proposed state"));
    assert!(!python["active"].as_bool().expect("Python active state"));
    let rust_sidecar = fs::read_to_string(rust_group.join("repo.work/1.allowed")).expect("Rust owner permissions");
    let python_sidecar = python["sidecar"].as_str().expect("Python sidecar");
    assert_eq!(
        rust_node.permissions_from_allowed_input(Some(&rust_sidecar)),
        rust_node.permissions_from_allowed_input(Some(python_sidecar)),
        "Rust must resolve both implementations' accepted permission aliases identically"
    );

    let permission_parser = r#"
import json, sys
from RNS.Utilities.rngit.server import ReticulumGitNode
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.identity_aliases = {}
def normalize(content):
    permissions = node.permissions_from_allowed_input(content)
    return {name: sorted(value.hex() if isinstance(value, bytes) else value for value in values)
            for name, values in permissions.items()}
print(json.dumps({"python": normalize(sys.argv[1]), "rust": normalize(sys.argv[2])}))
"#;
    let parsed_sidecars = Command::new(
        std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
    )
    .env("PYTHONPATH", &reference)
    .arg("-c")
    .arg(permission_parser)
    .arg(python_sidecar)
    .arg(&rust_sidecar)
    .output()
    .expect("run pinned Python permission parser");
    assert!(
        parsed_sidecars.status.success(),
        "Python permission parser failed: {}",
        String::from_utf8_lossy(&parsed_sidecars.stderr)
    );
    let parsed_sidecars: serde_json::Value =
        serde_json::from_slice(&parsed_sidecars.stdout).expect("parsed permissions JSON");
    assert_eq!(parsed_sidecars["python"], parsed_sidecars["rust"]);
}

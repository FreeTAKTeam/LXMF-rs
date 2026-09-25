#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_create_and_propose_missing_signature_errors_match_pinned_python_precedence() {
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
    assert!(revision.status.success(), "could not resolve Python reference HEAD");
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        PYTHON_REFERENCE_REVISION,
        "differential must use the requested pinned Python reference"
    );

    let temp = tempfile::tempdir().expect("fixture root");
    let group = temp.path().join("group");
    let repository = group.join("repo");
    fs::create_dir_all(&group).expect("create group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", repository.to_string_lossy().as_ref()])
        .status()
        .expect("initialize bare repository")
        .success());

    let mut rust_node = ReticulumGitNode::default();
    rust_node
        .load_repository_group("group", &group)
        .expect("load Rust repository group");
    let permissions = &mut rust_node.groups.get_mut("group").expect("Rust group").permissions;
    for permission in [
        &mut permissions.read,
        &mut permissions.write,
        &mut permissions.interact,
        &mut permissions.propose,
    ] {
        permission.add(PermissionTarget::All);
    }

    let script = r#"
import json, os, sys
from RNS.Identity import Identity
from RNS.Utilities.rngit.server import ReticulumGitNode
repo, public_hex, operation = sys.argv[1:]
identity = Identity(create_keys=False)
assert identity.load_public_key(bytes.fromhex(public_hex))
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
response = node.handle_work("/mgmt/work", {
    0: "group/repo", "operation": operation, "title": "", "content": "",
}, 1, identity, 0)
work = repo + ".work"
print(json.dumps({"status": response[0], "body": response[1:].decode(),
    "work_exists": os.path.exists(work)}))
"#;

    let signer = rns_transport::identity::PrivateIdentity::new_from_name("issue-612-missing-signature");
    let remote: [u8; 16] = signer
        .address_hash()
        .as_slice()
        .try_into()
        .expect("identity hash");
    for operation in ["create", "propose"] {
        let python = Command::new(
            std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
        )
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&repository)
        .arg(hex::encode([
            signer.as_identity().public_key_bytes().as_slice(),
            signer.as_identity().verifying_key_bytes().as_slice(),
        ].concat()))
        .arg(operation)
        .output()
        .expect("run pinned Python production handler");
        assert!(
            python.status.success(),
            "Python {operation} handler failed: {}",
            String::from_utf8_lossy(&python.stderr)
        );
        let python: serde_json::Value =
            serde_json::from_slice(&python.stdout).expect("Python result JSON");

        let request = vec![
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from(operation)),
            (rmpv::Value::from("title"), rmpv::Value::from("")),
            (rmpv::Value::from("content"), rmpv::Value::from("")),
        ];
        let rust = rust_node.handle_work_request_with_peer_identity(
            &request,
            remote,
            Some(*signer.as_identity()),
        );

        assert_eq!(python["status"].as_u64(), Some(u64::from(rust[0])), "{operation} status");
        assert_eq!(python["body"].as_str(), Some("No signature provided"), "Python {operation} body");
        assert_eq!(String::from_utf8_lossy(&rust[1..]), "No signature provided", "Rust {operation} body");
        assert_eq!(python["work_exists"], false, "Python {operation} must not create work state");
        assert!(!group.join("repo.work").exists(), "Rust {operation} must not create work state");
    }
}

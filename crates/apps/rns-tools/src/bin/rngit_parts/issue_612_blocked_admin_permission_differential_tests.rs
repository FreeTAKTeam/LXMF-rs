#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn blocked_former_repository_admin_gets_pinned_python_not_found_response() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const BLOCKED_ADMIN: &str = "33333333333333333333333333333333";
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
    assert_eq!(String::from_utf8_lossy(&revision.stdout).trim(), PYTHON_REFERENCE_REVISION);

    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo");
    fs::create_dir_all(&group_path).expect("group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git init")
        .success());
    fs::write(group_path.with_extension("allowed"), "read:all\nadmin:all\n")
        .expect("group policy");
    fs::write(repository_path.with_extension("allowed"), "read:none\nadmin:all\n")
        .expect("repository policy");

    let remote: [u8; 16] = hex::decode(BLOCKED_ADMIN)
        .expect("identity hex")
        .try_into()
        .expect("16-byte identity");
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &group_path).expect("load Rust group");
    rust_node.blocked_identities.insert(remote);
    let request = crate::pack_request([
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("rperms")),
        (rmpv::Value::from("step"), rmpv::Value::from("get")),
    ])
    .expect("pack production permission request");
    let rust_response = rust_node.handle_request("/mgmt/perms", &request, remote);

    let script = r#"
import sys, threading
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
group_path, identity_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {}
node.config = {}
node.perms_lock = threading.Lock()
node.load_repository_group("group", group_path)
identity = bytes.fromhex(identity_hex)
node.blocked_identities[identity] = True
response = node.handle_perms(None, {0: "group/repo", "operation": "rperms", "step": "get"}, 0, SimpleNamespace(hash=identity), 0)
sys.stdout.buffer.write(response)
"#;
    let output = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&group_path)
        .arg(BLOCKED_ADMIN)
        .output()
        .expect("run pinned Python permission handler");
    assert!(output.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&output.stderr));
    let expected = [ReticulumGitNode::RES_NOT_FOUND]
        .into_iter()
        .chain(b"Not found".iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(output.stdout, expected);
    assert_eq!(rust_response, output.stdout, "blocked former-admin permission response differs from pinned Python");
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn repository_admin_can_view_document_denied_by_its_sidecar_like_pinned_python() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const ADMIN: [u8; 16] = [0x33; 16];
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

    let rust_temp = tempfile::tempdir().expect("Rust fixture");
    let rust_group = rust_temp.path().join("group");
    fs::create_dir_all(&rust_group).expect("Rust group");
    let rust_repo = rust_group.join("repo");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", rust_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Rust repository")
        .success());
    fs::write(rust_group.with_extension("allowed"), "read:all\n")
        .expect("write Rust group permissions");
    fs::write(rust_repo.with_extension("allowed"), format!("admin:{}\n", hex::encode(ADMIN)))
        .expect("write Rust repository administrator");
    let rust_document = rust_group.join("repo.work/active/7");
    fs::create_dir_all(&rust_document).expect("Rust work document");
    fs::write(
        rust_document.join("root"),
        rngit_work_fixture(&rmpv::Value::Map(vec![
            (rmpv::Value::from("content"), rmpv::Value::from("admin-visible body")),
            (rmpv::Value::from("meta"), rmpv::Value::Map(Vec::new())),
        ])),
    )
    .expect("write Rust work document");
    fs::write(rust_group.join("repo.work/7.allowed"), "read:none\n")
        .expect("deny document read for ordinary identities");
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &rust_group).expect("load Rust group");

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    fs::create_dir_all(&python_group).expect("Python group");
    let python_repo = python_group.join("repo");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    fs::write(python_group.with_extension("allowed"), "read:all\n")
        .expect("write Python group permissions");
    fs::write(python_repo.with_extension("allowed"), format!("admin:{}\n", hex::encode(ADMIN)))
        .expect("write Python repository administrator");
    let python_document = python_group.join("repo.work/active/7");
    fs::create_dir_all(&python_document).expect("Python work document");
    fs::write(
        python_document.join("root"),
        rngit_work_fixture(&rmpv::Value::Map(vec![
            (rmpv::Value::from("content"), rmpv::Value::from("admin-visible body")),
            (rmpv::Value::from("meta"), rmpv::Value::Map(Vec::new())),
        ])),
    )
    .expect("write Python work document");
    fs::write(python_group.join("repo.work/7.allowed"), "read:none\n")
        .expect("deny document read for ordinary identities");

    let script = r#"
import json, sys
from types import SimpleNamespace
from threading import Lock
from RNS.Utilities.rngit.server import ReticulumGitNode
group_path, admin_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {}
node.identity_aliases = {}
node.config = {}
node.perms_lock = Lock()
node.log_request = lambda *args: None
node.load_repository_group("group", group_path)
response = node.handle_work(
    "/mgmt/work",
    {0: "group/repo", "operation": "view", "doc_id": 7, "scope": "active"},
    1,
    SimpleNamespace(hash=bytes.fromhex(admin_hex)),
    0,
)
print(json.dumps({"status": response[0], "body": response[1:].hex()}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&python_group)
        .arg(hex::encode(ADMIN))
        .output()
        .expect("run pinned Python work handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let python: serde_json::Value = serde_json::from_slice(&python.stdout).expect("Python JSON result");

    let request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("view")),
        (rmpv::Value::from("doc_id"), rmpv::Value::from(7_u64)),
        (rmpv::Value::from("scope"), rmpv::Value::from("active")),
    ];
    let rust_response = rust_node.handle_work_request(&request, ADMIN);
    assert_eq!(python["status"], ReticulumGitNode::RES_OK);
    assert_eq!(rust_response[0], ReticulumGitNode::RES_OK);
    assert_eq!(rust_response[0], python["status"].as_u64().expect("Python status") as u8);
    assert_eq!(hex::encode(&rust_response[1..]), python["body"]);
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn view_document_zero_keeps_rust_document_read_denial_stricter_than_python_truthiness() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const REMOTE: [u8; 16] = [0x42; 16];
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
    assert_eq!(String::from_utf8_lossy(&revision.stdout).trim(), PYTHON_REFERENCE_REVISION);

    let rust_temp = tempfile::tempdir().expect("Rust fixture");
    let rust_group = rust_temp.path().join("group");
    fs::create_dir_all(&rust_group).expect("Rust group");
    let rust_repo = rust_group.join("repo");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", rust_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Rust repository")
        .success());
    fs::write(rust_group.with_extension("allowed"), "read:all\n").expect("Rust group read");
    fs::write(rust_group.join("repo.allowed"), "read:all\n").expect("Rust repository read");
    let rust_document = rust_group.join("repo.work/active/0");
    fs::create_dir_all(&rust_document).expect("Rust work document");
    fs::write(
        rust_document.join("root"),
        rngit_work_fixture(&rmpv::Value::Map(vec![
            (rmpv::Value::from("content"), rmpv::Value::from("item zero")),
            (rmpv::Value::from("meta"), rmpv::Value::Map(Vec::new())),
        ])),
    )
    .expect("write Rust work document");
    fs::write(rust_group.join("repo.work/0.allowed"), "read:none\n")
        .expect("deny Rust document read");
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &rust_group).expect("load Rust group");

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    fs::create_dir_all(&python_group).expect("Python group");
    let python_repo = python_group.join("repo");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    fs::write(python_group.with_extension("allowed"), "read:all\n").expect("Python group read");
    fs::write(python_group.join("repo.allowed"), "read:all\n").expect("Python repository read");
    let python_document = python_group.join("repo.work/active/0");
    fs::create_dir_all(&python_document).expect("Python work document");
    let seed = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .args([
            "-c",
            "import msgpack, sys; msgpack.pack({'content':'item zero','meta':{}}, open(sys.argv[1], 'wb'))",
        ])
        .arg(python_document.join("root"))
        .status()
        .expect("seed Python document");
    assert!(seed.success());
    fs::write(python_group.join("repo.work/0.allowed"), "read:none\n")
        .expect("deny Python document read");

    let script = r#"
import json, sys
from types import SimpleNamespace
from threading import Lock
from RNS.Utilities.rngit.server import ReticulumGitNode
group_path, remote_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {}
node.identity_aliases = {}
node.config = {}
node.perms_lock = Lock()
node.log_request = lambda *args: None
node.load_repository_group("group", group_path)
response = node.handle_work(
    "/mgmt/work", {0: "group/repo", "operation": "view", "doc_id": 0}, 1,
    SimpleNamespace(hash=bytes.fromhex(remote_hex)), 0,
)
print(json.dumps({"status": response[0], "body": response[1:].hex()}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&python_group)
        .arg(hex::encode(REMOTE))
        .output()
        .expect("run pinned Python production work handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let python: serde_json::Value = serde_json::from_slice(&python.stdout).expect("Python result JSON");

    let request = [
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("view")),
        (rmpv::Value::from("doc_id"), rmpv::Value::from(0_u64)),
    ];
    let rust_response = rust_node.handle_work_request(&request, REMOTE);

    // Python's truthiness check skips the doc-level read gate for ID 0. Keep
    // Rust fail-closed rather than reproducing that authorization bypass.
    assert_eq!(python["status"], ReticulumGitNode::RES_OK);
    let python_body = hex::decode(python["body"].as_str().expect("Python body hex"))
        .expect("decode Python response body");
    let python_document = rmpv::decode::read_value(&mut python_body.as_slice())
        .expect("decode Python document response");
    assert_eq!(
        python_document
            .as_map()
            .and_then(|map| map_value(map, &rmpv::Value::from("content")))
            .and_then(rmpv::Value::as_str),
        Some("item zero"),
        "Python should demonstrate that ID 0 bypassed the document read gate"
    );
    assert_eq!(rust_response[0], ReticulumGitNode::RES_NOT_FOUND);
    assert_ne!(rust_response[0], python["status"].as_u64().expect("Python status") as u8);
}

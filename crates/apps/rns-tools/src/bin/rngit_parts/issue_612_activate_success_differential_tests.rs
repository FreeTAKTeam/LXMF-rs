#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn activating_proposed_work_matches_pinned_python_response_and_directory_transition() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const DOCUMENT_ID: u64 = 9;
    const REMOTE: [u8; 16] = [0x39; 16];
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
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        PYTHON_REFERENCE_REVISION,
        "differential must use the pinned RNS 1.5.4 reference"
    );

    let rust_temp = tempfile::tempdir().expect("Rust fixture");
    let rust_group = rust_temp.path().join("group");
    fs::create_dir_all(&rust_group).expect("Rust group");
    assert!(Command::new("git")
        .args([
            "init",
            "--bare",
            "--quiet",
            rust_group.join("repo").to_string_lossy().as_ref(),
        ])
        .status()
        .expect("initialize Rust repository")
        .success());
    let mut rust_node = ReticulumGitNode::default();
    rust_node
        .load_repository_group("group", &rust_group)
        .expect("load Rust group");
    let permissions = &mut rust_node
        .groups
        .get_mut("group")
        .expect("Rust group")
        .permissions;
    permissions.read.add(PermissionTarget::All);
    permissions.write.add(PermissionTarget::All);
    permissions.interact.add(PermissionTarget::All);
    let rust_work = rust_group.join("repo.work");
    let rust_proposed = rust_work.join("proposed").join(DOCUMENT_ID.to_string());
    fs::create_dir_all(&rust_proposed).expect("Rust proposed work");
    fs::write(
        rust_proposed.join("root"),
        rngit_work_fixture(&rmpv::Value::Map(vec![
            (rmpv::Value::from("content"), rmpv::Value::from("proposed work")),
            (
                rmpv::Value::from("meta"),
                rmpv::Value::Map(vec![(
                    rmpv::Value::from("author"),
                    rmpv::Value::Binary(REMOTE.to_vec()),
                )]),
            ),
        ])),
    )
    .expect("write Rust proposed work");

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    fs::create_dir_all(&python_group).expect("Python group");
    let python_repo = python_group.join("repo");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    let python_proposed = python_group
        .join("repo.work/proposed")
        .join(DOCUMENT_ID.to_string());
    fs::create_dir_all(&python_proposed).expect("Python proposed work");
    let seed = Command::new(
        std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
    )
    .env("PYTHONPATH", &reference)
    .args([
        "-c",
        "import msgpack, sys; msgpack.pack({'content':'proposed work','meta':{'author':bytes.fromhex(sys.argv[2])}}, open(sys.argv[1], 'wb'))",
    ])
    .arg(python_proposed.join("root"))
    .arg(hex::encode(REMOTE))
    .status()
    .expect("seed Python work");
    assert!(seed.success(), "could not seed pinned Python work fixture");

    let script = r#"
import json, os, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo = sys.argv[1]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
work = repo + ".work"
proposed_before = os.path.isdir(os.path.join(work, "proposed", "9"))
active_before = os.path.isdir(os.path.join(work, "active", "9"))
response = node.handle_work(
    "/mgmt/work",
    {0: "group/repo", "operation": "activate", "doc_id": 9},
    1,
    SimpleNamespace(hash=bytes.fromhex(sys.argv[2])),
    0,
)
print(json.dumps({
    "status": response[0],
    "body": response[1:].hex(),
    "proposed_before": proposed_before,
    "active_before": active_before,
    "proposed": os.path.isdir(os.path.join(work, "proposed", "9")),
    "active": os.path.isdir(os.path.join(work, "active", "9")),
}))
"#;
    let python = Command::new(
        std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
    )
    .env("PYTHONPATH", &reference)
    .arg("-c")
    .arg(script)
    .arg(&python_repo)
    .arg(hex::encode(REMOTE))
    .output()
    .expect("run pinned Python production work handler");
    assert!(
        python.status.success(),
        "Python handler failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    let python: serde_json::Value =
        serde_json::from_slice(&python.stdout).expect("Python handler result JSON");

    let rust_active = rust_work.join("active").join(DOCUMENT_ID.to_string());
    assert!(rust_proposed.is_dir(), "Rust fixture must begin in proposed scope");
    assert!(!rust_active.exists(), "Rust fixture must begin without active scope");
    assert_eq!(python["proposed_before"], true);
    assert_eq!(python["active_before"], false);

    let request = [
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("activate")),
        (
            rmpv::Value::from("doc_id"),
            rmpv::Value::from(DOCUMENT_ID),
        ),
    ];
    let rust_response = rust_node.handle_work_request(&request, REMOTE);
    const EXPECTED_BODY_HEX: &str = "82a2696409a573636f7065a6616374697665";
    assert_eq!(python["status"], ReticulumGitNode::RES_OK);
    assert_eq!(python["body"], EXPECTED_BODY_HEX);
    assert_eq!(rust_response[0], ReticulumGitNode::RES_OK);
    assert_eq!(hex::encode(&rust_response[1..]), EXPECTED_BODY_HEX);
    assert!(!rust_proposed.exists());
    assert!(rust_active.is_dir());
    assert_eq!(python["proposed"], false);
    assert_eq!(python["active"], true);
}

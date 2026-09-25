#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_complete_float_id_matches_pinned_python_integer_coercion_and_directory_transition() {
    use std::path::PathBuf;
    use std::process::Command;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const REMOTE: [u8; 16] = [0x51; 16];
    const DOCUMENT_ID: u64 = 7;
    const INTEGER_DOCUMENT_ID: u64 = 8;
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
    for id in [DOCUMENT_ID, INTEGER_DOCUMENT_ID] {
        let rust_active = rust_group.join("repo.work/active").join(id.to_string());
        fs::create_dir_all(&rust_active).expect("Rust active work");
        fs::write(
            rust_active.join("root"),
            rngit_work_fixture(&rmpv::Value::Map(vec![
                (rmpv::Value::from("content"), rmpv::Value::from("body")),
                (
                    rmpv::Value::from("meta"),
                    rmpv::Value::Map(vec![(
                        rmpv::Value::from("author"),
                        rmpv::Value::Binary(REMOTE.to_vec()),
                    )]),
                ),
            ])),
        )
        .expect("write Rust active work");
    }

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    let python_repo = python_group.join("repo");
    fs::create_dir_all(&python_group).expect("Python group");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());
    for id in [DOCUMENT_ID, INTEGER_DOCUMENT_ID] {
        let python_active = python_group.join("repo.work/active").join(id.to_string());
        fs::create_dir_all(&python_active).expect("Python active work");
        let seed = Command::new(
            std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
        )
        .env("PYTHONPATH", &reference)
        .args([
            "-c",
            "import msgpack, sys; msgpack.pack({'content':'body','meta':{'author':bytes.fromhex(sys.argv[2])}}, open(sys.argv[1], 'wb'))",
        ])
        .arg(python_active.join("root"))
        .arg(hex::encode(REMOTE))
        .status()
        .expect("seed Python work");
        assert!(seed.success());
    }

    let python_script = r#"
import json, os, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo, remote_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: False
responses = [node.handle_work("/mgmt/work", {0: "group/repo", "operation": "complete", "doc_id": doc_id}, 1, SimpleNamespace(hash=bytes.fromhex(remote_hex)), 0) for doc_id in (7.9, 8)]
work = repo + ".work"
print(json.dumps([{"status": response[0], "body": response[1:].hex(), "active": os.path.isdir(os.path.join(work, "active", str(doc_id))), "completed": os.path.isdir(os.path.join(work, "completed", str(doc_id)))} for response, doc_id in zip(responses, (7, 8))]))
"#;
    let python = Command::new(
        std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()),
    )
    .env("PYTHONPATH", &reference)
    .arg("-c")
    .arg(python_script)
    .arg(&python_repo)
    .arg(hex::encode(REMOTE))
    .output()
    .expect("run pinned Python production handler");
    assert!(
        python.status.success(),
        "Python handler failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    let python: serde_json::Value =
        serde_json::from_slice(&python.stdout).expect("Python result JSON");

    for (id, python_result) in [(rmpv::Value::F64(7.9), &python[0]),
        (rmpv::Value::from(INTEGER_DOCUMENT_ID), &python[1])]
    {
        let request = vec![
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from("complete")),
            (rmpv::Value::from("doc_id"), id),
        ];
        let response = rust_node.handle_work_request(&request, REMOTE);
        assert_eq!(
            response[0],
            python_result["status"].as_u64().expect("Python status") as u8
        );
        assert_eq!(hex::encode(&response[1..]), python_result["body"]);
    }
    for (id, python_result) in [(DOCUMENT_ID, &python[0]), (INTEGER_DOCUMENT_ID, &python[1])] {
        let rust_active = rust_group.join("repo.work/active").join(id.to_string()).is_dir();
        let rust_completed =
            rust_group.join("repo.work/completed").join(id.to_string()).is_dir();
        assert_eq!(rust_active, python_result["active"]);
        assert_eq!(rust_completed, python_result["completed"]);
    }
    assert_eq!(python[0]["status"], ReticulumGitNode::RES_OK);
    assert_eq!(python[1]["status"], ReticulumGitNode::RES_OK);
}

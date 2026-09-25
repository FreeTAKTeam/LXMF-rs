#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_complete_denial_and_request_errors_match_pinned_python_without_mutation() {
    use std::path::PathBuf;
    use std::process::Command;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const REMOTE: [u8; 16] = [0x51; 16];
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
    assert_eq!(String::from_utf8_lossy(&revision.stdout).trim(), PYTHON_REFERENCE_REVISION);

    let cases: [(&str, bool, Option<&str>); 3] = [
        ("denied", false, Some("7")),
        ("missing", true, None),
        ("malformed", true, Some("not-an-id")),
    ];
    for (case, allowed, document_id) in cases {
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
        permissions.interact.add(PermissionTarget::All);
        if allowed {
            permissions.write.add(PermissionTarget::All);
        }
        let rust_document = rust_group.join("repo.work/active/7");
        if case == "denied" {
            fs::create_dir_all(&rust_document).expect("Rust active work");
            fs::write(
                rust_document.join("root"),
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
        let python_document = python_group.join("repo.work/active/7");
        if case == "denied" {
            fs::create_dir_all(&python_document).expect("Python active work");
            let seeded = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
                .env("PYTHONPATH", &reference)
                .args(["-c", "import msgpack, sys; msgpack.pack({'content':'body','meta':{'author':bytes.fromhex(sys.argv[2])}}, open(sys.argv[1], 'wb'))"])
                .arg(python_document.join("root"))
                .arg(hex::encode(REMOTE))
                .status()
                .expect("seed Python work");
            assert!(seeded.success());
        }

        let python_script = r#"
import json, os, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo, remote_hex, allowed, doc_id = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda _remote, _group, _repo, perm: perm in (node.PERM_READ, node.PERM_INTERACT) or (allowed == "1" and perm == node.PERM_WRITE)
node.resolve_doc_permission = lambda *_args: False
request = {0: "group/repo", "operation": "complete"}
if doc_id != "<missing>": request["doc_id"] = doc_id
response = node.handle_work("/mgmt/work", request, 1, SimpleNamespace(hash=bytes.fromhex(remote_hex)), 0)
active = os.path.join(repo + ".work", "active", "7")
completed = os.path.join(repo + ".work", "completed", "7")
print(json.dumps({"status": response[0], "body": response[1:].hex(), "active": os.path.isdir(active), "completed": os.path.isdir(completed)}))
"#;
        let python =
            Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
                .env("PYTHONPATH", &reference)
                .arg("-c")
                .arg(python_script)
                .arg(&python_repo)
                .arg(hex::encode(REMOTE))
                .arg(if allowed { "1" } else { "0" })
                .arg(document_id.unwrap_or("<missing>"))
                .output()
                .expect("run pinned Python production handler");
        assert!(
            python.status.success(),
            "Python handler failed: {}",
            String::from_utf8_lossy(&python.stderr)
        );
        let python: serde_json::Value =
            serde_json::from_slice(&python.stdout).expect("Python result JSON");

        let mut request = vec![
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from("complete")),
        ];
        if let Some(id) = document_id {
            request.push((rmpv::Value::from("doc_id"), rmpv::Value::from(id)));
        }
        let response = rust_node.handle_work_request(&request, REMOTE);
        assert_eq!(
            response[0],
            python["status"].as_u64().expect("Python status") as u8,
            "case={case}"
        );
        assert_eq!(hex::encode(&response[1..]), python["body"], "case={case}");
        assert_eq!(rust_document.is_dir(), python["active"], "case={case}");
        assert_eq!(
            rust_group.join("repo.work/completed/7").is_dir(),
            python["completed"],
            "case={case}"
        );
    }
}

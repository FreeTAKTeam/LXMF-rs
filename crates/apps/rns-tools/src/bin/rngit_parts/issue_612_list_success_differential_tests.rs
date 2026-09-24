#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_list_success_matches_pinned_python_scopes_order_metadata_and_comment_counts() {
    use std::path::PathBuf;

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

    let rust_temp = tempfile::tempdir().expect("Rust fixture");
    let rust_group = rust_temp.path().join("group");
    fs::create_dir_all(&rust_group).expect("Rust group");
    let rust_repo = rust_group.join("repo");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", rust_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Rust repository")
        .success());
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &rust_group).expect("load Rust group");
    rust_node
        .groups
        .get_mut("group")
        .expect("Rust group")
        .permissions
        .read
        .add(PermissionTarget::All);

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    fs::create_dir_all(&python_group).expect("Python group");
    let python_repo = python_group.join("repo");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());

    for (scope, id, title, created) in [
        ("active", 2_u64, "older active", 10_u64),
        ("active", 1_u64, "newer active", 20_u64),
        ("completed", 3_u64, "completed", 15_u64),
        ("proposed", 4_u64, "proposed", 5_u64),
    ] {
        let document = rmpv::Value::Map(vec![
            (rmpv::Value::from("content"), rmpv::Value::from(format!("body-{id}"))),
            (
                rmpv::Value::from("meta"),
                rmpv::Value::Map(vec![
                    (rmpv::Value::from("title"), rmpv::Value::from(title)),
                    (rmpv::Value::from("created"), rmpv::Value::from(created)),
                    (rmpv::Value::from("edited"), rmpv::Value::from(created + 1)),
                    (rmpv::Value::from("author"), rmpv::Value::Binary(vec![id as u8; 16])),
                    (rmpv::Value::from("format"), rmpv::Value::from("markdown")),
                ]),
            ),
        ]);
        let bytes = super::pack_value(&document).expect("encode deterministic document");
        let rust_dir = rust_repo.join(format!("work/{scope}/{id}"));
        let python_dir = python_repo.join(format!("work/{scope}/{id}"));
        fs::create_dir_all(&rust_dir).expect("Rust document directory");
        fs::create_dir_all(&python_dir).expect("Python document directory");
        fs::write(rust_dir.join("root"), &bytes).expect("write Rust document");
        fs::write(python_dir.join("root"), bytes).expect("write Python document");
        if id == 1 {
            fs::write(rust_dir.join("1"), b"comment one").expect("Rust comment one");
            fs::write(rust_dir.join("2"), b"comment two").expect("Rust comment two");
            fs::write(python_dir.join("1"), b"comment one").expect("Python comment one");
            fs::write(python_dir.join("2"), b"comment two").expect("Python comment two");
        }
    }

    let script = r#"
import json, sys
from RNS.Utilities.rngit.server import ReticulumGitNode
repo = sys.argv[1]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
response = node.handle_work("/mgmt/work", {0: "group/repo", "operation": "list", "scope": "all"}, 1, object(), 0)
print(json.dumps({"status": response[0], "body": response[1:].hex()}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&python_repo)
        .output()
        .expect("run pinned Python production handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let python: serde_json::Value = serde_json::from_slice(&python.stdout).expect("Python JSON result");

    let request = [
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("list")),
        (rmpv::Value::from("scope"), rmpv::Value::from("all")),
    ];
    let rust = rust_node.handle_work_request(&request, [0x42; 16]);
    assert_eq!(python["status"], rust[0]);
    assert_eq!(python["body"], hex::encode(&rust[1..]));
    assert_eq!(rust[0], ReticulumGitNode::RES_OK);
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_list_completed_scope_matches_pinned_python_and_is_read_only() {
    use std::path::PathBuf;

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
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &rust_group).expect("load Rust group");
    rust_node.groups.get_mut("group").expect("Rust group")
        .permissions.read.add(PermissionTarget::All);

    let python_temp = tempfile::tempdir().expect("Python fixture");
    let python_group = python_temp.path().join("group");
    fs::create_dir_all(&python_group).expect("Python group");
    let python_repo = python_group.join("repo");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("initialize Python repository")
        .success());

    let mut snapshots = Vec::new();
    for (scope, id, title) in [("active", 1_u64, "must be excluded"), ("completed", 2, "completed only")] {
        let document = rmpv::Value::Map(vec![
            (rmpv::Value::from("content"), rmpv::Value::from(format!("body-{id}"))),
            (rmpv::Value::from("meta"), rmpv::Value::Map(vec![
                (rmpv::Value::from("title"), rmpv::Value::from(title)),
                (rmpv::Value::from("created"), rmpv::Value::from(id * 10)),
                (rmpv::Value::from("author"), rmpv::Value::Binary(vec![id as u8; 16])),
            ])),
        ]);
        let bytes = super::pack_value(&document).expect("encode document");
        for repo in [&rust_repo, &python_repo] {
            let dir = repo.with_file_name("repo.work").join(format!("{scope}/{id}"));
            fs::create_dir_all(&dir).expect("document directory");
            fs::write(dir.join("root"), &bytes).expect("document root");
            if scope == "completed" {
                fs::write(dir.join("1"), b"existing comment").expect("comment fixture");
            }
            let before = fs::read_dir(&dir)
                .expect("read fixture")
                .map(|entry| {
                    let entry = entry.expect("fixture entry");
                    (entry.file_name(), fs::read(entry.path()).expect("snapshot fixture file"))
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            snapshots.push((dir, before));
        }
    }

    let script = r#"
import json, sys
from RNS.Utilities.rngit.server import ReticulumGitNode
repo = sys.argv[1]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda *_args: True
node.resolve_doc_permission = lambda *_args: True
response = node.handle_work("/mgmt/work", {0: "group/repo", "operation": "list", "scope": "completed"}, 1, object(), 0)
print(json.dumps({"status": response[0], "body": response[1:].hex()}))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference).arg("-c").arg(script).arg(&python_repo)
        .output().expect("run pinned Python production handler");
    assert!(python.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let python: serde_json::Value = serde_json::from_slice(&python.stdout).expect("Python JSON result");
    let request = [
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("list")),
        (rmpv::Value::from("scope"), rmpv::Value::from("completed")),
    ];
    let rust = rust_node.handle_work_request(&request, [0x42; 16]);
    assert_eq!(python["status"], rust[0]);
    assert_eq!(python["body"], hex::encode(&rust[1..]));
    assert_eq!(rust[0], ReticulumGitNode::RES_OK);
    let payload = rmpv::decode::read_value(&mut std::io::Cursor::new(&rust[1..]))
        .expect("decode filtered list");
    let values = payload.as_map().expect("list scope map");
    assert!(map_value(values, &rmpv::Value::from("active")).and_then(rmpv::Value::as_array).expect("active scope").is_empty());
    assert_eq!(map_value(values, &rmpv::Value::from("completed")).and_then(rmpv::Value::as_array).expect("completed scope").len(), 1);
    assert!(map_value(values, &rmpv::Value::from("proposed")).and_then(rmpv::Value::as_array).expect("proposed scope").is_empty());
    for (dir, before) in snapshots {
        let after = fs::read_dir(&dir)
            .expect("post-request directory")
            .map(|entry| {
                let entry = entry.expect("post-request entry");
                (entry.file_name(), fs::read(entry.path()).expect("post-request file"))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(after, before, "list mutated fixture at {}", dir.display());
    }
}

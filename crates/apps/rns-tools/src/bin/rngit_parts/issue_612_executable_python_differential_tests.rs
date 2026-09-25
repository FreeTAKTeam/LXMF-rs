#[cfg(unix)]
#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn executable_allowed_resolver_matches_pinned_python_permission_decisions() {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const ALLOWED_IDENTITY: &str = "11111111111111111111111111111111";
    const DENIED_IDENTITY: &str = "22222222222222222222222222222222";

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
        String::from_utf8(revision.stdout).expect("revision is UTF-8").trim(),
        PYTHON_REFERENCE_REVISION,
        "differential must use the requested pinned Python reference"
    );

    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo");
    fs::create_dir_all(&group_path).expect("group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git init")
        .success());

    let resolver = group_path.with_extension("allowed");
    let resolver_output = format!("read:{ALLOWED_IDENTITY}\\n");
    fs::write(&resolver, format!("#!/bin/sh\nprintf '{resolver_output}'\n"))
        .expect("write executable resolver");
    fs::set_permissions(&resolver, fs::Permissions::from_mode(0o700)).expect("make resolver executable");

    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &group_path).expect("Rust production group loader");
    let rust_decisions = [ALLOWED_IDENTITY, DENIED_IDENTITY].map(|identity| {
        let bytes = hex::decode(identity).expect("identity hex");
        let hash: [u8; 16] = bytes.try_into().expect("16-byte identity");
        rust_node.resolve_permission(&hash, "group", "repo", ReticulumGitNode::PERM_READ)
    });
    let rust_stdout = crate::run_permission_resolver(&resolver)
        .expect("Rust bounded resolver execution");

    let python = std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into());
    let script = r#"
import json
import subprocess
import sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode

reference_group_path, allowed_identity, denied_identity = sys.argv[1:]
resolver_path = reference_group_path + ".allowed"
resolver_outputs = []
original_run = subprocess.run
def capture_resolver(command, *args, **kwargs):
    result = original_run(command, *args, **kwargs)
    if command == [resolver_path]:
        resolver_outputs.append(result.stdout)
    return result
subprocess.run = capture_resolver
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {}
node.config = {}
node.load_repository_group("group", reference_group_path)
group = node.groups["group"]
assert group["dynamic_perms"] is True
assert len(resolver_outputs) == 1
stdout = resolver_outputs[0].decode("utf-8")
decisions = [
    node.resolve_permission(SimpleNamespace(hash=bytes.fromhex(identity)), "group", "repo", node.PERM_READ)
    for identity in (allowed_identity, denied_identity)
]
print(json.dumps({"stdout": stdout, "decisions": decisions}))
"#;
    let output = Command::new(python)
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&group_path)
        .arg(ALLOWED_IDENTITY)
        .arg(DENIED_IDENTITY)
        .output()
        .expect("run pinned Python permission loader");
    assert!(
        output.status.success(),
        "Python production permission path failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python_result: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Python differential JSON");
    let python_stdout = python_result["stdout"].as_str().expect("Python resolver stdout");
    assert_eq!(rust_stdout, python_stdout, "resolver stdout differs");
    let python_decisions = python_result["decisions"]
        .as_array()
        .expect("Python decisions")
        .iter()
        .map(|decision| decision.as_bool().expect("boolean decision"))
        .collect::<Vec<_>>();

    assert_eq!(rust_decisions, [true, false], "Rust production permission decisions");
    assert_eq!(python_decisions, rust_decisions, "pinned Python and Rust decisions differ");
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn permission_handler_revocation_and_blocked_identity_match_pinned_python() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const AUTHOR: &str = "11111111111111111111111111111111";
    const ADMIN: &str = "33333333333333333333333333333333";
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
        .expect("group administrator policy");
    fs::write(repository_path.with_extension("allowed"), "read:all\nadmin:all\n")
        .expect("repository read policy");

    let python_root = tempfile::tempdir().expect("Python fixture directory");
    let python_group = python_root.path().join("group");
    let python_repository = python_group.join("repo");
    fs::create_dir_all(&python_group).expect("Python group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", python_repository.to_string_lossy().as_ref()])
        .status()
        .expect("Python git init")
        .success());
    fs::write(python_group.with_extension("allowed"), "read:all\nadmin:all\n")
        .expect("Python group administrator policy");
    fs::write(python_repository.with_extension("allowed"), "read:all\nadmin:all\n")
        .expect("Python repository read policy");

    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &group_path).expect("Rust production group loader");
    let author = hex::decode(AUTHOR).expect("author hex").try_into().expect("16-byte author");
    let admin = hex::decode(ADMIN).expect("admin hex").try_into().expect("16-byte admin");
    let rust_before = rust_node.resolve_permission(&author, "group", "repo", ReticulumGitNode::PERM_READ);
    let request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("rperms")),
        (rmpv::Value::from("step"), rmpv::Value::from("set")),
        (rmpv::Value::from("content"), rmpv::Value::from("read:none\n")),
    ];
    let rust_status = rust_node.handle_permission_request(&request, admin)[0];
    let rust_after = rust_node.resolve_permission(&author, "group", "repo", ReticulumGitNode::PERM_READ);
    rust_node.blocked_identities.insert(author);
    let rust_blocked = rust_node.resolve_permission(&author, "group", "repo", ReticulumGitNode::PERM_READ);
    assert!(!rust_node.remote_identified(author), "blocked identity must not be identified");

    let script = r#"
import json, sys, threading
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
group_path, author_hex, admin_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {}
node.config = {}
node.perms_lock = threading.Lock()
node.load_repository_group("group", group_path)
author = SimpleNamespace(hash=bytes.fromhex(author_hex))
admin = SimpleNamespace(hash=bytes.fromhex(admin_hex))
before = node.resolve_permission(author, "group", "repo", node.PERM_READ)
status = node.handle_perms(None, {0: "group/repo", "operation": "rperms", "step": "set", "content": "read:none\n"}, 0, admin, 0)[0]
after = node.resolve_permission(author, "group", "repo", node.PERM_READ)
node.blocked_identities[author.hash] = True
blocked = node.resolve_permission(author, "group", "repo", node.PERM_READ)
print(json.dumps({"before": before, "status": status, "after": after, "blocked": blocked}))
"#;
    let output = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&python_group)
        .arg(AUTHOR)
        .arg(ADMIN)
        .output()
        .expect("run pinned Python production permission handler");
    assert!(output.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&output.stderr));
    let python: serde_json::Value = serde_json::from_slice(&output.stdout).expect("Python JSON result");
    assert_eq!(rust_before, python["before"].as_bool().expect("before decision"));
    assert_eq!(rust_status, python["status"].as_u64().expect("handler status") as u8);
    assert_eq!(rust_after, python["after"].as_bool().expect("after decision"));
    assert_eq!(rust_blocked, python["blocked"].as_bool().expect("blocked decision"));
    assert!(rust_before && !rust_after && !rust_blocked, "revocation and block must deny immediately");
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn configured_group_access_merges_with_sidecar_like_pinned_python() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const CONFIGURED: &str = "11111111111111111111111111111111";
    const SIDECAR_ADMIN: &str = "33333333333333333333333333333333";
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
    fs::write(
        group_path.with_extension("allowed"),
        format!("admin:{SIDECAR_ADMIN}\n"),
    )
    .expect("group sidecar policy");

    let mut rust_node = ReticulumGitNode::default();
    rust_node
        .set_configured_group_permissions("group", &format!("read:{CONFIGURED}\n"))
        .expect("configured Rust access policy");
    rust_node.load_repository_group("group", &group_path).expect("load Rust group");
    let rust_decisions = [CONFIGURED, SIDECAR_ADMIN].map(|identity| {
        let identity: [u8; 16] = hex::decode(identity)
            .expect("identity hex")
            .try_into()
            .expect("identity length");
        (
            rust_node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ),
            rust_node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_ADMIN),
        )
    });

    let script = r#"
import json, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
group_path, configured, sidecar_admin = sys.argv[1:]
class AccessSection:
    def __iter__(self): return iter(["group"])
    def as_list(self, _name): return ["read:" + configured]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {}
node.identity_aliases = {}
node.config = {"access": AccessSection()}
node.load_repository_group("group", group_path)
decisions = []
for identity in (configured, sidecar_admin):
    remote = SimpleNamespace(hash=bytes.fromhex(identity))
    decisions.append([
        node.resolve_group_permission(remote, "group", node.PERM_READ),
        node.resolve_group_permission(remote, "group", node.PERM_ADMIN),
    ])
print(json.dumps(decisions))
"#;
    let output = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&group_path)
        .arg(CONFIGURED)
        .arg(SIDECAR_ADMIN)
        .output()
        .expect("run pinned Python group loader");
    assert!(output.status.success(), "Python loader failed: {}", String::from_utf8_lossy(&output.stderr));
    let python: serde_json::Value = serde_json::from_slice(&output.stdout).expect("Python JSON decisions");
    assert_eq!(rust_decisions, [(true, false), (true, true)]);
    assert_eq!(
        python,
        serde_json::json!([[true, false], [true, true]]),
        "pinned Python configured access and sidecar policies should merge additively"
    );
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn work_delete_ignores_requested_scope_like_pinned_python() {
    use std::path::PathBuf;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const AUTHOR: [u8; 16] = [0x11; 16];
    const OTHER: [u8; 16] = [0x22; 16];
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

    let rust_temp = tempfile::tempdir().expect("Rust fixture directory");
    let group_path = rust_temp.path().join("group");
    fs::create_dir_all(&group_path).expect("Rust group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", group_path.join("repo").to_string_lossy().as_ref()])
        .status()
        .expect("Rust git init")
        .success());
    let mut rust_node = ReticulumGitNode::default();
    rust_node.load_repository_group("group", &group_path).expect("load Rust group");
    let permissions = &mut rust_node.groups.get_mut("group").expect("Rust group").permissions;
    for permission in [&mut permissions.read, &mut permissions.write, &mut permissions.interact] {
        permission.add(PermissionTarget::All);
    }

    let document = |author: [u8; 16]| {
        rmpv::Value::Map(vec![
            (rmpv::Value::from("content"), rmpv::Value::from("body")),
            (
                rmpv::Value::from("meta"),
                rmpv::Value::Map(vec![(rmpv::Value::from("author"), rmpv::Value::Binary(author.to_vec()))]),
            ),
        ])
    };
    for (id, author) in [(7_u64, AUTHOR), (8_u64, AUTHOR)] {
        let directory = group_path.join(format!("repo.work/active/{id}"));
        fs::create_dir_all(&directory).expect("Rust active document");
        fs::write(directory.join("root"), rngit_work_fixture(&document(author)))
            .expect("write Rust work document");
        fs::write(group_path.join(format!("repo.work/{id}.allowed")), "write:all\ninteract:all\n")
            .expect("write Rust document permissions");
    }

    let python_temp = tempfile::tempdir().expect("Python fixture directory");
    let python_group = python_temp.path().join("group");
    let python_repo = python_group.join("repo");
    fs::create_dir_all(&python_group).expect("Python group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", python_repo.to_string_lossy().as_ref()])
        .status()
        .expect("Python git init")
        .success());
    for id in [7_u64, 8_u64] {
        let directory = python_group.join(format!("repo.work/active/{id}"));
        fs::create_dir_all(&directory).expect("Python active document");
        fs::write(python_group.join(format!("repo.work/{id}.allowed")), "write:all\ninteract:all\n")
            .expect("write Python document permissions");
    }

    let script = r#"
import json, msgpack, os, sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
repo_path, author_hex, other_hex = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {"group": {"repositories": {"repo": {"path": repo_path}}}}
node.blocked_identities = {}
node.log_request = lambda *args: None
node.parse_request_repository_path = lambda _path: ("group", "repo")
node.resolve_permission = lambda _remote, _group, _repo, permission: permission != node.PERM_ADMIN
node.resolve_doc_permission = lambda _remote, _group, _repo, _doc_id, permission: permission != node.PERM_ADMIN
for doc_id in (7, 8):
    root = os.path.join(repo_path + ".work", "active", str(doc_id), "root")
    with open(root, "wb") as stream:
        msgpack.pack({"content": "body", "meta": {"author": bytes.fromhex(author_hex)}}, stream)
responses = []
for doc_id, identity in ((7, author_hex), (8, other_hex), (9, author_hex)):
    request = {0: "group/repo", "operation": "delete", "doc_id": doc_id, "scope": "completed"}
    response = node.handle_work("/mgmt/work", request, 1, SimpleNamespace(hash=bytes.fromhex(identity)), 0)
    responses.append({
        "status": response[0],
        "body": response[1:].decode("utf-8", "replace"),
        "active_exists": os.path.exists(os.path.join(repo_path + ".work", "active", str(doc_id))),
    })
print(json.dumps(responses))
"#;
    let python = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg(script)
        .arg(&python_repo)
        .arg(hex::encode(AUTHOR))
        .arg(hex::encode(OTHER))
        .output()
        .expect("run pinned Python production work handler");
    assert!(python.status.success(), "Python work handler failed: {}", String::from_utf8_lossy(&python.stderr));
    let json_line = python
        .stdout
        .split(|byte| *byte == b'\n')
        .rfind(|line| !line.is_empty())
        .expect("Python JSON output line");
    let python: serde_json::Value = serde_json::from_slice(json_line)
        .unwrap_or_else(|error| panic!("Python result JSON: {error}; stdout={:?}", String::from_utf8_lossy(&python.stdout)));

    let make_request = |id| vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("delete")),
        (rmpv::Value::from("doc_id"), rmpv::Value::from(id)),
        (rmpv::Value::from("scope"), rmpv::Value::from("completed")),
    ];
    let rust_success = rust_node.handle_work_request(&make_request(7), AUTHOR);
    let rust_denied = rust_node.handle_work_request(&make_request(8), OTHER);
    let rust_missing = rust_node.handle_work_request(&make_request(9), AUTHOR);
    assert_eq!(rust_success, [ReticulumGitNode::RES_OK]);
    assert_eq!(rust_denied[0], ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(&rust_denied[1..], b"No access, not author");
    assert_eq!(rust_missing[0], ReticulumGitNode::RES_REMOTE_FAIL);
    assert_eq!(&rust_missing[1..], b"Remote error");
    assert_eq!(python[0]["status"], ReticulumGitNode::RES_OK);
    assert_eq!(python[0]["active_exists"], false);
    assert_eq!(python[1]["status"], ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(python[1]["body"], "No access, not author");
    assert_eq!(python[1]["active_exists"], true);
    assert_eq!(python[2]["status"], ReticulumGitNode::RES_REMOTE_FAIL);
    assert_eq!(python[2]["body"], "Remote error");
}

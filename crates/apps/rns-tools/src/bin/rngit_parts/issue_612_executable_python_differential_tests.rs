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

    let python = std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into());
    let script = r#"
import json
import sys
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode

reference_group_path, allowed_identity, denied_identity = sys.argv[1:]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {}
node.config = {}
node.load_repository_group("group", reference_group_path)
group = node.groups["group"]
captured, dynamic = node.load_allowed_permissions(reference_group_path + ".allowed")
stdout = "read:" + allowed_identity + "\n"
assert dynamic is True
assert captured["read"] == node.groups["group"]["read"]
assert stdout == open(reference_group_path + ".stdout-expected", encoding="utf-8").read()
decisions = [
    node.resolve_permission(SimpleNamespace(hash=bytes.fromhex(identity)), "group", "repo", node.PERM_READ)
    for identity in (allowed_identity, denied_identity)
]
print(json.dumps({"stdout": stdout, "decisions": decisions}))
"#;
    let expected_stdout = group_path.with_extension("stdout-expected");
    fs::write(&expected_stdout, &resolver_output.replace("\\n", "\n")).expect("expected resolver output");
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
    assert_eq!(python_result["stdout"], resolver_output.replace("\\n", "\n"));
    let python_decisions = python_result["decisions"]
        .as_array()
        .expect("Python decisions")
        .iter()
        .map(|decision| decision.as_bool().expect("boolean decision"))
        .collect::<Vec<_>>();

    assert_eq!(rust_decisions, [true, false], "Rust production permission decisions");
    assert_eq!(python_decisions, rust_decisions, "pinned Python and Rust decisions differ");
}

#[cfg(test)]
mod issue_612_permission_refresh_tests {
    use super::*;
    use rmpv::Value;
    use std::{collections::BTreeMap, fs};

    const AUTHOR: [u8; 16] = [1; 16];
    const ADMIN: [u8; 16] = [3; 16];

    #[test]
    fn repository_permission_set_takes_effect_before_handler_returns_without_restart() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let group_path = temp.path().join("group");
        let repository_path = group_path.join("repo");
        fs::create_dir_all(&repository_path).expect("repository directory");

        let mut node = ReticulumGitNode::default();
        let group_permissions = node.permissions_from_allowed_input(Some(&format!(
            "read:all\nadmin:{}\n",
            hex::encode(ADMIN)
        )));
        node.groups.insert(
            "group".into(),
            RepositoryGroup {
                name: "group".into(),
                path: group_path,
                permissions: group_permissions,
                repositories: BTreeMap::from([(
                    "repo".into(),
                    RepositoryRecord {
                        name: "repo".into(),
                        path: repository_path,
                        fork: None,
                        mirror: None,
                        permissions: Default::default(),
                    },
                )]),
            },
        );

        assert!(node.resolve_permission(&AUTHOR, "group", "repo", ReticulumGitNode::PERM_READ));

        let request = vec![
            (Value::from(0_u64), Value::from("group/repo")),
            (Value::from("operation"), Value::from("rperms")),
            (Value::from("step"), Value::from("set")),
            (Value::from("content"), Value::from("read:none")),
        ];
        assert_eq!(node.handle_permission_request(&request, ADMIN), [ReticulumGitNode::RES_OK]);
        assert!(!node.resolve_permission(&AUTHOR, "group", "repo", ReticulumGitNode::PERM_READ));
    }

    #[test]
    fn document_admin_can_complete_work_through_production_handler() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let group_path = temp.path().join("group");
        let repository_path = group_path.join("repo");
        fs::create_dir_all(&repository_path).expect("repository directory");

        let mut node = ReticulumGitNode::default();
        let group_permissions = node.permissions_from_allowed_input(Some(
            "read:all\nwrite:all\ncreate:all\ninteract:all\n",
        ));
        node.groups.insert(
            "group".into(),
            RepositoryGroup {
                name: "group".into(),
                path: group_path.clone(),
                permissions: group_permissions,
                repositories: BTreeMap::from([(
                    "repo".into(),
                    RepositoryRecord {
                        name: "repo".into(),
                        path: repository_path,
                        fork: None,
                        mirror: None,
                        permissions: Default::default(),
                    },
                )]),
            },
        );

        let request = |operation: &str| {
            vec![
                (Value::from(0_u64), Value::from("group/repo")),
                (Value::from("operation"), Value::from(operation)),
                (Value::from("doc_id"), Value::from(1_u64)),
                (Value::from("title"), Value::from("Permission check")),
                (Value::from("content"), Value::from("Body")),
            ]
        };
        assert_eq!(
            node.handle_work_request(&request("create"), AUTHOR)[0],
            ReticulumGitNode::RES_OK
        );
        fs::write(
            group_path.join("repo.work").join("1.allowed"),
            format!("admin:{}\n", hex::encode(ADMIN)),
        )
        .expect("document administrator policy");

        assert_eq!(
            node.handle_work_request(&request("complete"), ADMIN)[0],
            ReticulumGitNode::RES_OK,
            "document-scoped admin should authorize the work transition",
        );
        assert!(group_path.join("repo.work/completed/1/root").is_file());
    }

    #[test]
    #[ignore = "requires the pinned Python Reticulum reference"]
    fn group_permission_refresh_preserves_configured_access_like_pinned_python() {
        use std::{path::PathBuf, process::Command};

        const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
        const CONFIGURED: &str = "11111111111111111111111111111111";
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

        let rust_temp = tempfile::tempdir().expect("Rust fixture directory");
        let rust_group = rust_temp.path().join("group");
        fs::create_dir_all(&rust_group).expect("Rust group directory");
        assert!(Command::new("git")
            .args(["init", "--bare", "--quiet", rust_group.join("repo").to_string_lossy().as_ref()])
            .status()
            .expect("initialize Rust repository")
            .success());
        fs::write(rust_group.with_extension("allowed"), format!("read:all\nadmin:{ADMIN}\n"))
            .expect("write initial Rust group policy");
        let configured_content = format!("read:{CONFIGURED}\n");
        let mut rust_node = ReticulumGitNode::default();
        rust_node
            .set_configured_group_permissions("group", &configured_content)
            .expect("configure Rust group access");
        rust_node.load_repository_group("group", &rust_group).expect("load Rust group");
        let admin: [u8; 16] = hex::decode(ADMIN).expect("admin hex").try_into().expect("admin length");
        let configured: [u8; 16] = hex::decode(CONFIGURED)
            .expect("configured identity hex")
            .try_into()
            .expect("configured identity length");
        let request = vec![
            (Value::from(2_u64), Value::from("group")),
            (Value::from("operation"), Value::from("gperms")),
            (Value::from("step"), Value::from("set")),
            (Value::from("content"), Value::from(format!("admin:{ADMIN}\n"))),
        ];
        let rust_status = rust_node.handle_permission_request(&request, admin)[0];
        let rust_decisions = [configured, admin].map(|identity| {
            rust_node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ)
        });

        let python_temp = tempfile::tempdir().expect("Python fixture directory");
        let python_group = python_temp.path().join("group");
        fs::create_dir_all(&python_group).expect("Python group directory");
        assert!(Command::new("git")
            .args(["init", "--bare", "--quiet", python_group.join("repo").to_string_lossy().as_ref()])
            .status()
            .expect("initialize Python repository")
            .success());
        fs::write(python_group.with_extension("allowed"), format!("read:all\nadmin:{ADMIN}\n"))
            .expect("write initial Python group policy");
        let script = r#"
import json, sys
from threading import Lock
from types import SimpleNamespace
from RNS.Utilities.rngit.server import ReticulumGitNode
group_path, configured, admin = sys.argv[1:]
class AccessSection:
    def __iter__(self): return iter(["group"])
    def as_list(self, _name): return ["read:" + configured]
node = ReticulumGitNode.__new__(ReticulumGitNode)
node.groups = {}
node.blocked_identities = {}
node.identity_aliases = {}
node.perms_lock = Lock()
node.config = {"access": AccessSection()}
node.load_repository_group("group", group_path)
status = node.handle_perms(None, {ReticulumGitNode.IDX_GROUP: "group", "operation": "gperms", "step": "set", "content": "admin:" + admin + "\n"}, 0, SimpleNamespace(hash=bytes.fromhex(admin)), 0)[0]
decisions = [node.resolve_group_permission(SimpleNamespace(hash=bytes.fromhex(identity)), "group", node.PERM_READ) for identity in (configured, admin)]
print(json.dumps({"status": status, "decisions": decisions}))
"#;
        let output = Command::new(std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into()))
            .env("PYTHONPATH", &reference)
            .arg("-c")
            .arg(script)
            .arg(&python_group)
            .arg(CONFIGURED)
            .arg(ADMIN)
            .output()
            .expect("run pinned Python production group-permission handler");
        assert!(output.status.success(), "Python handler failed: {}", String::from_utf8_lossy(&output.stderr));
        let python: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!("Python returned invalid JSON ({error}): {}", String::from_utf8_lossy(&output.stdout))
        });
        assert_eq!(rust_status, 0, "Rust group-permission update should succeed");
        assert_eq!(rust_decisions, [true, true], "Rust refresh should preserve configured access and sidecar admin inheritance");
        assert_eq!(python["status"].as_u64(), Some(0));
        assert_eq!(python["decisions"], serde_json::json!([true, true]));
        assert_eq!(python["decisions"], serde_json::json!(rust_decisions));
    }
}

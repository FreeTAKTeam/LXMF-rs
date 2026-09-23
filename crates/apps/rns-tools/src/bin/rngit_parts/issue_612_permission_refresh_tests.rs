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
}

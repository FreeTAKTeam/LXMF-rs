mod rns_1_5_4 {
    use super::*;
    use rmpv::Value;
    use std::path::PathBuf;

    const AUTHOR: [u8; 16] = [1; 16];
    const WRITER: [u8; 16] = [2; 16];
    const ADMIN: [u8; 16] = [3; 16];

    fn fixture(name: &str) -> (tempfile::TempDir, ReticulumGitNode, PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let group = temp.path().join("group");
        let repository = group.join(name);
        fs::create_dir_all(&repository).expect("repository");
        let mut node = ReticulumGitNode::default();
        let permissions = node.permissions_from_allowed_input(Some(&format!(
            "read:all\nwrite:all\ninteract:all\npropose:all\nadmin:{}\n", hex::encode(ADMIN)
        )));
        node.groups.insert("group".into(), RepositoryGroup {
            name: "group".into(), path: group, permissions,
            repositories: BTreeMap::from([(name.into(), RepositoryRecord {
                name: name.into(), path: repository.clone(), fork: None, mirror: None,
                permissions: Default::default(),
            })]),
        });
        (temp, node, repository)
    }

    fn request(name: &str, operation: &str) -> Vec<(Value, Value)> {
        vec![
            (Value::from(0_u64), Value::from(format!("group/{name}"))),
            (Value::from("operation"), Value::from(operation)),
            (Value::from("doc_id"), Value::from(1_u64)),
            (Value::from("title"), Value::from("Parity")),
            (Value::from("content"), Value::from("Body")),
        ]
    }

    fn create(node: &mut ReticulumGitNode, name: &str, proposed: bool) {
        let operation = if proposed { "propose" } else { "create" };
        assert_eq!(node.handle_work_request(&request(name, operation), AUTHOR)[0], ReticulumGitNode::RES_OK);
    }

    #[test]
    fn work_transitions_require_author_or_administrator() {
        for operation in ["complete", "activate"] {
            let (_temp, mut node, path) = fixture("repo");
            create(&mut node, "repo", operation == "activate");
            let response = node.handle_work_request(&request("repo", operation), WRITER);
            assert_eq!(response[0], ReticulumGitNode::RES_DISALLOWED, "{operation}");
            assert_eq!(&response[1..], b"Not allowed");
            let scope = if operation == "activate" { "proposed" } else { "active" };
            assert!(path.with_extension("work").join(scope).join("1/root").is_file());
            assert_eq!(node.handle_work_request(&request("repo", operation), ADMIN)[0], ReticulumGitNode::RES_OK);
        }
    }

    #[test]
    fn author_can_activate_a_proposal_then_complete_and_reactivate_it() {
        let (_temp, mut node, path) = fixture("repo");
        create(&mut node, "repo", true);
        for (operation, scope) in [("activate", "active"), ("complete", "completed"), ("activate", "active")] {
            let response = node.handle_work_request(&request("repo", operation), AUTHOR);
            assert_eq!(response[0], ReticulumGitNode::RES_OK, "{operation}: {response:?}");
            assert!(path.with_extension("work").join(scope).join("1/root").is_file());
        }
    }

    #[test]
    fn transition_requires_read_write_and_interact_permissions() {
        for denied in ["read", "write", "interact"] {
            let (_temp, mut node, _path) = fixture("repo");
            create(&mut node, "repo", false);
            let permissions = node.permissions_from_allowed_input(Some(&format!("{denied}:none")));
            node.groups.get_mut("group").expect("group").repositories.get_mut("repo").expect("repo").permissions = permissions;
            let response = node.handle_work_request(&request("repo", "complete"), AUTHOR);
            let expected = if denied == "read" { ReticulumGitNode::RES_NOT_FOUND } else { ReticulumGitNode::RES_DISALLOWED };
            assert_eq!(response[0], expected, "denied {denied}");
        }
    }

    #[test]
    fn work_companion_path_preserves_the_repository_extension() {
        let (_temp, mut node, path) = fixture("repo.git");
        create(&mut node, "repo.git", true);
        assert!(path.with_file_name("repo.git.work").join("proposed/1/root").is_file());
        assert!(!path.with_extension("work").exists(), "must not collide with repo.work");
    }

    #[test]
    fn permission_updates_preserve_the_repository_extension_and_refresh_immediately() {
        let (_temp, mut node, path) = fixture("repo.git");
        let mut change = request("repo.git", "rperms");
        change.retain(|(key, _)| key.as_str() != Some("content"));
        change.extend([(Value::from("step"), Value::from("set")), (Value::from("content"), Value::from("read:none"))]);
        assert_eq!(node.handle_permission_request(&change, ADMIN)[0], ReticulumGitNode::RES_OK);
        assert_eq!(fs::read_to_string(path.with_file_name("repo.git.allowed")).expect("canonical sidecar"), "read:none");
        assert!(!path.with_extension("allowed").exists());
        assert!(!node.resolve_permission(&AUTHOR, "group", "repo.git", ReticulumGitNode::PERM_READ));
    }

    #[cfg(unix)]
    #[test]
    fn executable_permission_resolvers_cannot_be_overwritten_remotely() {
        use std::os::unix::fs::PermissionsExt;
        for group_scope in [false, true] {
            let (_temp, mut node, path) = fixture("repo");
            let path = if group_scope { path.parent().expect("group").to_path_buf() } else { path };
            let allowed = path.with_extension("allowed");
            let original = "#!/bin/sh\nprintf 'read:all\\n'\n";
            fs::write(&allowed, original).expect("resolver");
            fs::set_permissions(&allowed, fs::Permissions::from_mode(0o700)).expect("executable");
            let mut change = request("repo", if group_scope { "gperms" } else { "rperms" });
            change.retain(|(key, _)| key.as_str() != Some("content"));
            change.extend([(Value::from(2_u64), Value::from("group")), (Value::from("step"), Value::from("set")), (Value::from("content"), Value::from("read:none"))]);
            let response = node.handle_permission_request(&change, ADMIN);
            assert_eq!(response[0], ReticulumGitNode::RES_DISALLOWED);
            assert_eq!(fs::read_to_string(&allowed).expect("unchanged resolver"), original);
            assert!(node.resolve_permission(&AUTHOR, "group", "repo", ReticulumGitNode::PERM_READ));
        }
    }
    #[test]
    fn ambiguous_legacy_permission_files_fail_closed_on_load() {
        let temp = tempfile::tempdir().expect("tempdir");
        let group = temp.path().join("team.git");
        fs::create_dir_all(&group).expect("group");
        fs::write(group.with_extension("allowed"), "read:none").expect("legacy permissions");
        let mut node = ReticulumGitNode::default();
        let error = node.load_repository_group("team", &group).expect_err("legacy naming must not broaden access");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("legacy permission"));
    }

    #[test]
    fn permission_directories_are_not_treated_as_legacy_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let group = temp.path().join("group.allowed");
        fs::create_dir(&group).expect("group directory");
        assert_eq!(
            crate::permission_sidecar(&group).expect("directory name should be valid"),
            temp.path().join("group.allowed.allowed")
        );

        let repository = temp.path().join("repo.git");
        fs::create_dir(&repository).expect("repository directory");
        fs::create_dir(temp.path().join("repo.allowed")).expect("sibling directory");
        assert_eq!(
            crate::permission_sidecar(&repository).expect("sibling directory should be ignored"),
            temp.path().join("repo.git.allowed")
        );
    }

}

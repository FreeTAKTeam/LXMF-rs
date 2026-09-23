#[test]
fn failed_configured_permission_refresh_does_not_cache_new_policy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let identity = [9_u8; 16];
    fs::create_dir_all(&group_path).expect("group");

    let mut node = ReticulumGitNode::default();
    node.set_configured_group_permissions("group", &format!("read:{}", hex::encode(identity)))
        .expect("initial configured permissions");
    node.load_repository_group("group", &group_path).expect("load group");
    assert!(node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ));
    assert!(!node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_WRITE));

    let sidecar = group_path.with_extension("allowed");
    fs::write(&sidecar, "invalid:permission\n").expect("malformed sidecar");
    assert!(node
        .set_configured_group_permissions("group", "write:all")
        .expect_err("sidecar read failure")
        .contains("invalid permissions"));
    assert!(!node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_WRITE));

    fs::write(&sidecar, "\n").expect("repair sidecar");
    node.update_group_permissions("group").expect("refresh permissions");
    assert!(node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ));
    assert!(!node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_WRITE));
}

#[cfg(unix)]
#[test]
fn failed_executable_permission_refresh_preserves_loaded_policy() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    fs::create_dir_all(&group_path).expect("group");
    let resolver = group_path.with_extension("allowed");
    fs::write(&resolver, "#!/bin/sh\nprintf 'read:all\\n'\n").expect("resolver");
    fs::set_permissions(&resolver, fs::Permissions::from_mode(0o700)).expect("resolver mode");

    let identity = [10_u8; 16];
    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path).expect("load group");
    assert!(node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ));
    assert!(!node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_WRITE));

    fs::write(&resolver, "#!/bin/sh\nexit 7\n").expect("failing resolver");
    assert!(node.update_group_permissions("group").is_err());
    assert!(node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ));
    assert!(!node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_WRITE));
}

#[test]
fn failed_permission_replacement_does_not_update_cached_permissions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let identity = [11_u8; 16];
    fs::create_dir_all(&group_path).expect("group");

    let mut node = ReticulumGitNode::default();
    node.set_configured_group_permissions("group", "admin:all")
        .expect("configured administrator");
    node.load_repository_group("group", &group_path).expect("load group");
    assert!(!node.groups["group"].permissions.read.all);

    let sidecar = group_path.with_extension("allowed");
    fs::create_dir(&sidecar).expect("block sidecar replacement");
    let request = vec![
        (rmpv::Value::String("operation".into()), rmpv::Value::String("gperms".into())),
        (rmpv::Value::from(2_u64), rmpv::Value::String("group".into())),
        (rmpv::Value::String("step".into()), rmpv::Value::String("set".into())),
        (rmpv::Value::String("content".into()), rmpv::Value::String("read:all".into())),
    ];
    assert_eq!(
        node.handle_permission_request(&request, identity)[0],
        ReticulumGitNode::RES_REMOTE_FAIL
    );
    assert!(sidecar.is_dir());
    assert!(!node.groups["group"].permissions.read.all);
}

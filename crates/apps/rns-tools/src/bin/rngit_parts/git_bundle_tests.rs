#[test]
fn local_git_bundle_fetch_and_push_use_the_registered_request_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    let group_path = temp.path().join("group");
    fs::create_dir_all(&source).expect("source");
    fs::create_dir_all(&group_path).expect("group");
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "rngit@example.invalid"],
        vec!["config", "user.name", "rngit-test"],
    ] {
        assert!(Command::new("git")
            .args(&args)
            .current_dir(&source)
            .status()
            .expect("git")
            .success());
    }
    fs::write(source.join("README"), "round trip").expect("file");
    for args in [vec!["add", "README"], vec!["commit", "-qm", "initial"]] {
        assert!(Command::new("git")
            .args(&args)
            .current_dir(&source)
            .status()
            .expect("git")
            .success());
    }

    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path).expect("load group");
    let group = node.groups.get_mut("group").expect("group state");
    group.permissions.read.add(PermissionTarget::All);
    group.permissions.write.add(PermissionTarget::All);
    group.permissions.create.add(PermissionTarget::All);
    let mut client = ReticulumGitClient::default();
    client.attach_local_node(node);
    let remote = "rns://00000000000000000000000000000000/group/repo";
    client.create_repository(remote).expect("create repository");

    let bundle = temp.path().join("source.bundle");
    assert!(Command::new("git")
        .args(["bundle", "create", bundle.to_string_lossy().as_ref(), "--all"])
        .current_dir(&source)
        .status()
        .expect("bundle")
        .success());
    let bundle = fs::read(bundle).expect("bundle bytes");
    let pushed = client
        .process_push_queue(remote, "refs/heads/master", "refs/heads/main", &bundle, false)
        .expect("push");
    assert_eq!(pushed.first().copied(), Some(ReticulumGitNode::RES_OK));
    let fetched = client
        .process_fetch_queue(remote, &["refs/heads/main".to_string()])
        .expect("fetch");
    assert_eq!(fetched.first().copied(), Some(ReticulumGitNode::RES_OK));
    assert!(fetched.len() > 1, "fetch should return the Git bundle payload");
}

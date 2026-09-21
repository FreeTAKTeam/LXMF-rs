#[test]
fn concurrent_work_creators_reserve_distinct_document_directories() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo");
    fs::create_dir_all(&group_path).expect("group");
    assert!(Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git")
        .success());

    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path).expect("load group");
    let permissions = node.groups.get_mut("group").expect("group");
    for list in [
        &mut permissions.permissions.read,
        &mut permissions.permissions.write,
        &mut permissions.permissions.interact,
    ] {
        list.add(PermissionTarget::All);
    }

    let nodes = (0..8).map(|_| node.clone()).collect::<Vec<_>>();
    let threads = nodes
        .into_iter()
        .map(|mut node| {
            std::thread::spawn(move || {
                node.handle_work_request(
                    &[
                        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
                        (rmpv::Value::from("operation"), rmpv::Value::from("create")),
                        (rmpv::Value::from("title"), rmpv::Value::from("Concurrent")),
                        (rmpv::Value::from("content"), rmpv::Value::from("body")),
                    ],
                    [0_u8; 16],
                )
            })
        })
        .collect::<Vec<_>>();
    let responses = threads
        .into_iter()
        .map(|thread| thread.join().expect("creator thread"))
        .collect::<Vec<_>>();
    assert!(responses
        .iter()
        .all(|response| response.first().copied() == Some(ReticulumGitNode::RES_OK)));

    let active = fs::read_dir(group_path.join("repo.work/active"))
        .expect("active documents")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("root").is_file())
        .count();
    assert_eq!(active, 8);
}

#[test]
fn proposed_work_creation_rolls_back_when_permission_sidecar_cannot_be_replaced() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo");
    fs::create_dir_all(&group_path).expect("group");
    assert!(Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git")
        .success());

    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path).expect("load group");
    let group = node.groups.get_mut("group").expect("group");
    for list in [
        &mut group.permissions.read,
        &mut group.permissions.write,
        &mut group.permissions.interact,
        &mut group.permissions.propose,
    ] {
        list.add(PermissionTarget::All);
    }
    fs::create_dir(group_path.join("repo.work")).expect("work root");
    fs::create_dir(group_path.join("repo.work/1.allowed")).expect("blocking sidecar");

    let response = node.handle_work_request(
        &[
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from("propose")),
            (rmpv::Value::from("title"), rmpv::Value::from("Proposal")),
            (rmpv::Value::from("content"), rmpv::Value::from("body")),
        ],
        [0_u8; 16],
    );
    assert_eq!(
        response.first().copied(),
        Some(ReticulumGitNode::RES_REMOTE_FAIL)
    );
    assert!(!group_path.join("repo.work/proposed/1/root").exists());
}

#[test]
fn completing_existing_work_with_unreadable_root_matches_python_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo");
    std::fs::create_dir_all(&group_path).expect("group");
    assert!(std::process::Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .output()
        .expect("git init")
        .status
        .success());

    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path).expect("load group");
    let permissions = &mut node.groups.get_mut("group").expect("group").permissions;
    permissions.read.add(PermissionTarget::All);
    permissions.write.add(PermissionTarget::All);
    permissions.interact.add(PermissionTarget::All);

    let document_dir = group_path.join("repo.work/active/1");
    std::fs::create_dir_all(&document_dir).expect("document directory");
    std::fs::write(document_dir.join("root"), [0xc1]).expect("malformed root");

    let request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::String("group/repo".into())),
        (rmpv::Value::String("operation".into()), rmpv::Value::String("complete".into())),
        (rmpv::Value::String("doc_id".into()), rmpv::Value::from(1_u64)),
    ];

    let response = node.handle_work_request(&request, [12_u8; 16]);
    assert_eq!(response[0], ReticulumGitNode::RES_REMOTE_FAIL);
    assert_eq!(&response[1..], b"Error loading document");
    assert!(document_dir.is_dir(), "failed completion must not move data");
}

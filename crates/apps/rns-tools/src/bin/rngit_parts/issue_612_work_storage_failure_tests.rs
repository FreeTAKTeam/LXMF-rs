#[test]
fn viewing_malformed_persisted_work_returns_remote_failure() {
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
    node.load_repository_group("group", &group_path)
        .expect("load group");
    node.groups
        .get_mut("group")
        .expect("group")
        .permissions
        .read
        .add(PermissionTarget::All);

    let document_dir = group_path.join("repo.work/active/1");
    fs::create_dir_all(&document_dir).expect("document directory");
    let request = vec![
        (
            rmpv::Value::from(0_u64),
            rmpv::Value::String("group/repo".into()),
        ),
        (
            rmpv::Value::String("operation".into()),
            rmpv::Value::String("view".into()),
        ),
        (
            rmpv::Value::String("doc_id".into()),
            rmpv::Value::from(1_u64),
        ),
        (
            rmpv::Value::String("scope".into()),
            rmpv::Value::String("active".into()),
        ),
    ];

    for (index, malformed_record) in [vec![0xc1], vec![0x80, 0xc1]].into_iter().enumerate() {
        fs::write(document_dir.join("root"), malformed_record)
            .expect("malformed persisted document");
        let response = node.handle_work_request(&request, [12_u8; 16]);
        assert_eq!(
            response[0],
            ReticulumGitNode::RES_REMOTE_FAIL,
            "malformed record {index} status"
        );
        assert_eq!(&response[1..], b"Error loading document");
    }
}

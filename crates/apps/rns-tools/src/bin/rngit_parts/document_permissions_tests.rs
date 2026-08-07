#[test]
fn document_permissions_restrict_work_item_operations_and_listing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    fs::create_dir_all(&group_path).expect("group");
    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path).expect("load group");
    let group = node.groups.get_mut("group").expect("group state");
    for permissions in [
        &mut group.permissions.read,
        &mut group.permissions.write,
        &mut group.permissions.create,
        &mut group.permissions.interact,
        &mut group.permissions.admin,
    ] {
        permissions.add(PermissionTarget::All);
    }

    let mut client = ReticulumGitClient::default();
    client.attach_local_node(node);
    let remote = "rns://00000000000000000000000000000000/group/repo";
    client.create_repository(remote).expect("create repository");
    assert_eq!(
        client.work_create(remote, "Restricted", "Body").expect("create document")[0],
        ReticulumGitNode::RES_OK
    );
    fs::write(
        group_path.join("repo.work").join("1.allowed"),
        "read:none\nwrite:none\ninteract:none\nadmin:none\n",
    )
    .expect("document permissions");

    let listed = client.work_list(remote, "active").expect("list work documents");
    assert_eq!(listed.first().copied(), Some(ReticulumGitNode::RES_OK));
    let listed = rmpv::decode::read_value(&mut std::io::Cursor::new(&listed[1..]))
        .expect("decode work listing");
    assert_eq!(listed.as_array().map(Vec::len), Some(0));

    assert_eq!(
        client.work_view(remote, 1, "active").expect("view response")[0],
        ReticulumGitNode::RES_DISALLOWED
    );
    assert_eq!(
        client.work_edit(remote, 1, "Restricted", "Changed", "active").expect("edit response")[0],
        ReticulumGitNode::RES_DISALLOWED
    );
    assert_eq!(
        client.work_comment(remote, 1, "active", "Comment").expect("comment response")[0],
        ReticulumGitNode::RES_DISALLOWED
    );
    assert_eq!(
        client.work_delete(remote, 1, "active").expect("delete response")[0],
        ReticulumGitNode::RES_DISALLOWED
    );
}

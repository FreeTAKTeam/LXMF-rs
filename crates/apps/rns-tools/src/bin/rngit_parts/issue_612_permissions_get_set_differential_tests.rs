#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn repository_admin_without_document_admin_cannot_get_or_set_permissions_like_pinned_python() {
    let repository_permissions = format!(
        "read:all\nwrite:all\ninteract:all\nadmin:{}\n",
        hex::encode(REMOTE)
    );
    let document_permissions = "admin:none\nread:all\nwrite:all\ninteract:all\n";

    let (rust_get, python_get) = assert_work_operation_differential(
        "perms",
        &repository_permissions,
        document_permissions,
        OTHER_AUTHOR,
    );
    assert_eq!(python_get["status"], ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(rust_get.status, ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(
        rust_get.permission_content.as_deref(),
        Some(document_permissions)
    );

    let updated_permissions = "admin:none\nread:none\n";
    let (rust_set, python_set) = assert_work_operation_step_differential(
        "perms",
        &repository_permissions,
        document_permissions,
        OTHER_AUTHOR,
        "set",
        updated_permissions,
    );
    assert_eq!(python_set["status"], ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(rust_set.status, ReticulumGitNode::RES_DISALLOWED);
    assert_eq!(
        rust_set.permission_content.as_deref(),
        Some(document_permissions)
    );
}

#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn document_author_or_document_admin_can_manage_permissions_like_pinned_python() {
    let repository_permissions = format!(
        "read:all\nwrite:all\ninteract:all\nadmin:{}\n",
        hex::encode(REMOTE)
    );
    let author_permissions = "admin:none\nread:none\n";
    let (rust_author, python_author) = assert_work_operation_differential(
        "perms",
        &repository_permissions,
        author_permissions,
        REMOTE,
    );
    assert_eq!(python_author["status"], ReticulumGitNode::RES_OK);
    assert_eq!(rust_author.status, ReticulumGitNode::RES_OK);

    let document_admin_permissions = format!("admin:{}\nread:none\n", hex::encode(REMOTE));
    let (rust_admin, python_admin) = assert_work_operation_step_differential(
        "perms",
        &repository_permissions,
        &document_admin_permissions,
        OTHER_AUTHOR,
        "set",
        "admin:none\nread:all\n",
    );
    assert_eq!(python_admin["status"], ReticulumGitNode::RES_OK);
    assert_eq!(rust_admin.status, ReticulumGitNode::RES_OK);
    assert_eq!(
        rust_admin.permission_content.as_deref(),
        Some("admin:none\nread:all\n")
    );
}

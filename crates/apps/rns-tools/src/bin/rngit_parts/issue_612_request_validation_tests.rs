fn request_validation_fixture() -> (tempfile::TempDir, std::path::PathBuf, ReticulumGitNode) {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo");
    fs::create_dir_all(&group_path).expect("group directory");
    assert!(Command::new("git")
        .args(["init", "--bare", "--quiet", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git init")
        .success());

    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path)
        .expect("load repository group");
    let permissions = &mut node.groups.get_mut("group").expect("group state").permissions;
    for permission in [
        &mut permissions.read,
        &mut permissions.write,
        &mut permissions.interact,
        &mut permissions.propose,
        &mut permissions.admin,
    ] {
        permission.add(PermissionTarget::All);
    }
    (temp, group_path, node)
}

fn snapshot_work_request_state(root: &std::path::Path) -> Vec<(std::path::PathBuf, Option<Vec<u8>>)> {
    fn visit(
        root: &std::path::Path,
        directory: &std::path::Path,
        state: &mut Vec<(std::path::PathBuf, Option<Vec<u8>>)>,
    ) {
        let mut entries = fs::read_dir(directory)
            .expect("read state directory")
            .map(|entry| entry.expect("directory entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            let relative = path.strip_prefix(root).expect("path under snapshot root").to_owned();
            if path.is_dir() {
                state.push((relative, None));
                visit(root, &path, state);
            } else {
                state.push((relative, Some(fs::read(path).expect("read state file"))));
            }
        }
    }

    let mut state = Vec::new();
    visit(root, root, &mut state);
    state
}

fn encode_work_request(entries: Vec<(rmpv::Value, rmpv::Value)>) -> Vec<u8> {
    let request = rmpv::Value::Map(entries);
    super::pack_value(&request).expect("encode work request")
}

fn assert_work_request_error_without_state_change(
    node: &mut ReticulumGitNode,
    group_path: &std::path::Path,
    request: &[u8],
    expected_message: &[u8],
) {
    let before = snapshot_work_request_state(group_path);
    let response = node.handle_request(
        "/mgmt/work",
        request,
        [0x42; 16],
    );
    assert_eq!(response.first(), Some(&ReticulumGitNode::RES_INVALID_REQ));
    assert_eq!(&response[1..], expected_message);
    assert_eq!(snapshot_work_request_state(group_path), before);
}

#[test]
fn work_request_missing_repository_returns_exact_invalid_request_without_state_change() {
    let (_temp, group_path, mut node) = request_validation_fixture();
    let request = encode_work_request(vec![(
        rmpv::Value::String("operation".into()),
        rmpv::Value::String("list".into()),
    )]);

    assert_work_request_error_without_state_change(
        &mut node,
        &group_path,
        &request,
        b"No repository specified",
    );
}

#[test]
fn work_request_wrong_repository_key_type_returns_exact_missing_repository_error() {
    let (_temp, group_path, mut node) = request_validation_fixture();
    let request = encode_work_request(vec![
        (
            rmpv::Value::String("0".into()),
            rmpv::Value::String("group/repo".into()),
        ),
        (
            rmpv::Value::String("operation".into()),
            rmpv::Value::String("list".into()),
        ),
    ]);

    assert_work_request_error_without_state_change(
        &mut node,
        &group_path,
        &request,
        b"No repository specified",
    );
}

#[test]
fn work_request_missing_operation_returns_exact_invalid_request_without_state_change() {
    let (_temp, group_path, mut node) = request_validation_fixture();
    let request = encode_work_request(vec![(
        rmpv::Value::from(0_u64),
        rmpv::Value::String("group/repo".into()),
    )]);

    assert_work_request_error_without_state_change(
        &mut node,
        &group_path,
        &request,
        b"Invalid request",
    );
}

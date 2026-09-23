fn rngit_work_shape_node() -> (tempfile::TempDir, ReticulumGitNode, std::path::PathBuf) {
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
    node.groups
        .get_mut("group")
        .expect("group")
        .permissions
        .read
        .add(PermissionTarget::All);
    (temp, node, group_path)
}

fn rngit_work_request(operation: &str) -> Vec<(rmpv::Value, rmpv::Value)> {
    vec![
        (
            rmpv::Value::from(0_u64),
            rmpv::Value::String("group/repo".into()),
        ),
        (
            rmpv::Value::String("operation".into()),
            rmpv::Value::String(operation.into()),
        ),
        (
            rmpv::Value::String("doc_id".into()),
            rmpv::Value::from(1_u64),
        ),
        (
            rmpv::Value::String("scope".into()),
            rmpv::Value::String("active".into()),
        ),
    ]
}

fn rngit_work_fixture(value: &rmpv::Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    rmpv::encode::write_value(&mut bytes, value).expect("encode work MessagePack");
    bytes
}

fn rngit_work_success_payload(response: &[u8]) -> rmpv::Value {
    assert_eq!(response.first(), Some(&ReticulumGitNode::RES_OK));
    rmpv::decode::read_value(&mut std::io::Cursor::new(&response[1..]))
        .expect("decode work response MessagePack")
}

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

    fs::write(
        document_dir.join("root"),
        rngit_work_fixture(&rmpv::Value::Map(Vec::new())),
    )
    .expect("empty persisted document");
    let response = node.handle_work_request(&request, [12_u8; 16]);
    assert_eq!(response[0], ReticulumGitNode::RES_REMOTE_FAIL);
    assert_eq!(&response[1..], b"Error loading document");
}

#[test]
fn missing_work_metadata_uses_python_view_and_list_defaults() {
    let (_temp, mut node, group_path) = rngit_work_shape_node();
    let document_dir = group_path.join("repo.work/active/1");
    fs::create_dir_all(&document_dir).expect("document directory");
    let document = rmpv::Value::Map(vec![(
        rmpv::Value::String("content".into()),
        rmpv::Value::String("Python-shaped body".into()),
    )]);
    fs::write(document_dir.join("root"), rngit_work_fixture(&document))
        .expect("write Python-shaped document");

    let view = rngit_work_success_payload(
        &node.handle_work_request(&rngit_work_request("view"), [12_u8; 16]),
    );
    let view_map = view.as_map().expect("view payload map");
    assert_eq!(
        map_value(view_map, &rmpv::Value::String("content".into())).and_then(rmpv::Value::as_str),
        Some("Python-shaped body")
    );
    let metadata = map_value(view_map, &rmpv::Value::String("meta".into()))
        .and_then(rmpv::Value::as_map)
        .expect("view metadata map");
    assert_eq!(
        map_value(metadata, &rmpv::Value::String("title".into())).and_then(rmpv::Value::as_str),
        Some("Untitled")
    );
    assert_eq!(
        map_value(metadata, &rmpv::Value::String("format".into())).and_then(rmpv::Value::as_str),
        Some("markdown")
    );

    let mut list_request = rngit_work_request("list");
    list_request.retain(|(key, _)| key != &rmpv::Value::String("doc_id".into()));
    let list = rngit_work_success_payload(&node.handle_work_request(&list_request, [12_u8; 16]));
    let listed = list
        .as_map()
        .and_then(|map| map_value(map, &rmpv::Value::String("active".into())))
        .and_then(rmpv::Value::as_array)
        .and_then(|documents| documents.first())
        .and_then(rmpv::Value::as_map)
        .expect("active work list contains the document");
    assert_eq!(
        map_value(listed, &rmpv::Value::String("title".into())).and_then(rmpv::Value::as_str),
        Some("Untitled")
    );
    assert_eq!(
        map_value(listed, &rmpv::Value::String("format".into())).and_then(rmpv::Value::as_str),
        Some("markdown")
    );
}

#[test]
fn missing_work_edited_timestamp_defaults_to_zero_in_view_list_and_comments() {
    let (_temp, mut node, group_path) = rngit_work_shape_node();
    let document_dir = group_path.join("repo.work/active/1");
    fs::create_dir_all(&document_dir).expect("document directory");
    let document = rmpv::Value::Map(vec![
        (
            rmpv::Value::String("content".into()),
            rmpv::Value::String("work body".into()),
        ),
        (
            rmpv::Value::String("meta".into()),
            rmpv::Value::Map(vec![(
                rmpv::Value::String("created".into()),
                rmpv::Value::F64(123.5),
            )]),
        ),
    ]);
    let comment = rmpv::Value::Map(vec![
        (
            rmpv::Value::String("content".into()),
            rmpv::Value::String("comment body".into()),
        ),
        (
            rmpv::Value::String("meta".into()),
            rmpv::Value::Map(vec![(
                rmpv::Value::String("created".into()),
                rmpv::Value::F64(456.5),
            )]),
        ),
    ]);
    fs::write(document_dir.join("root"), rngit_work_fixture(&document))
        .expect("write Python-shaped document");
    fs::write(document_dir.join("1"), rngit_work_fixture(&comment))
        .expect("write Python-shaped comment");

    let view = rngit_work_success_payload(
        &node.handle_work_request(&rngit_work_request("view"), [12_u8; 16]),
    );
    let view_map = view.as_map().expect("view payload map");
    let metadata = map_value(view_map, &rmpv::Value::String("meta".into()))
        .and_then(rmpv::Value::as_map)
        .expect("view metadata map");
    assert_eq!(
        map_value(metadata, &rmpv::Value::String("created".into())).and_then(rmpv::Value::as_f64),
        Some(123.5)
    );
    assert_eq!(
        map_value(metadata, &rmpv::Value::String("edited".into())).and_then(rmpv::Value::as_u64),
        Some(0)
    );
    let comments = map_value(view_map, &rmpv::Value::String("comments".into()))
        .and_then(rmpv::Value::as_array)
        .expect("comments array");
    let comment_payload = comments[0].as_map().expect("comment payload");
    assert_eq!(
        map_value(comment_payload, &rmpv::Value::String("created".into()))
            .and_then(rmpv::Value::as_f64),
        Some(456.5)
    );
    assert_eq!(
        map_value(comment_payload, &rmpv::Value::String("edited".into()))
            .and_then(rmpv::Value::as_u64),
        Some(0)
    );

    let mut list_request = rngit_work_request("list");
    list_request.retain(|(key, _)| key != &rmpv::Value::String("doc_id".into()));
    let list = rngit_work_success_payload(&node.handle_work_request(&list_request, [12_u8; 16]));
    let listed = list
        .as_map()
        .and_then(|map| map_value(map, &rmpv::Value::String("active".into())))
        .and_then(rmpv::Value::as_array)
        .and_then(|documents| documents.first())
        .and_then(rmpv::Value::as_map)
        .expect("active work list contains the document");
    assert_eq!(
        map_value(listed, &rmpv::Value::String("edited".into())).and_then(rmpv::Value::as_u64),
        Some(0)
    );
}

#[test]
fn malformed_nested_work_metadata_matches_python_view_list_and_comment_behavior() {
    let (_temp, mut node, group_path) = rngit_work_shape_node();
    let document_dir = group_path.join("repo.work/active/1");
    fs::create_dir_all(&document_dir).expect("document directory");
    let valid_document = rmpv::Value::Map(vec![(
        rmpv::Value::String("content".into()),
        rmpv::Value::String("Python-shaped body".into()),
    )]);
    fs::write(document_dir.join("root"), rngit_work_fixture(&valid_document))
        .expect("write document without optional metadata");

    let malformed_comment = rmpv::Value::Map(vec![
        (
            rmpv::Value::String("content".into()),
            rmpv::Value::String("bad comment metadata".into()),
        ),
        (
            rmpv::Value::String("meta".into()),
            rmpv::Value::String("not a metadata map".into()),
        ),
    ]);
    fs::write(document_dir.join("1"), rngit_work_fixture(&malformed_comment))
        .expect("write malformed comment record");
    let view = rngit_work_success_payload(
        &node.handle_work_request(&rngit_work_request("view"), [12_u8; 16]),
    );
    let comments = view
        .as_map()
        .and_then(|map| map_value(map, &rmpv::Value::String("comments".into())))
        .and_then(rmpv::Value::as_array)
        .expect("comments array");
    assert!(comments.is_empty(), "Python skips malformed comment metadata");

    let malformed_document = rmpv::Value::Map(vec![
        (
            rmpv::Value::String("content".into()),
            rmpv::Value::String("malformed metadata".into()),
        ),
        (
            rmpv::Value::String("meta".into()),
            rmpv::Value::String("not a metadata map".into()),
        ),
    ]);
    fs::write(document_dir.join("root"), rngit_work_fixture(&malformed_document))
        .expect("write malformed document metadata");
    let view_response = node.handle_work_request(&rngit_work_request("view"), [12_u8; 16]);
    assert_eq!(view_response[0], ReticulumGitNode::RES_REMOTE_FAIL);
    assert_eq!(&view_response[1..], b"Remote error");

    let mut list_request = rngit_work_request("list");
    list_request.retain(|(key, _)| key != &rmpv::Value::String("doc_id".into()));
    let list = rngit_work_success_payload(&node.handle_work_request(&list_request, [12_u8; 16]));
    let active = list
        .as_map()
        .and_then(|map| map_value(map, &rmpv::Value::String("active".into())))
        .and_then(rmpv::Value::as_array)
        .expect("active work list");
    assert!(active.is_empty(), "Python skips records with malformed metadata");
}

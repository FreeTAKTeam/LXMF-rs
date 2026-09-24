#[cfg(unix)]
#[test]
fn executable_permission_resolvers_are_bounded_and_not_remote_replaceable() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo");
    fs::create_dir_all(&group_path).expect("group");
    assert!(Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git")
        .success());
    let resolver = group_path.with_extension("allowed");
    fs::write(&resolver, "#!/bin/sh\nprintf 'read:all\\nadmin:all\\n'\n").expect("resolver");
    fs::set_permissions(&resolver, fs::Permissions::from_mode(0o700)).expect("resolver mode");

    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path).expect("load group");
    let remote = [7_u8; 16];
    assert!(node.resolve_group_permission(&remote, "group", ReticulumGitNode::PERM_READ));

    let request = vec![
        (rmpv::Value::String("operation".into()), rmpv::Value::String("gperms".into())),
        (rmpv::Value::from(2_u64), rmpv::Value::String("group".into())),
        (rmpv::Value::String("step".into()), rmpv::Value::String("set".into())),
        (rmpv::Value::String("content".into()), rmpv::Value::String("read:none".into())),
    ];
    assert_eq!(
        node.handle_permission_request(&request, remote)[0],
        ReticulumGitNode::RES_DISALLOWED
    );
    assert_eq!(
        fs::read_to_string(&resolver).expect("resolver remains"),
        "#!/bin/sh\nprintf 'read:all\\nadmin:all\\n'\n"
    );
}

#[test]
fn configured_group_permissions_merge_without_overriding_file_denials() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo");
    let identity = [8_u8; 16];
    fs::create_dir_all(&group_path).expect("group");
    assert!(Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git")
        .success());

    let mut node = ReticulumGitNode::default();
    node.set_configured_group_permissions("group", &format!("read:{}", hex::encode(identity)))
        .expect("configured permissions");
    node.load_repository_group("group", &group_path).expect("load group");
    assert!(node.resolve_permission(&identity, "group", "repo", ReticulumGitNode::PERM_READ));

    fs::write(group_path.with_extension("allowed"), "read:none\n").expect("deny file");
    node.load_repository_group("group", &group_path).expect("reload group");
    assert!(!node.resolve_permission(&identity, "group", "repo", ReticulumGitNode::PERM_READ));
}

#[cfg(unix)]
#[test]
fn failed_executable_permission_resolvers_fail_closed_before_registration() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    fs::create_dir_all(&group_path).expect("group");
    let resolver = group_path.with_extension("allowed");
    fs::write(&resolver, "#!/bin/sh\nexit 7\n").expect("resolver");
    fs::set_permissions(&resolver, fs::Permissions::from_mode(0o700)).expect("resolver mode");

    let mut node = ReticulumGitNode::default();
    assert!(node.load_repository_group("group", &group_path).is_err());
    assert!(node.groups.is_empty());
}

#[test]
fn dotted_repository_names_use_canonical_companions_and_reject_ambiguous_legacy_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo.git");
    fs::create_dir_all(&group_path).expect("group");
    assert!(Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git")
        .success());
    fs::write(group_path.join("repo.allowed"), "read:all\n").expect("legacy sidecar");
    let mut node = ReticulumGitNode::default();
    assert!(node.load_repository_group("group", &group_path).is_err());

    fs::remove_file(group_path.join("repo.allowed")).expect("legacy sidecar");
    fs::create_dir(group_path.join("repo.allowed")).expect("directory sidecar");
    node.load_repository_group("group", &group_path).expect("directory is not a sidecar");
    assert!(node
        .groups
        .get("group")
        .and_then(|group| group.repositories.get("repo.git"))
        .is_some());
}

#[test]
fn work_storage_matches_python_messagepack_shapes_and_survives_transitions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    fs::create_dir_all(&group_path).expect("group");
    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path).expect("load group");
    let group = node.groups.get_mut("group").expect("group state");
    for permissions in [
        &mut group.permissions.read,
        &mut group.permissions.write,
        &mut group.permissions.interact,
        &mut group.permissions.propose,
        &mut group.permissions.admin,
    ] {
        permissions.add(PermissionTarget::All);
    }

    let mut client = ReticulumGitClient::default();
    client.attach_local_node(node);
    let remote = "rns://00000000000000000000000000000000/group/repo";
    client.create_repository(remote).expect("create repository");
    client.work_create(remote, "Title", "Body").expect("create document");
    let work_root = group_path.join("repo.work");
    let root = rmpv::decode::read_value(&mut std::io::Cursor::new(
        fs::read(work_root.join("active/1/root")).expect("root document"),
    ))
    .expect("decode root document");
    assert!(map_value(root.as_map().expect("root map"), &rmpv::Value::String("id".into()))
        .is_none());
    assert!(map_value(
        root.as_map().expect("root map"),
        &rmpv::Value::String("comments".into())
    )
    .is_none());
    let meta = map_value(root.as_map().expect("root map"), &rmpv::Value::String("meta".into()))
        .and_then(rmpv::Value::as_map)
        .expect("root metadata");
    assert_eq!(
        map_value(meta, &rmpv::Value::String("author".into())).and_then(rmpv::Value::as_slice),
        Some([0_u8; 16].as_slice())
    );
    assert!(map_value(meta, &rmpv::Value::String("created".into()))
        .and_then(rmpv::Value::as_f64)
        .is_some());

    client.work_comment(remote, 1, "active", "Comment").expect("add comment");
    assert!(work_root.join("active/1/1").is_file());
    let listed = client.work_list(remote, "active").expect("list documents");
    let listed =
        rmpv::decode::read_value(&mut std::io::Cursor::new(&listed[1..])).expect("decode list");
    let active = map_value(
        listed.as_map().expect("list map"),
        &rmpv::Value::String("active".into()),
    )
    .and_then(rmpv::Value::as_array)
    .expect("active list");
    assert_eq!(active.len(), 1);
    assert_eq!(
        map_value(
            active[0].as_map().expect("summary"),
            &rmpv::Value::String("comments".into()),
        )
        .and_then(rmpv::Value::as_u64),
        Some(1)
    );

    let viewed = client.work_view(remote, 1, "active").expect("view document");
    let viewed =
        rmpv::decode::read_value(&mut std::io::Cursor::new(&viewed[1..])).expect("decode view");
    let viewed_map = viewed.as_map().expect("view map");
    assert_eq!(
        map_value(viewed_map, &rmpv::Value::String("comments".into()))
            .and_then(rmpv::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    let viewed_meta = map_value(viewed_map, &rmpv::Value::String("meta".into()))
        .and_then(rmpv::Value::as_map)
        .expect("view metadata");
    assert_eq!(
        map_value(viewed_meta, &rmpv::Value::String("author".into()))
            .and_then(rmpv::Value::as_str),
        Some("00000000000000000000000000000000")
    );

    client.work_complete(remote, 1).expect("complete document");
    assert!(work_root.join("completed/1/root").is_file());
    client.work_activate(remote, 1).expect("activate document");
    assert!(work_root.join("active/1/root").is_file());
}

#[test]
fn python_produced_work_document_round_trips_binary_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let document_dir = temp.path().join("active/42");
    fs::create_dir_all(&document_dir).expect("document directory");
    let python_fixture = hex::decode(
        "82a7636f6e74656e74ab507974686f6e20626f6479a46d65746187a6666f726d6174a86d61726b646f776ea57469746c65ac507974686f6e207469746c65a763726561746564cb41d954fc40100000a6656469746564cb41d954fc40600000a6617574686f72c41000112233445566778899aabbccddeeffa97369676e6174757265c440000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3fa86964656e74697479c420000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    )
    .expect("fixture hex");
    fs::write(document_dir.join("root"), python_fixture).expect("write Python fixture");

    let node = ReticulumGitNode::default();
    let document = node
        .work_load_document(&document_dir.join("root"))
        .expect("load Python document");
    let metadata = document
        .as_map()
        .and_then(|map| map_value(map, &rmpv::Value::String("meta".into())))
        .and_then(rmpv::Value::as_map)
        .expect("Python metadata");
    let expected_author = hex::decode("00112233445566778899aabbccddeeff").expect("author");
    assert_eq!(
        map_value(metadata, &rmpv::Value::String("author".into()))
            .and_then(rmpv::Value::as_slice),
        Some(expected_author.as_slice())
    );
    assert_eq!(
        map_value(metadata, &rmpv::Value::String("created".into()))
            .and_then(rmpv::Value::as_f64),
        Some(1_700_000_000.25)
    );

    let payload = node.work_view_payload("active", 42, &document_dir, &document);
    let payload = payload.as_map().expect("view payload");
    assert_eq!(
        map_value(payload, &rmpv::Value::String("content".into()))
            .and_then(rmpv::Value::as_str),
        Some("Python body")
    );
    let view_meta = map_value(payload, &rmpv::Value::String("meta".into()))
        .and_then(rmpv::Value::as_map)
        .expect("view metadata");
    assert_eq!(
        map_value(view_meta, &rmpv::Value::String("author".into()))
            .and_then(rmpv::Value::as_str),
        Some("00112233445566778899aabbccddeeff")
    );
    assert_eq!(
        map_value(view_meta, &rmpv::Value::String("signature".into()))
            .and_then(rmpv::Value::as_slice)
            .map(<[u8]>::len),
        Some(64)
    );
    assert_eq!(
        map_value(view_meta, &rmpv::Value::String("identity".into()))
            .and_then(rmpv::Value::as_slice)
            .map(<[u8]>::len),
        Some(32)
    );
}

#[test]
fn identified_peer_work_requests_verify_signatures_and_store_public_identity() {
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
    let group = node.groups.get_mut("group").expect("group state");
    for permissions in [
        &mut group.permissions.read,
        &mut group.permissions.write,
        &mut group.permissions.interact,
    ] {
        permissions.add(PermissionTarget::All);
    }

    let signer = rns_transport::identity::PrivateIdentity::new_from_name("issue-612-signer");
    let remote: [u8; 16] = signer
        .address_hash()
        .as_slice()
        .try_into()
        .expect("identity hash");
    let content = "signed work body";
    let request = |signature: Vec<u8>| {
        vec![
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from("create")),
            (rmpv::Value::from("title"), rmpv::Value::from("Signed work")),
            (rmpv::Value::from("content"), rmpv::Value::from(content)),
            (rmpv::Value::from("format"), rmpv::Value::from("markdown")),
            (rmpv::Value::from("signature"), rmpv::Value::Binary(signature)),
        ]
    };

    let invalid = node.handle_work_request_with_peer_identity(
        &request(vec![0; 64]),
        remote,
        Some(*signer.as_identity()),
    );
    assert_eq!(invalid[0], ReticulumGitNode::RES_INVALID_REQ);
    assert!(!group_path.join("repo.work/active/1/root").exists());

    let valid = node.handle_work_request_with_peer_identity(
        &request(signer.sign(content.as_bytes()).to_bytes().to_vec()),
        remote,
        Some(*signer.as_identity()),
    );
    assert_eq!(valid[0], ReticulumGitNode::RES_OK);
    let document = node
        .work_load_document(&group_path.join("repo.work/active/1/root"))
        .expect("stored work document");
    let metadata = document
        .as_map()
        .and_then(|map| map_value(map, &rmpv::Value::String("meta".into())))
        .and_then(rmpv::Value::as_map)
        .expect("stored metadata");
    assert_eq!(
        map_value(metadata, &rmpv::Value::String("signature".into()))
            .and_then(rmpv::Value::as_slice)
            .map(<[u8]>::len),
        Some(64)
    );
    assert_eq!(
        map_value(metadata, &rmpv::Value::String("identity".into()))
            .and_then(rmpv::Value::as_slice)
            .map(<[u8]>::len),
        Some(64)
    );
}

#[test]
fn work_documents_and_permission_sidecars_survive_node_reload() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("rngit-root");
    let group_path = root.join("group");
    let repository_path = group_path.join("repo");
    fs::create_dir_all(&group_path).expect("group");
    assert!(Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git")
        .success());
    fs::write(
        root.join("group.allowed"),
        "read:all\nwrite:all\ninteract:all\nadmin:all\n",
    )
    .expect("group permissions");

    let signer = rns_transport::identity::PrivateIdentity::new_from_name("issue-612-reload");
    let remote: [u8; 16] = signer
        .address_hash()
        .as_slice()
        .try_into()
        .expect("identity hash");
    let content = "persisted work body";
    let request = vec![
        (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
        (rmpv::Value::from("operation"), rmpv::Value::from("create")),
        (rmpv::Value::from("title"), rmpv::Value::from("Reload me")),
        (rmpv::Value::from("content"), rmpv::Value::from(content)),
        (rmpv::Value::from("format"), rmpv::Value::from("markdown")),
        (
            rmpv::Value::from("signature"),
            rmpv::Value::Binary(signer.sign(content.as_bytes()).to_bytes().to_vec()),
        ),
    ];
    let mut node = ReticulumGitNode::default();
    assert_eq!(node.load_repository_root(&root).expect("initial load"), 1);
    let response = node.handle_work_request_with_peer_identity(
        &request,
        remote,
        Some(*signer.as_identity()),
    );
    assert_eq!(response[0], ReticulumGitNode::RES_OK);
    fs::write(
        group_path.join("repo.work/1.allowed"),
        "read:all\nwrite:all\ninteract:all\nadmin:all\n",
    )
    .expect("document permissions");

    let mut restarted = ReticulumGitNode::default();
    assert_eq!(restarted.load_repository_root(&root).expect("reload"), 1);
    let listed = restarted.handle_work_request(
        &[
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from("list")),
            (rmpv::Value::from("scope"), rmpv::Value::from("active")),
        ],
        remote,
    );
    assert_eq!(listed[0], ReticulumGitNode::RES_OK);
    let listed = rmpv::decode::read_value(&mut std::io::Cursor::new(&listed[1..]))
        .expect("decode reloaded list");
    let active = map_value(
        listed.as_map().expect("list map"),
        &rmpv::Value::String("active".into()),
    )
    .and_then(rmpv::Value::as_array)
    .expect("active list");
    assert_eq!(active.len(), 1);
    let viewed = restarted.handle_work_request(
        &[
            (rmpv::Value::from(0_u64), rmpv::Value::from("group/repo")),
            (rmpv::Value::from("operation"), rmpv::Value::from("view")),
            (rmpv::Value::from("doc_id"), rmpv::Value::from(1_u64)),
            (rmpv::Value::from("scope"), rmpv::Value::from("active")),
        ],
        remote,
    );
    assert_eq!(viewed[0], ReticulumGitNode::RES_OK);
    let viewed = rmpv::decode::read_value(&mut std::io::Cursor::new(&viewed[1..]))
        .expect("decode reloaded view");
    assert_eq!(
        map_value(viewed.as_map().expect("view map"), &rmpv::Value::String("content".into()))
            .and_then(rmpv::Value::as_str),
        Some(content)
    );
}

include!("issue_612_activation_authorization_differential_tests.rs");
include!("issue_612_view_missing_id_differential_tests.rs");
include!("issue_612_propose_success_differential_tests.rs");

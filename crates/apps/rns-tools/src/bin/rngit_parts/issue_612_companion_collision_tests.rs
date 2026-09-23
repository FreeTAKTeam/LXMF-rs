#[test]
fn canonical_companion_roots_isolate_repo_and_repo_git_through_service_reload() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repo_path = group_path.join("repo");
    let dotted_repo_path = group_path.join("repo.git");
    fs::create_dir_all(&group_path).expect("group");
    for repository_path in [&repo_path, &dotted_repo_path] {
        assert!(Command::new("git")
            .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
            .status()
            .expect("git init")
            .success());
    }

    let repo_permissions = group_path.join("repo.allowed");
    let dotted_permissions = group_path.join("repo.git.allowed");
    fs::write(&repo_permissions, "read:all\nwrite:all\ninteract:all\nrelease:all\nadmin:all\n")
        .expect("repo permissions");
    fs::write(&dotted_permissions, "read:all\nwrite:all\ninteract:all\nrelease:all\nadmin:all\n")
        .expect("dotted repo permissions");

    // Python's extension-replacement spelling for repo.git aliases repo.allowed.
    assert_eq!(dotted_repo_path.with_extension("allowed"), repo_permissions);
    assert_ne!(dotted_repo_path.with_extension("allowed"), dotted_permissions);

    let mut node = ReticulumGitNode::default();
    assert_eq!(node.load_repository_group("group", &group_path).expect("register repositories"), 2);
    let remote = [21_u8; 16];
    let work_request = |repository: &str, title: &str| {
        vec![
            (rmpv::Value::from(0_u64), rmpv::Value::String(format!("group/{repository}").into())),
            (rmpv::Value::String("operation".into()), rmpv::Value::String("create".into())),
            (rmpv::Value::String("title".into()), rmpv::Value::String(title.into())),
            (rmpv::Value::String("content".into()), rmpv::Value::String(format!("body for {title}").into())),
        ]
    };
    for (repository, title) in [("repo", "plain"), ("repo.git", "dotted")] {
        assert_eq!(node.handle_work_request(&work_request(repository, title), remote)[0], ReticulumGitNode::RES_OK);
        let release_request = vec![
            (rmpv::Value::from(0_u64), rmpv::Value::String(format!("group/{repository}").into())),
            (rmpv::Value::String("operation".into()), rmpv::Value::String("create".into())),
            (rmpv::Value::String("target".into()), rmpv::Value::String(format!("v-{title}").into())),
        ];
        assert_eq!(node.handle_release_request(&release_request, remote)[0], ReticulumGitNode::RES_OK);
    }
    let update_dotted_permissions = vec![
        (rmpv::Value::String("operation".into()), rmpv::Value::String("rperms".into())),
        (rmpv::Value::from(0_u64), rmpv::Value::String("group/repo.git".into())),
        (rmpv::Value::String("step".into()), rmpv::Value::String("set".into())),
        (rmpv::Value::String("content".into()), rmpv::Value::String("read:none\nwrite:none\ninteract:none\nrelease:none\nadmin:all\n".into())),
    ];
    assert_eq!(node.handle_permission_request(&update_dotted_permissions, remote)[0], ReticulumGitNode::RES_OK);

    let plain_work = group_path.join("repo.work/active/1/root");
    let dotted_work = group_path.join("repo.git.work/active/1/root");
    let plain_release = group_path.join("repo.releases/v-plain/META");
    let dotted_release = group_path.join("repo.git.releases/v-dotted/META");
    assert!(plain_work.is_file());
    assert!(dotted_work.is_file());
    assert!(plain_release.is_file());
    assert!(dotted_release.is_file());
    assert!(fs::read(&plain_work).expect("plain work").windows(5).any(|part| part == b"plain"));
    assert!(fs::read(&dotted_work).expect("dotted work").windows(6).any(|part| part == b"dotted"));
    assert!(fs::read_to_string(&plain_release).expect("plain release").contains("v-plain"));
    assert!(fs::read_to_string(&dotted_release).expect("dotted release").contains("v-dotted"));
    assert_eq!(fs::read_to_string(&repo_permissions).expect("neighbor permission file"), "read:all\nwrite:all\ninteract:all\nrelease:all\nadmin:all\n");
    assert_eq!(fs::read_to_string(&dotted_permissions).expect("dotted permission file"), "read:none\nwrite:none\ninteract:none\nrelease:none\nadmin:all\n");

    let mut reloaded = ReticulumGitNode::default();
    assert_eq!(reloaded.load_repository_group("group", &group_path).expect("reload repositories"), 2);
    assert!(reloaded.groups["group"].repositories.contains_key("repo"));
    assert!(reloaded.groups["group"].repositories.contains_key("repo.git"));
    let plain_doc = reloaded.work_load_document(&plain_work).expect("plain work after reload");
    let dotted_doc = reloaded.work_load_document(&dotted_work).expect("dotted work after reload");
    assert_eq!(ReticulumGitNode::work_meta_string(&plain_doc, "title"), "plain");
    assert_eq!(ReticulumGitNode::work_meta_string(&dotted_doc, "title"), "dotted");
    let plain_releases = reloaded.releases_list_data(&repo_path);
    let dotted_releases = reloaded.releases_list_data(&dotted_repo_path);
    assert_eq!(plain_releases[0], ReticulumGitNode::RES_OK);
    assert_eq!(dotted_releases[0], ReticulumGitNode::RES_OK);
    assert!(plain_releases.windows(7).any(|part| part == b"v-plain"));
    assert!(dotted_releases.windows(8).any(|part| part == b"v-dotted"));
    assert_eq!(fs::read_to_string(&repo_permissions).expect("repo permission after reload"), "read:all\nwrite:all\ninteract:all\nrelease:all\nadmin:all\n");
    assert_eq!(fs::read_to_string(&dotted_permissions).expect("dotted permission after reload"), "read:none\nwrite:none\ninteract:none\nrelease:none\nadmin:all\n");
}

#[test]
fn dotted_repository_release_requests_isolate_colliding_sibling_companion() {
    let temp = tempfile::tempdir().expect("tempdir");
    let group_path = temp.path().join("group");
    let repository_path = group_path.join("repo.git");
    let sibling_releases = group_path.join("repo.releases");
    fs::create_dir_all(&group_path).expect("group");
    assert!(Command::new("git")
        .args(["init", "--bare", repository_path.to_string_lossy().as_ref()])
        .status()
        .expect("git init")
        .success());
    fs::write(group_path.join("repo.git.allowed"), "release:all\n").expect("release permission");
    fs::create_dir_all(sibling_releases.join("v-collision")).expect("sibling release");
    fs::write(sibling_releases.join("latest"), "v-collision\n").expect("sibling latest");
    fs::write(
        sibling_releases.join("v-collision/META"),
        "tag=v-collision\nstatus=published\ncreated=7\n",
    )
    .expect("sibling metadata");

    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group_path).expect("register repository");
    let remote = [37_u8; 16];
    let request = |operation: &str, target: Option<&str>| {
        let mut request = vec![
            (rmpv::Value::from(0_u64), rmpv::Value::String("group/repo.git".into())),
            (rmpv::Value::String("operation".into()), rmpv::Value::String(operation.into())),
        ];
        if let Some(target) = target {
            request.push((rmpv::Value::String("target".into()), rmpv::Value::String(target.into())));
        }
        request
    };

    assert_eq!(node.handle_release_request(&request("create", Some("v-dotted")), remote)[0], ReticulumGitNode::RES_OK);
    let canonical_releases = group_path.join("repo.git.releases");
    assert_eq!(fs::read_to_string(canonical_releases.join("latest")).expect("canonical latest"), "v-dotted");
    assert_eq!(fs::read_to_string(sibling_releases.join("latest")).expect("sibling latest unchanged"), "v-collision\n");
    assert!(!sibling_releases.join("v-dotted/META").exists());
    assert!(canonical_releases.join("v-dotted/META").is_file());

    let list = node.handle_release_request(&request("list", None), remote);
    assert_eq!(list[0], ReticulumGitNode::RES_OK);
    let payload = rmpv::decode::read_value(&mut std::io::Cursor::new(&list[1..])).expect("decode release list");
    fn field<'a>(value: &'a rmpv::Value, name: &str) -> Option<&'a rmpv::Value> {
        value.as_map()?.iter().find_map(|(key, value)| (key.as_str() == Some(name)).then_some(value))
    }
    let releases = field(&payload, "releases").and_then(rmpv::Value::as_array).expect("release array");
    assert!(releases.iter().any(|release| field(release, "tag").and_then(rmpv::Value::as_str) == Some("v-dotted")));
    assert!(!releases.iter().any(|release| field(release, "tag").and_then(rmpv::Value::as_str) == Some("v-collision")));
    assert_eq!(field(&payload, "latest").and_then(rmpv::Value::as_str), Some("v-dotted"));

    let view = node.handle_release_request(&request("view", Some("v-dotted")), remote);
    assert_eq!(view[0], ReticulumGitNode::RES_OK);
    assert!(view[1..].windows(8).any(|bytes| bytes == b"v-dotted"));
    assert_eq!(
        node.handle_release_request(&request("view", Some("v-collision")), remote)[0],
        ReticulumGitNode::RES_NOT_FOUND
    );
    let latest = node.handle_release_request(&request("latest", None), remote);
    assert_eq!(latest[0], ReticulumGitNode::RES_OK);
    let latest = rmpv::decode::read_value(&mut std::io::Cursor::new(&latest[1..])).expect("decode latest");
    assert_eq!(latest.as_str(), Some("v-dotted"));
    assert_eq!(fs::read_to_string(sibling_releases.join("latest")).expect("sibling latest preserved"), "v-collision\n");
}

use super::{decode_page_request, page_paths};
use rns_transport::destination::link::Link;
use std::io;
use std::path::Path;

const PYTHON_NULL_IDENTITY_HASH: [u8; 16] = [
    0xd7, 0xdb, 0x22, 0xf6, 0x3b, 0x45, 0x3c, 0x23, 0xbb, 0x06, 0x88, 0xdd, 0xe5, 0x65, 0xb7, 0xc1,
];

fn run_git(directory: &Path, args: &[&str]) {
    assert!(
        std::process::Command::new("git")
            .args(args)
            .current_dir(directory)
            .status()
            .expect("git command")
            .success(),
        "git {:?} failed",
        args
    );
}

fn page_fixture() -> (tempfile::TempDir, ReticulumGitNode) {
    let temporary = tempfile::tempdir().expect("fixture tempdir");
    let root = temporary.path().join("root");
    let group = root.join("group");
    let repository = group.join("repo");
    let source = temporary.path().join("source");
    std::fs::create_dir_all(&group).expect("group directory");
    std::fs::create_dir_all(&source).expect("source directory");

    run_git(&source, &["init", "-q"]);
    run_git(&source, &["config", "user.email", "rngit@example.invalid"]);
    run_git(&source, &["config", "user.name", "rngit-test"]);
    run_git(&source, &["checkout", "-qb", "main"]);
    std::fs::write(source.join("README.md"), "# page fixture\n").expect("README");
    std::fs::write(source.join("image.png"), b"\x89PNG\r\n\x1a\n\x00\x01").expect("image");
    run_git(&source, &["add", "README.md", "image.png"]);
    run_git(&source, &["commit", "-qm", "initial"]);

    run_git(&group, &["init", "--bare", "-q", "repo"]);
    run_git(&repository, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    run_git(&source, &["remote", "add", "origin", repository.to_string_lossy().as_ref()]);
    run_git(&source, &["push", "-q", "origin", "main"]);

    let mut node = ReticulumGitNode::default();
    assert_eq!(node.load_repository_root(&root).expect("load page fixture"), 1);
    let group_state = node.groups.get_mut("group").expect("loaded group");
    group_state.permissions.read.add(PermissionTarget::All);
    group_state.permissions.stats.add(PermissionTarget::All);
    group_state.permissions.release.add(PermissionTarget::All);
    (temporary, node)
}

fn request_map(entries: &[(&str, rmpv::Value)]) -> rmpv::Value {
    rmpv::Value::Map(
        entries
            .iter()
            .map(|(key, value)| (rmpv::Value::String((*key).into()), value.clone()))
            .collect(),
    )
}

#[test]
fn nomadnet_request_decode_and_handler_registration_match_link_contract() {
    let (_temporary, node) = page_fixture();
    let request = Link::request_payload(
        "/page/index.mu",
        request_map(&[("var_g", rmpv::Value::String("group".into()))]),
    )
    .expect("request payload");
    let decoded = decode_page_request(&request.packed)
        .expect("decode request")
        .expect("known page path");
    assert_eq!(decoded.path, "/page/index.mu");
    assert!(decoded.requested_at > 0.0);
    assert!(decoded.data.is_map());
    assert!(decode_page_request(&[0xc0]).expect("valid nil").is_none());

    let handlers = node.register_request_handlers();
    for path in page_paths() {
        assert!(handlers.contains(path), "missing registered path {path}");
    }
}

#[test]
fn pages_accept_nomadnet_var_fields_and_render_not_found_errors() {
    let (_temporary, mut node) = page_fixture();
    let remote = [7_u8; 16];
    let link = [8_u8; 16];
    let response = node
        .handle_page_request(
            "/page/repo.mu",
            &request_map(&[
                ("var_g", rmpv::Value::String("group".into())),
                ("var_r", rmpv::Value::String("repo".into())),
                ("var_ref", rmpv::Value::String("HEAD".into())),
            ]),
            remote,
            link,
        )
        .expect("repository page response");
    assert!(String::from_utf8_lossy(&response.data).contains("Repository"));

    let response = node
        .handle_page_request(
            "/page/blob.mu",
            &request_map(&[
                ("var_g", rmpv::Value::String("group".into())),
                ("var_r", rmpv::Value::String("repo".into())),
                ("var_ref", rmpv::Value::String("HEAD".into())),
                ("var_path", rmpv::Value::String("README.md".into())),
            ]),
            remote,
            link,
        )
        .expect("blob page response");
    assert!(String::from_utf8_lossy(&response.data).contains("page fixture"));

    let response = node
        .handle_page_request(
            "/page/blob.mu",
            &request_map(&[
                ("var_g", rmpv::Value::String("group".into())),
                ("var_r", rmpv::Value::String("repo".into())),
                ("var_ref", rmpv::Value::String("HEAD".into())),
                ("var_path", rmpv::Value::String("image.png".into())),
            ]),
            remote,
            link,
        )
        .expect("image blob page response");
    assert!(String::from_utf8_lossy(&response.data).contains("/media/group/repo/HEAD/image.png"));

    let response = node
        .handle_page_request(
            "/page/repo.mu",
            &request_map(&[
                ("var_g", rmpv::Value::String("missing".into())),
                ("var_r", rmpv::Value::String("repo".into())),
            ]),
            remote,
            link,
        )
        .expect("not-found page response");
    assert!(String::from_utf8_lossy(&response.data).contains("Not Found"));
}

#[test]
fn media_and_file_endpoints_enforce_keys_refs_permissions_and_metadata() {
    let (_temporary, mut node) = page_fixture();
    node.media_conversion = false;
    let remote = [7_u8; 16];
    let link = [8_u8; 16];
    let media_request = request_map(&[
        ("key", rmpv::Value::Binary(vec![1, 2, 3])),
        (
            "path",
            rmpv::Value::String("/media/group/repo/HEAD/image.png".into()),
        ),
    ]);
    let response = node
        .handle_page_request("/media", &media_request, remote, link)
        .expect("media response");
    assert_eq!(response.data, b"\x89PNG\r\n\x1a\n\x00\x01");
    let metadata = rmpv::decode::read_value(&mut std::io::Cursor::new(response.metadata.unwrap()))
        .expect("media metadata");
    assert_eq!(
        metadata
            .as_map()
            .and_then(|map| super::map_value(map, &rmpv::Value::String("name".into())))
            .and_then(rmpv::Value::as_slice),
        Some(&b"image.png"[..])
    );

    let download = node
        .handle_page_request(
            "/file/download",
            &request_map(&[
                ("var_g", rmpv::Value::String("group".into())),
                ("var_r", rmpv::Value::String("repo".into())),
                ("var_ref", rmpv::Value::String("HEAD".into())),
                ("var_path", rmpv::Value::String("README.md".into())),
            ]),
            remote,
            link,
        )
        .expect("download response");
    assert_eq!(download.data, b"# page fixture\n");

    assert!(node
        .handle_page_request(
            "/media",
            &request_map(&[(
                "path",
                rmpv::Value::String("/media/group/repo/does-not-exist/image.png".into())
            )]),
            remote,
            link,
        )
        .is_some_and(|response| {
            response.response_is_false && response.data.is_empty() && response.metadata.is_none()
        }));
    assert!(node
        .handle_page_request(
            "/media",
            &request_map(&[(
                "key",
                rmpv::Value::Binary(vec![1]),
            )]),
            remote,
            link,
        )
        .is_some_and(|response| {
            response.response_is_false && response.data.is_empty() && response.metadata.is_none()
        }));

    node.groups.get_mut("group").expect("group").permissions.read = Default::default();
    assert!(node
        .handle_page_request("/media", &media_request, remote, link)
        .is_some_and(|response| {
            response.response_is_false && response.data.is_empty() && response.metadata.is_none()
        }));
}

#[test]
fn media_conversion_failure_falls_back_to_raw_and_link_cleanup_removes_temp_files() {
    let (_temporary, mut node) = page_fixture();
    let remote = [7_u8; 16];
    let link = [8_u8; 16];
    node.page_link_connected(link);
    let directory = node.next_media_directory(link).expect("media directory");
    assert!(directory.is_dir());
    let cleanup = node.page_link_closed(link);
    assert_eq!(cleanup.removed_directories, 1);
    assert!(cleanup.failures.is_empty());
    assert!(!directory.exists());

    node.page_link_connected(link);
    let response = node
        .handle_page_request(
            "/media",
            &request_map(&[
                ("key", rmpv::Value::Binary(vec![1])),
                (
                    "path",
                    rmpv::Value::String("/media/group/repo/HEAD/image.png".into()),
                ),
            ]),
            remote,
            link,
        )
        .expect("raw fallback response");
    assert_eq!(response.data, b"\x89PNG\r\n\x1a\n\x00\x01");
    assert!(node.active_page_links.get(&link).is_some_and(|paths| paths.is_empty()));
}

#[test]
fn page_link_cleanup_retries_failed_removal() {
    let (_temporary, mut node) = page_fixture();
    let link = [9_u8; 16];
    let directory = node.next_media_directory(link).expect("media directory");

    let first_attempt = ReticulumGitNode::remove_tracked_page_media_directory(
        &mut node.active_page_links,
        link,
        &directory,
        |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected removal failure")),
    );

    assert!(first_attempt.is_err());
    assert!(directory.is_dir());
    assert!(node
        .active_page_links
        .get(&link)
        .is_some_and(|paths| paths.contains(&directory)));

    let retry = node.page_link_closed(link);
    assert_eq!(retry.removed_directories, 1);
    assert!(retry.failures.is_empty());
    assert!(!directory.exists());
    assert!(!node.active_page_links.contains_key(&link));
}

#[test]
fn stale_page_links_are_cleaned_while_active_links_keep_media() {
    let (temporary, mut node) = page_fixture();
    let base_link_id = rns_transport::hash::address_hash(
        temporary.path().to_string_lossy().as_bytes(),
    );
    let link_id = |discriminator: u8| {
        let mut link_id = base_link_id;
        link_id[15] ^= discriminator;
        link_id
    };
    let active = link_id(1);
    let stale = link_id(2);
    let closed = link_id(3);
    let missing = link_id(4);
    let active_directory = node.next_media_directory(active).expect("active media directory");
    let stale_directory = node.next_media_directory(stale).expect("stale media directory");
    let closed_directory = node.next_media_directory(closed).expect("closed media directory");
    let missing_directory = node.next_media_directory(missing).expect("missing media directory");

    let cleanup = super::rngit_network::clean_stale_page_links(
        &mut node,
        [
            (active, Some(LinkStatus::Active)),
            (stale, Some(LinkStatus::Stale)),
            (closed, Some(LinkStatus::Closed)),
            (missing, None),
        ],
    );

    assert_eq!(cleanup.removed_directories, 3);
    assert!(cleanup.failures.is_empty());
    assert!(active_directory.is_dir());
    assert!(!stale_directory.exists());
    assert!(!closed_directory.exists());
    assert!(!missing_directory.exists());
    assert!(node.active_page_links.contains_key(&active));
    assert!(!node.active_page_links.contains_key(&stale));
    let cleanup = node.page_link_closed(active);
    assert_eq!(cleanup.removed_directories, 1);
    assert!(cleanup.failures.is_empty());
    assert!(!active_directory.exists());
}

#[test]
fn blocked_anonymous_client_receives_no_identity_template_for_frozen_null_identity_hash() {
    let (_temporary, mut node) = page_fixture();
    node.blocked_identities.insert(PYTHON_NULL_IDENTITY_HASH);
    let response = node
        .handle_page_request(
            "/page/index.mu",
            &rmpv::Value::Map(Vec::new()),
            [0_u8; 16],
            [9_u8; 16],
        )
        .expect("no-identity response");
    let rendered = String::from_utf8_lossy(&response.data);
    assert!(rendered.contains("No Identity"));
    assert!(!rendered.contains("repo"), "repository content leaked: {rendered}");
    assert!(!rendered.contains("Python rngit interop"));
}

#[test]
fn unblocked_anonymous_client_receives_the_front_page() {
    let (_temporary, mut node) = page_fixture();
    let response = node
        .handle_page_request(
            "/page/index.mu",
            &rmpv::Value::Map(Vec::new()),
            [0_u8; 16],
            [9_u8; 16],
        )
        .expect("front page response");
    let rendered = String::from_utf8_lossy(&response.data);
    assert!(!rendered.contains("No Identity"));
    assert!(rendered.contains("Groups"));
}

#[test]
fn identified_blocked_client_does_not_receive_no_identity_template() {
    let (_temporary, mut node) = page_fixture();
    let blocked_identity = [0xa5_u8; 16];
    node.blocked_identities.insert(blocked_identity);
    let response = node
        .handle_page_request(
            "/page/index.mu",
            &rmpv::Value::Map(Vec::new()),
            blocked_identity,
            [9_u8; 16],
        )
        .expect("front page response");
    let rendered = String::from_utf8_lossy(&response.data);
    assert!(!rendered.contains("No Identity"));
    assert!(!rendered.contains("repo"), "blocked repository content leaked: {rendered}");
}

#[test]
fn custom_page_templates_replace_the_default_no_identity_page() {
    let (_temporary, mut node) = page_fixture();
    let templates = tempfile::tempdir().expect("templates directory");
    std::fs::write(templates.path().join("no_ident.mu"), "custom identity required")
        .expect("custom template");
    assert_eq!(node.load_page_templates(templates.path()).expect("load template"), 1);
    node.blocked_identities.insert(PYTHON_NULL_IDENTITY_HASH);
    let response = node
        .handle_page_request(
            "/page/index.mu",
            &rmpv::Value::Map(Vec::new()),
            [0_u8; 16],
            [9_u8; 16],
        )
        .expect("custom no-identity response");
    let rendered = String::from_utf8_lossy(&response.data);
    assert!(rendered.contains("custom identity required"));
    assert!(!rendered.contains("This page requires identification"));
}

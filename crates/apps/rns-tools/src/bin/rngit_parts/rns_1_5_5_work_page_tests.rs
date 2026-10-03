#[test]
fn rns_1_5_5_work_filter_links_count_only_readable_docs_with_roots() {
    let (_temporary, mut node) = page_fixture();
    let repository_path = node.groups["group"].repositories["repo"].path.clone();
    let work_root = super::companion_path(&repository_path, "work");
    for (scope, id, has_root) in [
        ("active", 1_u64, true),
        ("active", 2, false),
        ("completed", 3, true),
        ("proposed", 4, true),
    ] {
        let directory = work_root.join(scope).join(id.to_string());
        fs::create_dir_all(&directory).expect("create work directory");
        if has_root {
            fs::write(directory.join("root"), b"document").expect("write work root");
        }
    }
    fs::write(work_root.join("3.allowed"), "read:none\n").expect("deny document read");

    let response = node
        .handle_page_request(
            "/page/work.mu",
            &request_map(&[
                ("var_g", rmpv::Value::from("group")),
                ("var_r", rmpv::Value::from("repo")),
                ("var_scope", rmpv::Value::from("active")),
            ]),
            [7_u8; 16],
            [8_u8; 16],
        )
        .expect("work page");
    let text = String::from_utf8(response.data).expect("Micron text");
    for expected in [
        "Active`:/page/work.mu|g=group|r=repo|scope=active]`_ (1)",
        "Completed`:/page/work.mu|g=group|r=repo|scope=completed] (0)",
        "Proposed`:/page/work.mu|g=group|r=repo|scope=proposed] (1)",
        "All`:/page/work.mu|g=group|r=repo|scope=all] (2)",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text:?}");
    }
}

#[test]
fn rns_1_5_5_markdown_download_converts_only_for_active_link() {
    let (_temporary, mut node) = page_fixture();
    let remote = [7_u8; 16];
    let link = [8_u8; 16];
    let request = request_map(&[
        ("var_g", rmpv::Value::from("group")),
        ("var_r", rmpv::Value::from("repo")),
        ("var_ref", rmpv::Value::from("HEAD")),
        ("var_path", rmpv::Value::from("README.md")),
        ("var_fmt", rmpv::Value::from("mu")),
    ]);
    assert!(node.handle_page_request("/file/download", &request, remote, link).is_none());
    node.page_link_connected(link);
    let response = node
        .handle_page_request("/file/download", &request, remote, link)
        .expect("converted download");
    assert_eq!(response.data, b">page fixture");
    let metadata = rmpv::decode::read_value(&mut std::io::Cursor::new(
        response.metadata.expect("resource metadata"),
    ))
    .expect("decode resource metadata");
    assert_eq!(
        metadata
            .as_map()
            .and_then(|map| super::map_value(map, &rmpv::Value::from("name")))
            .and_then(rmpv::Value::as_slice),
        Some(&b"README.mu"[..]),
    );
    assert!(node.active_page_links.get(&link).is_some_and(|paths| paths.is_empty()));
    assert!(node.page_link_closed(link).failures.is_empty());
    assert!(!node.active_page_links.contains_key(&link));
}

#[test]
fn rns_1_5_5_markdown_conversion_rejects_invalid_format_or_non_markdown() {
    let (_temporary, mut node) = page_fixture();
    let remote = [7_u8; 16];
    let link = [8_u8; 16];
    node.page_link_connected(link);
    for (path, format) in [("README.md", "html"), ("README.md", "%GG"), ("image.png", "mu")] {
        let request = request_map(&[
            ("var_g", rmpv::Value::from("group")),
            ("var_r", rmpv::Value::from("repo")),
            ("var_path", rmpv::Value::from(path)),
            ("var_fmt", rmpv::Value::from(format)),
        ]);
        assert!(
            node.handle_page_request("/file/download", &request, remote, link).is_none(),
            "unexpected conversion for {path} as {format}"
        );
    }
}

#[test]
fn rns_1_5_5_markdown_conversion_rejects_malformed_utf8_without_a_download() {
    let (temporary, mut node) = page_fixture();
    let source = temporary.path().join("source");
    fs::write(source.join("BINARY.md"), b"# invalid \xff\n").expect("write malformed Markdown");
    run_git(&source, &["add", "BINARY.md"]);
    run_git(&source, &["commit", "-qm", "add malformed Markdown"]);
    run_git(&source, &["push", "-q", "origin", "main"]);

    let link = [8_u8; 16];
    node.page_link_connected(link);
    let request = request_map(&[
        ("var_g", rmpv::Value::from("group")),
        ("var_r", rmpv::Value::from("repo")),
        ("var_path", rmpv::Value::from("BINARY.md")),
        ("var_fmt", rmpv::Value::from("mu")),
    ]);
    assert!(
        node.handle_page_request("/file/download", &request, [7_u8; 16], link).is_none(),
        "malformed Markdown must not be presented as a successful conversion"
    );
    assert!(node.active_page_links.get(&link).is_some_and(|paths| paths.is_empty()));
}

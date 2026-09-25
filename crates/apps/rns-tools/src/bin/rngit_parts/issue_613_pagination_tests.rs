#[test]
fn commits_page_renders_reference_compatible_pagination() {
    let (temporary, mut node) = page_fixture();
    let source = temporary.path().join("source");
    for index in 0..100 {
        std::fs::write(source.join("README.md"), format!("commit {index}\n")).expect("update file");
        run_git(&source, &["add", "README.md"]);
        run_git(&source, &["commit", "-qm", &format!("commit {index}")]);
    }
    run_git(&source, &["push", "-q", "origin", "main"]);
    node.set_page_template("commits", "{PAGE_CONTENT}");
    node.set_page_template("base", "{PAGE_CONTENT}");
    let remote = [7; 16];
    let link = [8; 16];

    let first = node
        .handle_page_request(
            "/page/commits.mu",
            &request_map(&[("g", "group".into()), ("r", "repo".into())]),
            remote,
            link,
        )
        .expect("first commits page");
    let first = String::from_utf8(first.data).expect("UTF-8 page");
    assert!(first.contains("Page 1"));
    assert!(first.contains("Older »"));
    assert!(first.contains("`!`[Older »`:/page/commits.mu`g=group|r=repo|ref=HEAD|path=|page=1]`!"));
    assert!(first.contains("commit 99"), "first page: {first}");
    assert!(first.contains("\tcommit 0\n"));

    let second = node
        .handle_page_request(
            "/page/commits.mu",
            &request_map(&[("g", "group".into()), ("r", "repo".into()), ("page", "1".into())]),
            remote,
            link,
        )
        .expect("second commits page");
    let second = String::from_utf8(second.data).expect("UTF-8 page");
    assert!(second.contains("« Newer"));
    assert!(second.contains("`!`[« Newer`:/page/commits.mu`g=group|r=repo|ref=HEAD|path=|page=0]`!"));
    assert!(second.contains("Page 2"));
    assert!(second.contains("\tinitial\n"));
    assert!(!second.contains("Older »"));

    std::fs::write(source.join("only-this-file.txt"), "path-scoped history\n").expect("path file");
    run_git(&source, &["add", "only-this-file.txt"]);
    run_git(&source, &["commit", "-qm", "path-only commit"]);
    run_git(&source, &["push", "-q", "origin", "main"]);
    let path_history = node
        .handle_page_request(
            "/page/commits.mu",
            &request_map(&[
                ("g", "group".into()),
                ("r", "repo".into()),
                ("path", "only-this-file.txt".into()),
            ]),
            remote,
            link,
        )
        .expect("path-scoped commits page");
    let path_history = String::from_utf8(path_history.data).expect("UTF-8 page");
    assert!(path_history.contains("path-only commit"));
    assert!(!path_history.contains("commit 99"));
}

#[test]
fn tree_page_sorts_and_renders_reference_compatible_pagination() {
    let (temporary, mut node) = page_fixture();
    let source = temporary.path().join("source");
    for index in 0..1000 {
        std::fs::write(source.join(format!("entry-{index:04}")), b"entry").expect("tree entry");
    }
    std::fs::write(source.join("assets/nested [image].png"), b"nested").expect("special tree name");
    run_git(&source, &["add", "."]);
    run_git(&source, &["commit", "-qm", "large tree"]);
    run_git(&source, &["push", "-q", "origin", "main"]);
    node.set_page_template("tree", "{PAGE_CONTENT}");
    node.set_page_template("base", "{PAGE_CONTENT}");
    let remote = [7; 16];
    let link = [8; 16];

    let first = node
        .handle_page_request(
            "/page/tree.mu",
            &request_map(&[("g", "group".into()), ("r", "repo".into())]),
            remote,
            link,
        )
        .expect("first tree page");
    let first = String::from_utf8(first.data).expect("UTF-8 page");
    assert!(first.contains("Showing 1-1000 of 1003 entries"));
    assert!(first.contains("entry-0000"));
    assert!(!first.contains("entry-0999"));
    let directory_link = "`[assets/`:/page/tree.mu`g=group|r=repo|ref=HEAD|path=assets]";
    let file_link = "`[entry-0000`:/page/blob.mu`g=group|r=repo|ref=HEAD|path=entry-0000]";
    assert!(first.contains(directory_link));
    assert!(first.contains(file_link));
    assert!(first.find(directory_link).expect("directory link") < first.find(file_link).expect("file link"));
    assert!(first.contains("`!`[Next »`:/page/tree.mu`g=group|r=repo|ref=HEAD|path=|page=1]`!"));

    let second = node
        .handle_page_request(
            "/page/tree.mu",
            &request_map(&[("g", "group".into()), ("r", "repo".into()), ("page", "1".into())]),
            remote,
            link,
        )
        .expect("second tree page");
    let second = String::from_utf8(second.data).expect("UTF-8 page");
    assert!(second.contains("Showing 1001-1003 of 1003 entries"));
    assert!(second.contains("entry-0999"));
    assert!(second.contains("`!`[« Previous`:/page/tree.mu`g=group|r=repo|ref=HEAD|path=|page=0]`!"));

    let negative_page = node
        .handle_page_request(
            "/page/tree.mu",
            &request_map(&[("g", "group".into()), ("r", "repo".into()), ("page", "-7".into())]),
            remote,
            link,
        )
        .expect("negative page falls back to zero");
    let negative_page = String::from_utf8(negative_page.data).expect("UTF-8 page");
    assert!(negative_page.contains("entry-0000"));

    let malformed_page = node
        .handle_page_request(
            "/page/tree.mu",
            &request_map(&[
                ("g", "group".into()),
                ("r", "repo".into()),
                ("page", "not-a-number".into()),
            ]),
            remote,
            link,
        )
        .expect("malformed page falls back to zero");
    let malformed_page = String::from_utf8(malformed_page.data).expect("UTF-8 page");
    assert!(malformed_page.contains("entry-0000"));

    let oversized_page = node
        .handle_page_request(
            "/page/tree.mu",
            &request_map(&[
                ("g", "group".into()),
                ("r", "repo".into()),
                ("page", "99999999999999999999999999999999999999".into()),
            ]),
            remote,
            link,
        )
        .expect("oversized page is empty rather than wrapping to page zero");
    let oversized_page = String::from_utf8(oversized_page.data).expect("UTF-8 page");
    assert!(!oversized_page.contains("entry-0000"));
    assert!(oversized_page.contains("« Previous"));

    let nested = node
        .handle_page_request(
            "/page/tree.mu",
            &request_map(&[
                ("g", "group".into()),
                ("r", "repo".into()),
                ("path", "assets".into()),
            ]),
            remote,
            link,
        )
        .expect("nested tree page");
    let nested = String::from_utf8(nested.data).expect("UTF-8 page");
    assert!(nested.contains("`[nested image.png`:/page/blob.mu`g=group|r=repo|ref=HEAD|path=assets%2Fnested+image.png]"));
    assert!(nested.contains("`[nested image.png`:/page/blob.mu`g=group|r=repo|ref=HEAD|path=assets%2Fnested+%5Bimage%5D.png]"));
}

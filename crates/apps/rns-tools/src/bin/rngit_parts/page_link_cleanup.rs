pub(super) fn clean_stale_page_links(
    node: &mut ReticulumGitNode,
    links: impl IntoIterator<Item = ([u8; 16], Option<LinkStatus>)>,
) -> PageLinkCleanup {
    let stale = links
        .into_iter()
        .filter_map(|(link_id, status)| {
            status
                .is_none_or(|status| matches!(status, LinkStatus::Closed | LinkStatus::Stale))
                .then_some(link_id)
        })
        .collect::<Vec<_>>();
    node.clean_page_links(&stale)
}

fn log_page_link_cleanup_failures(cleanup: &PageLinkCleanup) {
    for failure in &cleanup.failures {
        log_page_media_cleanup_failure(
            "link cleanup",
            failure.link_id,
            &failure.directory,
            &failure.error,
        );
    }
}

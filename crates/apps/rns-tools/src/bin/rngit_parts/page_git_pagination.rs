fn parse_page_index(value: &str) -> usize {
    let value = value.trim();
    let (negative, digits) = if let Some(value) = value.strip_prefix('-') {
        (true, value)
    } else if let Some(value) = value.strip_prefix('+') {
        (false, value)
    } else {
        (false, value)
    };
    if negative || digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return 0;
    }
    digits.parse().unwrap_or(usize::MAX)
}

fn sanitize_page_link_label(value: &str) -> String {
    value.chars().filter(|character| !matches!(*character, '[' | ']' | '`')).collect()
}

impl ReticulumGitNode {
    fn serve_tree(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some(group) = page_param(map, "g") else {
            return self.not_found("", "Invalid tree request");
        };
        let Some(repository) = page_param(map, "r") else {
            return self.not_found("", "Invalid tree request");
        };
        let reference = page_param(map, "ref").unwrap_or_else(|| "HEAD".to_string());
        let tree_path = page_param(map, "path")
            .and_then(|value| percent_decode_plus(&value))
            .unwrap_or_default();
        let page = page_param(map, "page").map_or(0, |value| parse_page_index(&value));
        let Some(record) = self.accessible_repository(&remote, &group, &repository) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested repository was not found",
            );
        };
        let Some(resolved) = Self::resolve_page_ref(&record.path, &reference) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested reference was not found",
            );
        };
        let spec = if tree_path.is_empty() {
            resolved.clone()
        } else if Self::valid_page_path(&tree_path) {
            format!("{resolved}:{tree_path}")
        } else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "Invalid tree path",
            );
        };
        let Some(listing) = Self::page_git_text(
            &record.path,
            &["ls-tree".into(), "-z".into(), "--format=%(objecttype)%x09%(path)".into(), spec],
            256 * 1024,
        ) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested tree was not found",
            );
        };
        const TREE_ENTRIES_PER_PAGE: usize = 1000;
        let mut entries = listing
            .split('\0')
            .filter_map(|entry| {
                let (kind, name) = entry.split_once('\t')?;
                Some((kind, name))
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|(kind, name)| (!matches!(*kind, "tree" | "commit"), name.to_lowercase()));
        let total_entries = entries.len();
        let start = page.saturating_mul(TREE_ENTRIES_PER_PAGE).min(total_entries);
        let end = start.saturating_add(TREE_ENTRIES_PER_PAGE).min(total_entries);
        let mut content = String::from("> Tree\n\n");
        if total_entries > TREE_ENTRIES_PER_PAGE {
            let _ = writeln!(content, "`F666Showing {}-{} of {} entries`f\n", start + 1, end, total_entries);
        }
        for (kind, name) in &entries[start..end] {
            if *kind == "commit" {
                let _ = writeln!(content, "- ⧉ `{name}` (submodule)");
                continue;
            }
            let subpath = if tree_path.is_empty() {
                (*name).to_string()
            } else {
                format!("{tree_path}/{name}")
            };
            let display_name = sanitize_page_link_label(name);
            let (label, path) = if *kind == "tree" {
                (format!("{display_name}/"), "/page/tree.mu")
            } else {
                (display_name, "/page/blob.mu")
            };
            let link = format!(
                "`[{label}`:{path}`g={}|r={}|ref={}|path={}]",
                percent_encode_plus(&group),
                percent_encode_plus(&repository),
                percent_encode_plus(&reference),
                percent_encode_plus(&subpath),
            );
            let _ = writeln!(content, "- {link}");
        }
        if total_entries > TREE_ENTRIES_PER_PAGE {
            if page > 0 {
                let _ = write!(
                    content,
                    "`!`[« Previous`:/page/tree.mu`g={}|r={}|ref={}|path={}|page={}]`!",
                    percent_encode_plus(&group),
                    percent_encode_plus(&repository),
                    percent_encode_plus(&reference),
                    percent_encode_plus(&tree_path),
                    page - 1
                );
            }
            let pages = total_entries.div_ceil(TREE_ENTRIES_PER_PAGE);
            if page > 0 { content.push_str(" | "); }
            let _ = write!(content, "Page {} of {pages}", page.saturating_add(1));
            if end < total_entries {
                let _ = write!(
                    content,
                    " | `!`[Next »`:/page/tree.mu`g={}|r={}|ref={}|path={}|page={}]`!",
                    percent_encode_plus(&group),
                    percent_encode_plus(&repository),
                    percent_encode_plus(&reference),
                    percent_encode_plus(&tree_path),
                    page.saturating_add(1)
                );
            }
            content.push('\n');
        }
        self.view_succeeded(Some(&group), Some(&repository), false);
        self.page_render("tree", content, Some(&group), Some(&repository))
    }

    fn serve_commits(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some((group, repository, reference, encoded_path, record)) = self.page_repository(map, &remote) else {
            return self.not_found("", "The requested repository was not found");
        };
        let file_path = percent_decode_plus(&encoded_path).unwrap_or_default();
        if !file_path.is_empty() && !Self::valid_page_path(&file_path) {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "Invalid commit path",
            );
        }
        let Some(resolved) = Self::resolve_page_ref(&record.path, &reference) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested reference was not found",
            );
        };
        const COMMITS_PER_PAGE: usize = 100;
        let page = page_param(map, "page").map_or(0, |value| parse_page_index(&value));
        let skip = page.saturating_mul(COMMITS_PER_PAGE);
        let mut args = vec![
            "log".into(),
            format!("--skip={skip}"),
            format!("--max-count={COMMITS_PER_PAGE}"),
            "--format=%h%x09%s".into(),
            resolved,
        ];
        if !file_path.is_empty() {
            args.push("--".into());
            args.push(file_path.clone());
        }
        let log = Self::page_git_text(&record.path, &args, 256 * 1024).unwrap_or_default();
        self.view_succeeded(Some(&group), Some(&repository), false);
        let mut content = format!("> Commits\n\n{log}");
        let commit_count = log.lines().filter(|line| !line.is_empty()).count();
        if page > 0 || commit_count == COMMITS_PER_PAGE {
            if page > 0 {
                let _ = write!(
                    content,
                    "`!`[« Newer`:/page/commits.mu`g={}|r={}|ref={}|path={}|page={}]`! | ",
                    percent_encode_plus(&group),
                    percent_encode_plus(&repository),
                    percent_encode_plus(&reference),
                    percent_encode_plus(&file_path),
                    page - 1
                );
            }
            let _ = write!(content, "Page {}", page + 1);
            if commit_count == COMMITS_PER_PAGE {
                let _ = write!(
                    content,
                    " | `!`[Older »`:/page/commits.mu`g={}|r={}|ref={}|path={}|page={}]`!",
                    percent_encode_plus(&group),
                    percent_encode_plus(&repository),
                    percent_encode_plus(&reference),
                    percent_encode_plus(&file_path),
                    page.saturating_add(1)
                );
            }
            content.push('\n');
        }
        self.page_render("commits", content, Some(&group), Some(&repository))
    }
}

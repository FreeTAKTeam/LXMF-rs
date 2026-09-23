impl ReticulumGitNode {
    fn page_git_output(path: &Path, args: &[String], limit: usize) -> Option<Vec<u8>> {
        let output = Command::new("git").args(args).current_dir(path).output().ok()?;
        if !output.status.success() || output.stdout.len() > limit {
            return None;
        }
        Some(output.stdout)
    }

    fn page_git_text(path: &Path, args: &[String], limit: usize) -> Option<String> {
        let output = Self::page_git_output(path, args, limit)?;
        match String::from_utf8(output) {
            Ok(text) => Some(text),
            Err(error) => {
                eprintln!(
                    "rngit: git command output is not UTF-8 in {} for {args:?}: {error}",
                    path.display()
                );
                None
            }
        }
    }

    fn valid_page_ref(reference: &str) -> bool {
        if reference.is_empty()
            || reference.starts_with('-')
            || reference.contains('\0')
            || reference
                .chars()
                .any(|character| matches!(character, ' ' | '\n' | '\r' | '\t' | '\\' | '~' | '^' | ':' | '?' | '*' | '['))
            || reference.contains("..")
            || reference.contains("@{")
        {
            return false;
        }
        reference == "HEAD"
            || san_sha(reference).is_some()
            || reference.split('/').all(|component| {
                !component.is_empty()
                    && !component.starts_with('.')
                    && !component.ends_with('.')
                    && !component.ends_with(".lock")
                    && component != "@"
            })
    }

    fn resolve_page_ref(path: &Path, reference: &str) -> Option<String> {
        if !Self::valid_page_ref(reference) {
            return None;
        }
        let spec = format!("{reference}^{{commit}}");
        let output = Self::page_git_text(
            path,
            &["rev-parse".into(), "--verify".into(), spec],
            128,
        )?;
        let resolved = output.trim();
        san_sha(resolved).map(ToOwned::to_owned)
    }

    fn valid_page_path(file_path: &str) -> bool {
        !file_path.is_empty()
            && !file_path.starts_with('/')
            && !file_path.contains('\0')
            && !file_path.contains('\\')
            && file_path.split('/').all(|component| {
                !component.is_empty() && component != "." && component != ".."
            })
    }

    fn page_blob(path: &Path, resolved: &str, file_path: &str, limit: usize) -> Option<Vec<u8>> {
        if !Self::valid_page_path(file_path) {
            return None;
        }
        let spec = format!("{resolved}:{file_path}");
        Self::page_git_output(path, &["cat-file".into(), "blob".into(), spec], limit)
    }

    fn page_repository<'a>(
        &'a self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: &[u8; 16],
    ) -> Option<(String, String, String, String, &'a RepositoryRecord)> {
        let group = page_param(map, "g")?;
        let repository = page_param(map, "r")?;
        let reference = page_param(map, "ref").unwrap_or_else(|| "HEAD".to_string());
        let path = page_param(map, "path").unwrap_or_default();
        let record = self.accessible_repository(remote, &group, &repository)?;
        record.path.is_dir().then_some((group, repository, reference, path, record))
    }

    fn page_render(
        &self,
        template: &str,
        content: String,
        group: Option<&str>,
        repository: Option<&str>,
    ) -> PageResponse {
        page_response(self.render_template(template, &content, &self.navigation(group, repository)), None)
    }

    fn repository_description(path: &Path) -> String {
        let args = vec!["config".to_string(), "--get".to_string(), "repository.description".to_string()];
        Self::page_git_text(path, &args, 4096).unwrap_or_default().trim().to_string()
    }

    fn serve_front(&mut self, remote: [u8; 16]) -> PageResponse {
        let mut content = String::new();
        for (group_name, group) in &self.groups {
            let repositories = group
                .repositories
                .keys()
                .filter(|repository| {
                    self.resolve_permission(&remote, group_name, repository, Self::PERM_READ)
                })
                .count();
            if repositories > 0 {
                let _ = writeln!(
                    content,
                    "\u{0060}![{group_name}\u{0060}:/page/group.mu|g={group_name}]\u{0060} ({repositories} repositories)"
                );
            }
        }
        if content.is_empty() {
            content.push_str(">>\nNo groups available\n");
        }
        self.view_succeeded(None, None, false);
        self.page_render("front", content, None, None)
    }

    fn serve_group(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some(group_name) = page_param(map, "g") else {
            return self.not_found("", "Invalid group request");
        };
        let Some(group) = self.groups.get(&group_name) else {
            return self.not_found(
                &self.navigation(Some(&group_name), None),
                "The requested group was not found",
            );
        };
        let repositories = group
            .repositories
            .iter()
            .filter(|(name, _)| {
                self.resolve_permission(&remote, &group_name, name, Self::PERM_READ)
            })
            .collect::<Vec<_>>();
        if repositories.is_empty() {
            return self.not_found(
                &self.navigation(Some(&group_name), None),
                "The requested group was not found",
            );
        }
        let mut content = String::from("> Repositories\n\n");
        for (repository_name, repository) in repositories {
            let description = Self::repository_description(&repository.path);
            let suffix = if description.is_empty() {
                String::new()
            } else {
                format!(" - {description}")
            };
            let _ = writeln!(
                content,
                "\u{0060}![{repository_name}\u{0060}:/page/repo.mu|g={group_name}|r={repository_name}]\u{0060}{suffix}"
            );
        }
        self.view_succeeded(Some(&group_name), None, false);
        self.page_render("group", content, Some(&group_name), None)
    }

    fn serve_repo(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some(group) = page_param(map, "g") else {
            return self.not_found("", "Invalid repository request");
        };
        let Some(repository) = page_param(map, "r") else {
            return self.not_found(
                &self.navigation(Some(&group), None),
                "Invalid repository request",
            );
        };
        let reference = page_param(map, "ref").unwrap_or_else(|| "HEAD".to_string());
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
        let description = Self::repository_description(&record.path);
        let refs = Self::page_git_text(
            &record.path,
            &[
                "for-each-ref".into(),
                "--format=%(refname:short)".into(),
                "refs/heads".into(),
                "refs/tags".into(),
            ],
            64 * 1024,
        )
        .unwrap_or_default();
        let branch_count = refs.lines().filter(|line| !line.is_empty()).count();
        let mut content = String::new();
        if !description.is_empty() {
            let _ = writeln!(content, "{description}\n");
        }
        let _ = writeln!(
            content,
            "> Repository\n\nReference: \u{0060}{reference}\u{0060}\nCommit: \u{0060}{resolved}\u{0060}\nRefs: {branch_count}\n"
        );
        let _ = writeln!(
            content,
            "\u{0060}![Browse tree\u{0060}:/page/tree.mu|g={group}|r={repository}|ref={reference}]\u{0060}"
        );
        let _ = writeln!(
            content,
            "\u{0060}![Commits\u{0060}:/page/commits.mu|g={group}|r={repository}|ref={reference}]\u{0060}"
        );
        self.view_succeeded(Some(&group), Some(&repository), false);
        self.page_render("repo", content, Some(&group), Some(&repository))
    }

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
            &["ls-tree".into(), "-z".into(), "--name-only".into(), spec],
            256 * 1024,
        ) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested tree was not found",
            );
        };
        let mut content = String::from("> Tree\n\n");
        for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
            let _ = writeln!(content, "- `{entry}`");
        }
        self.view_succeeded(Some(&group), Some(&repository), false);
        self.page_render("tree", content, Some(&group), Some(&repository))
    }

    fn serve_blob(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        _link_id: [u8; 16],
    ) -> PageResponse {
        let Some(group) = page_param(map, "g") else {
            return self.not_found("", "Invalid blob request");
        };
        let Some(repository) = page_param(map, "r") else {
            return self.not_found("", "Invalid blob request");
        };
        let reference = page_param(map, "ref").unwrap_or_else(|| "HEAD".to_string());
        let file_path = page_param(map, "path")
            .and_then(|value| percent_decode_plus(&value))
            .unwrap_or_default();
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
        let Some(blob) = Self::page_blob(&record.path, &resolved, &file_path, 256 * 1024) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested file was not found or is too large",
            );
        };
        let mut content = format!("> {file_path}\n\n");
        match String::from_utf8(blob) {
            Ok(text) => content.push_str(&text),
            Err(_) => {
                let extension = Path::new(&file_path)
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| value.to_ascii_lowercase());
                if extension.as_deref().is_some_and(|value| {
                    matches!(value, "webp" | "png" | "jpg" | "jpeg" | "gif" | "tiff" | "tif" | "bmp")
                }) {
                    let media_path = format!(
                        "/media/{}/{}/{}/{}",
                        percent_encode_plus(&group),
                        percent_encode_plus(&repository),
                        percent_encode_plus(&reference),
                        percent_encode_plus(&file_path)
                    );
                    content.push_str(&format!("`(Image file`w=n`a=c`:{media_path})\n"));
                } else {
                    content.push_str(
                        "The requested blob is binary. Use the download or media endpoint.\n",
                    );
                }
            }
        }
        self.view_succeeded(Some(&group), Some(&repository), false);
        self.page_render("blob", content, Some(&group), Some(&repository))
    }

    fn serve_commits(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some((group, repository, reference, _, record)) = self.page_repository(map, &remote) else {
            return self.not_found("", "The requested repository was not found");
        };
        let Some(resolved) = Self::resolve_page_ref(&record.path, &reference) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested reference was not found",
            );
        };
        let log = Self::page_git_text(
            &record.path,
            &[
                "log".into(),
                "-n".into(),
                "100".into(),
                "--format=%h%x09%s".into(),
                resolved,
            ],
            256 * 1024,
        )
        .unwrap_or_default();
        self.view_succeeded(Some(&group), Some(&repository), false);
        self.page_render(
            "commits",
            format!("> Commits\n\n{log}"),
            Some(&group),
            Some(&repository),
        )
    }

    fn serve_commit(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some((group, repository, reference, _, record)) = self.page_repository(map, &remote) else {
            return self.not_found("", "The requested repository was not found");
        };
        let Some(resolved) = Self::resolve_page_ref(&record.path, &reference) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested commit was not found",
            );
        };
        let output = Self::page_git_text(
            &record.path,
            &["show".into(), "--stat".into(), "--oneline".into(), resolved],
            256 * 1024,
        )
        .unwrap_or_else(|| "The requested commit could not be loaded.".to_string());
        self.view_succeeded(Some(&group), Some(&repository), false);
        self.page_render(
            "commit",
            format!("> Commit\n\n{output}"),
            Some(&group),
            Some(&repository),
        )
    }

    fn serve_refs(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some((group, repository, _, _, record)) = self.page_repository(map, &remote) else {
            return self.not_found("", "The requested repository was not found");
        };
        let output = Self::page_git_text(
            &record.path,
            &[
                "for-each-ref".into(),
                "--format=%(refname) %(objectname)".into(),
                "refs/heads".into(),
                "refs/tags".into(),
            ],
            256 * 1024,
        )
        .unwrap_or_default();
        self.page_render(
            "refs",
            format!("> Refs\n\n{output}"),
            Some(&group),
            Some(&repository),
        )
    }

    fn serve_stats(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some((group, repository, _, _, _)) = self.page_repository(map, &remote) else {
            return self.not_found("", "The requested repository was not found");
        };
        if !self.resolve_permission(&remote, &group, &repository, Self::PERM_STATS) {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested repository was not found",
            );
        }
        let days = page_param(map, "days")
            .and_then(|value| value.parse().ok())
            .unwrap_or(30);
        let content = self
            .repository_stats(&remote, &group, &repository, days)
            .map(|value| format!("> Statistics\n\n{value:?}\n"))
            .unwrap_or_else(|| "> Statistics\n\nNo statistics available.\n".to_string());
        self.page_render("stats", content, Some(&group), Some(&repository))
    }

}

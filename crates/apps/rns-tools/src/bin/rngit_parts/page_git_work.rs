impl ReticulumGitNode {
    fn visible_work_count(
        &self,
        root: &Path,
        scope: &str,
        remote: &[u8; 16],
        group: &str,
        repository: &str,
    ) -> usize {
        let directory = root.join(scope);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return 0,
            Err(error) => {
                eprintln!("rngit: cannot list work scope {}: {error}", directory.display());
                return 0;
            }
        };
        let mut count = 0;
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    eprintln!("rngit: cannot read work entry in {}: {error}", directory.display());
                    continue;
                }
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(id) = entry.file_name().to_str().and_then(|name| name.parse::<u64>().ok())
            else {
                continue;
            };
            if path.join("root").is_file()
                && self.resolve_doc_permission(remote, group, repository, id, Self::PERM_READ)
            {
                count += 1;
            }
        }
        count
    }

    fn serve_releases(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some((group, repository, _, _, record)) = self.page_repository(map, &remote) else {
            return self.not_found("", "The requested repository was not found");
        };
        let releases = companion_path(&record.path, "releases");
        let latest = fs::read_to_string(releases.join("latest")).unwrap_or_default();
        let mut content = format!("> Releases\n\nLatest: {}\n", latest.trim());
        if let Ok(entries) = fs::read_dir(releases) {
            for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
                if let Some(name) = entry.file_name().to_str() {
                    let published = self
                        .release_data(&entry.path(), name)
                        .and_then(|value| value.as_map().cloned())
                        .and_then(|values| {
                            map_value(&values, &rmpv::Value::String("status".into()))
                                .and_then(rmpv::Value::as_str)
                                .map(|status| status == "published")
                        })
                        .unwrap_or(false);
                    if published {
                        let _ = writeln!(content, "- {name}");
                    }
                }
            }
        }
        self.page_render("releases", content, Some(&group), Some(&repository))
    }

    fn serve_release(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some((group, repository, default_reference, _, record)) = self.page_repository(map, &remote) else {
            return self.not_found("", "The requested repository was not found");
        };
        let reference = page_param(map, "t")
            .or_else(|| page_param(map, "ref"))
            .unwrap_or(default_reference);
        let Some(path) = Self::release_path(record, &reference) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "Invalid release tag",
            );
        };
        let Some(value) = self.release_data(&path, &reference) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested release was not found",
            );
        };
        let published = value
            .as_map()
            .and_then(|values| map_value(values, &rmpv::Value::String("status".into())))
            .and_then(rmpv::Value::as_str)
            .is_some_and(|status| status == "published");
        if !published {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested release was not found",
            );
        }
        self.page_render(
            "release",
            format!("> Release\n\n{value:?}\n"),
            Some(&group),
            Some(&repository),
        )
    }

    fn serve_work(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some((group, repository, _, _, record)) = self.page_repository(map, &remote) else {
            return self.not_found("", "The requested repository was not found");
        };
        let root = companion_path(&record.path, "work");
        let scope = page_param(map, "scope").unwrap_or_else(|| "active".to_string());
        let scope = if matches!(scope.as_str(), "active" | "completed" | "proposed" | "all") {
            scope.as_str()
        } else {
            "active"
        };
        let scopes = ["active", "completed", "proposed"];
        let counts = scopes.map(|item| self.visible_work_count(&root, item, &remote, &group, &repository));
        let total = counts.iter().sum::<usize>();
        let mut content = String::from("> Work\n\n");
        for (index, (label, item, count)) in [
            ("Active", "active", counts[0]),
            ("Completed", "completed", counts[1]),
            ("Proposed", "proposed", counts[2]),
            ("All", "all", total),
        ]
        .into_iter()
        .enumerate()
        {
            if index > 0 {
                content.push_str(" • ");
            }
            let selected = if scope == item { "`_" } else { "" };
            let _ = write!(
                content,
                "{selected}`[{label}`:/page/work.mu|g={group}|r={repository}|scope={item}]{selected} ({count})"
            );
        }
        content.push_str("\n\n");
        for (item, count) in scopes.into_iter().zip(counts) {
            if scope == "all" || scope == item {
                let _ = writeln!(content, "{item}: {count}");
            }
        }
        self.page_render("work", content, Some(&group), Some(&repository))
    }

    fn serve_work_doc(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> PageResponse {
        let Some((group, repository, _, _, record)) = self.page_repository(map, &remote) else {
            return self.not_found("", "The requested repository was not found");
        };
        let root = companion_path(&record.path, "work");
        let mut request = map.to_vec();
        if map_value(&request, &rmpv::Value::String("doc_id".into())).is_none() {
            if let Some(id) = page_param(map, "id").and_then(|value| value.parse::<u64>().ok()) {
                request.push((rmpv::Value::String("doc_id".into()), rmpv::Value::from(id)));
            }
        }
        if map_value(&request, &rmpv::Value::String("scope".into())).is_none() {
            if let Some(scope) = page_param(map, "scope") {
                request.push((rmpv::Value::String("scope".into()), rmpv::Value::String(scope.into())));
            }
        }
        let Some((scope, id, directory, document)) = self.work_request_document(&root, &request) else {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested work document was not found",
            );
        };
        if !self.resolve_doc_permission(&remote, &group, &repository, id, Self::PERM_READ) {
            return self.not_found(
                &self.navigation(Some(&group), Some(&repository)),
                "The requested work document was not found",
            );
        }
        let title = Self::work_meta_string(&document, "title");
        let content = document
            .as_map()
            .and_then(|value| map_value(value, &rmpv::Value::String("content".into())))
            .and_then(rmpv::Value::as_str)
            .unwrap_or_default();
        self.page_render(
            "work_doc",
            format!(
                "> {title}\n\nStatus: {scope}\n\n{content}\n\nDocument directory: {}\n",
                directory.display()
            ),
            Some(&group),
            Some(&repository),
        )
    }
}

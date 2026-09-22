impl ReticulumGitNode {
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
        let mut content = String::from("> Work\n\n");
        for scope in ["active", "completed", "proposed"] {
            let count = fs::read_dir(root.join(scope))
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().to_str().and_then(|name| name.parse::<u64>().ok()))
                .filter(|id| self.resolve_doc_permission(&remote, &group, &repository, *id, Self::PERM_READ))
                .count();
            let _ = writeln!(content, "{scope}: {count}");
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

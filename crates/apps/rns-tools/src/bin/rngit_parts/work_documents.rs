impl ReticulumGitNode {
    fn work_list(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        group: &str,
        repository: &str,
    ) -> Vec<u8> {
        let requested_scope = map_value(request, &rmpv::Value::String("scope".into()));
        let scopes: Vec<&str> = match requested_scope.and_then(rmpv::Value::as_str) {
            None => {
                if map_value(request, &rmpv::Value::String("scope".into())).is_none() {
                    vec!["active"]
                } else {
                    Vec::new()
                }
            }
            Some("active") => vec!["active"],
            Some("all") => vec!["active", "completed", "proposed"],
            Some(scope @ ("completed" | "proposed")) => vec![scope],
            Some(_) => Vec::new(),
        };
        let mut result = BTreeMap::from([
            ("active", Vec::new()),
            ("completed", Vec::new()),
            ("proposed", Vec::new()),
        ]);
        for scope in scopes {
            let Some(documents) = result.get_mut(scope) else {
                continue;
            };
            let Ok(entries) = fs::read_dir(root.join(scope)) else {
                continue;
            };
            for entry in entries.flatten() {
                let Some(id) = entry
                    .file_name()
                    .to_str()
                    .and_then(|value| value.parse::<u64>().ok())
                else {
                    continue;
                };
                if !self.resolve_doc_permission(
                    &remote,
                    group,
                    repository,
                    id,
                    Self::PERM_READ,
                ) {
                    continue;
                }
                let document_dir = entry.path();
                let Some(document) = self.work_load_document(&document_dir.join("root")) else {
                    continue;
                };
                if document.as_map().is_none_or(|map| map.is_empty())
                    || !Self::work_metadata_shape_is_valid(&document)
                {
                    continue;
                }
                let created = Self::work_meta_value(&document, "created")
                    .unwrap_or_else(|| rmpv::Value::from(0_u64));
                let edited = Self::work_meta_value(&document, "edited")
                    .unwrap_or_else(|| rmpv::Value::from(0_u64));
                let comments = fs::read_dir(&document_dir)
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_str()
                            .is_some_and(|value| value.parse::<u64>().is_ok())
                    })
                    .count() as u64;
                documents.push(rmpv::Value::Map(vec![
                    (rmpv::Value::String("id".into()), rmpv::Value::from(id)),
                    (
                        rmpv::Value::String("title".into()),
                        Self::work_meta_value_or_default(
                            &document,
                            "title",
                            rmpv::Value::String("Untitled".into()),
                        ),
                    ),
                    (rmpv::Value::String("created".into()), created),
                    (rmpv::Value::String("edited".into()), edited),
                    (
                        rmpv::Value::String("author".into()),
                        rmpv::Value::String(Self::work_meta_string(&document, "author").into()),
                    ),
                    (
                        rmpv::Value::String("format".into()),
                        Self::work_meta_value_or_default(
                            &document,
                            "format",
                            rmpv::Value::String("markdown".into()),
                        ),
                    ),
                    (
                        rmpv::Value::String("comments".into()),
                        rmpv::Value::from(comments),
                    ),
                ]));
            }
            documents.sort_by(|left, right| {
                let timestamp = |value: &rmpv::Value| {
                    value
                        .as_map()
                        .and_then(|map| {
                            map_value(map, &rmpv::Value::String("created".into()))
                        })
                        .and_then(|value| {
                            value
                                .as_f64()
                                .or_else(|| value.as_u64().map(|value| value as f64))
                        })
                        .unwrap_or(0.0)
                };
                timestamp(right)
                    .partial_cmp(&timestamp(left))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        let payload = rmpv::Value::Map(
            result
                .into_iter()
                .map(|(scope, documents)| {
                    (
                        rmpv::Value::String(scope.into()),
                        rmpv::Value::Array(documents),
                    )
                })
                .collect(),
        );
        response(Self::RES_OK, "", Some(&payload))
    }

    fn work_view(&self, root: &Path, request: &[(rmpv::Value, rmpv::Value)]) -> Vec<u8> {
        let Some((scope, id, directory)) = Self::work_view_location(root, request) else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        let root_path = directory.join("root");
        if !root_path.is_file() {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        }
        let Some(document) = self.work_load_document(&root_path) else {
            return response(Self::RES_REMOTE_FAIL, "Error loading document", None);
        };
        if document.as_map().is_none_or(|map| map.is_empty()) {
            return response(Self::RES_REMOTE_FAIL, "Error loading document", None);
        }
        if !Self::work_metadata_shape_is_valid(&document) {
            return response(Self::RES_REMOTE_FAIL, "Remote error", None);
        }
        let payload = self.work_view_payload(&scope, id, &directory, &document);
        response(Self::RES_OK, "", Some(&payload))
    }

    fn work_create(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        proposed: bool,
        peer_identity: Option<Identity>,
    ) -> Vec<u8> {
        let title = map_string(request, &rmpv::Value::String("title".into()))
            .unwrap_or_default()
            .trim()
            .to_string();
        let content = map_string(request, &rmpv::Value::String("content".into()))
            .unwrap_or_default()
            .trim()
            .to_string();
        let format = Self::work_format(request);
        if title.is_empty() || content.is_empty() {
            return response(Self::RES_INVALID_REQ, "Title and content are required", None);
        }
        if title.len() + content.len() + format.len() > Self::WORK_DOC_LIMIT {
            return response(Self::RES_INVALID_REQ, "Content limit exceeded", None);
        }
        if let Err(error) = Self::validate_work_signature(request, peer_identity) {
            return response(Self::RES_INVALID_REQ, error, None);
        }
        let scope = if proposed { "proposed" } else { "active" };
        let scope_root = root.join(scope);
        if let Err(error) = fs::create_dir_all(&scope_root) {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        let (id, directory) = loop {
            let id = self.work_get_next_id(root);
            let directory = scope_root.join(id.to_string());
            match fs::create_dir(&directory) {
                Ok(()) => break (id, directory),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
            }
        };
        let now = Self::work_now();
        let document = rmpv::Value::Map(vec![
            (
                rmpv::Value::String("content".into()),
                rmpv::Value::String(content.into()),
            ),
            (
                rmpv::Value::String("meta".into()),
                rmpv::Value::Map(vec![
                    (
                        rmpv::Value::String("format".into()),
                        rmpv::Value::String(format.into()),
                    ),
                    (
                        rmpv::Value::String("title".into()),
                        rmpv::Value::String(title.into()),
                    ),
                    (rmpv::Value::String("created".into()), now.clone()),
                    (rmpv::Value::String("edited".into()), now),
                    (
                        rmpv::Value::String("author".into()),
                        rmpv::Value::Binary(remote.to_vec()),
                    ),
                    (
                        rmpv::Value::String("signature".into()),
                        Self::work_optional_binary(request, "signature"),
                    ),
                    (
                        rmpv::Value::String("identity".into()),
                        Self::work_identity_value(request, peer_identity),
                    ),
                ]),
            ),
        ]);
        if let Err(error) = self.work_save_document(&directory.join("root"), &document) {
            let cleanup = fs::remove_dir_all(&directory);
            if let Err(cleanup_error) = cleanup {
                log::warn!(
                    "failed to roll back work document after save failure path={} error={} cleanup_error={}",
                    directory.display(),
                    error,
                    cleanup_error
                );
            }
            return response(Self::RES_REMOTE_FAIL, error, None);
        }
        if proposed {
            let owner = format!(
                "interact:{}\nwrite:{}\n",
                hex::encode(remote),
                hex::encode(remote)
            );
            if let Err(error) = Self::work_write_permissions(root, id, &owner) {
                if let Err(cleanup_error) = fs::remove_dir_all(&directory) {
                    log::warn!(
                        "failed to roll back proposed work document after permission failure path={} error={} cleanup_error={}",
                        directory.display(),
                        error,
                        cleanup_error
                    );
                }
                return response(Self::RES_REMOTE_FAIL, error, None);
            }
        }
        response(
            Self::RES_OK,
            "",
            Some(&rmpv::Value::Map(vec![
                (rmpv::Value::String("id".into()), rmpv::Value::from(id)),
                (
                    rmpv::Value::String("scope".into()),
                    rmpv::Value::String(scope.into()),
                ),
            ])),
        )
    }

    fn work_edit(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
        peer_identity: Option<Identity>,
    ) -> Vec<u8> {
        let Some((_, _, directory, mut document)) =
            self.work_request_document_ignoring_scope(root, request)
        else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        let title = map_string(request, &rmpv::Value::String("title".into()));
        let content = map_string(request, &rmpv::Value::String("content".into()));
        if title.as_deref().unwrap_or_default().trim().is_empty()
            && content.as_deref().unwrap_or_default().trim().is_empty()
        {
            return response(Self::RES_INVALID_REQ, "No changes specified", None);
        }
        if let Err(error) = Self::validate_work_signature(request, peer_identity) {
            return response(Self::RES_INVALID_REQ, error, None);
        }
        let rmpv::Value::Map(map) = &mut document else {
            return response(Self::RES_REMOTE_FAIL, "Malformed work document", None);
        };
        if let Some(content) = content {
            set_map_value(map, "content", rmpv::Value::String(content.trim().into()));
        }
        if let Some(meta) = map_value_mut(map, "meta").and_then(value_map_mut) {
            if let Some(title) = title {
                if !title.trim().is_empty() {
                    set_map_value(meta, "title", rmpv::Value::String(title.trim().into()));
                }
            }
            set_map_value(meta, "edited", Self::work_now());
            if map_value(request, &rmpv::Value::String("signature".into())).is_some() {
                set_map_value(
                    meta,
                    "signature",
                    Self::work_optional_binary(request, "signature"),
                );
            }
            if map_value(request, &rmpv::Value::String("identity".into())).is_some() {
                set_map_value(
                    meta,
                    "identity",
                    Self::work_optional_binary(request, "identity"),
                );
            } else if peer_identity.is_some() {
                set_map_value(meta, "identity", Self::work_identity_value(request, peer_identity));
            }
        }
        match self.work_save_document(&directory.join("root"), &document) {
            Ok(()) => vec![Self::RES_OK],
            Err(error) => response(Self::RES_REMOTE_FAIL, error, None),
        }
    }
}

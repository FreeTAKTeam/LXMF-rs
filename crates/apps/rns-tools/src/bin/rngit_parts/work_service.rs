impl ReticulumGitNode {
    const WORK_DOC_LIMIT: usize = 256 * 1024;

    fn work_root(record: &RepositoryRecord) -> PathBuf {
        record.path.with_extension("work")
    }

    pub fn work_get_next_id(&self, work_root: &Path) -> u64 {
        ["active", "completed", "proposed"]
            .into_iter()
            .flat_map(|scope| fs::read_dir(work_root.join(scope)).into_iter().flatten())
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str().and_then(|value| value.parse::<u64>().ok()))
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub fn work_get_next_comment_id(&self, document: &rmpv::Value) -> u64 {
        document
            .as_map()
            .and_then(|map| map_value(map, &rmpv::Value::String("comments".into())))
            .and_then(rmpv::Value::as_array)
            .and_then(|comments| {
                comments
                    .iter()
                    .filter_map(|comment| comment.as_map())
                    .filter_map(|comment| map_value(comment, &rmpv::Value::String("id".into())))
                    .filter_map(rmpv::Value::as_u64)
                    .max()
            })
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub fn work_load_document(&self, path: &Path) -> Option<rmpv::Value> {
        let bytes = fs::read(path).ok()?;
        if bytes.len() > Self::WORK_DOC_LIMIT {
            return None;
        }
        rmpv::decode::read_value(&mut std::io::Cursor::new(bytes)).ok()
    }

    pub fn work_save_document(&self, path: &Path, document: &rmpv::Value) -> Result<(), String> {
        let bytes = pack_value(document)?;
        if bytes.len() > Self::WORK_DOC_LIMIT {
            return Err("work document exceeds size limit".to_string());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(path, bytes).map_err(|error| error.to_string())
    }

    fn work_document_path(&self, root: &Path, scope: &str, id: u64) -> PathBuf {
        root.join(scope).join(id.to_string()).join("root")
    }

    fn work_document(&self, root: &Path, scope: &str, id: u64) -> Option<rmpv::Value> {
        self.work_load_document(&self.work_document_path(root, scope, id))
    }

    fn work_meta(document: &rmpv::Value) -> Option<&[(rmpv::Value, rmpv::Value)]> {
        document
            .as_map()
            .and_then(|map| map_value(map, &rmpv::Value::String("meta".into())))
            .and_then(rmpv::Value::as_map)
            .map(Vec::as_slice)
    }

    fn work_meta_string(document: &rmpv::Value, key: &str) -> String {
        Self::work_meta(document)
            .and_then(|map| map_value(map, &rmpv::Value::String(key.into())))
            .and_then(rmpv::Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    fn work_request_document(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
    ) -> Option<(String, u64, rmpv::Value)> {
        let id = map_value(request, &rmpv::Value::String("doc_id".into()))?.as_u64()?;
        let requested_scope = map_string(request, &rmpv::Value::String("scope".into()));
        let scopes: Vec<String> = requested_scope.map_or_else(
            || vec!["active".to_string(), "completed".to_string(), "proposed".to_string()],
            |scope| vec![scope],
        );
        scopes.into_iter().find_map(|scope| {
            self.work_document(root, &scope, id).map(|document| (scope, id, document))
        })
    }

    pub fn handle_work_request(
        &mut self,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> Vec<u8> {
        let (group, repository, record) = match self.repository_for_request(request) {
            Ok(value) => value,
            Err(error) => return error,
        };
        let operation = map_string(request, &rmpv::Value::String("operation".into())).unwrap_or_default();
        let required = match operation.as_str() {
            "list" | "view" => Self::PERM_READ,
            "propose" => Self::PERM_PROPOSE,
            "perms" => Self::PERM_ADMIN,
            "comment" => Self::PERM_INTERACT,
            "create" | "edit" | "delete" | "complete" | "activate" => Self::PERM_WRITE,
            _ => return response(Self::RES_INVALID_REQ, "Invalid request", None),
        };
        if !self.resolve_permission(&remote, &group, &repository, required) {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        if matches!(
            operation.as_str(),
            "view" | "edit" | "delete" | "comment" | "complete" | "activate" | "perms"
        ) {
            let Some(doc_id) = map_value(request, &rmpv::Value::String("doc_id".into()))
                .and_then(rmpv::Value::as_u64)
            else {
                return response(Self::RES_INVALID_REQ, "No document ID specified", None);
            };
            if !self.resolve_doc_permission(&remote, &group, &repository, doc_id, required) {
                return response(Self::RES_DISALLOWED, "Not allowed", None);
            }
        }
        let root = Self::work_root(record);
        match operation.as_str() {
            "list" => self.work_list(&root, request, remote, &group, &repository),
            "view" => self.work_view(&root, request),
            "create" | "propose" => self.work_create(&root, request, remote, operation == "propose"),
            "edit" => self.work_edit(&root, request),
            "delete" => self.work_delete(&root, request),
            "comment" => self.work_comment(&root, request, remote),
            "complete" => self.work_move(&root, request, "active", "completed"),
            "activate" => self.work_move(&root, request, "completed", "active"),
            "perms" => self.work_permissions(&root, request),
            _ => response(Self::RES_INVALID_REQ, "Invalid request", None),
        }
    }

    fn work_list(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        group: &str,
        repository: &str,
    ) -> Vec<u8> {
        let requested_scope = map_string(request, &rmpv::Value::String("scope".into()));
        let scopes: Vec<String> = requested_scope.map_or_else(
            || vec!["active".to_string(), "completed".to_string(), "proposed".to_string()],
            |scope| vec![scope],
        );
        let mut documents = Vec::new();
        for scope in scopes {
            if let Ok(entries) = fs::read_dir(root.join(&scope)) {
                for entry in entries.flatten() {
                    let Some(id) = entry.file_name().to_str().and_then(|value| value.parse::<u64>().ok()) else {
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
                    if let Some(document) = self.work_document(root, &scope, id) {
                        documents.push(rmpv::Value::Map(vec![
                            (rmpv::Value::String("id".into()), rmpv::Value::from(id)),
                            (rmpv::Value::String("scope".into()), rmpv::Value::String(scope.clone().into())),
                            (rmpv::Value::String("title".into()), rmpv::Value::String(Self::work_meta_string(&document, "title").into())),
                            (rmpv::Value::String("author".into()), rmpv::Value::String(Self::work_meta_string(&document, "author").into())),
                        ]));
                    }
                }
            }
        }
        response(Self::RES_OK, "", Some(&rmpv::Value::Array(documents)))
    }

    fn work_view(&self, root: &Path, request: &[(rmpv::Value, rmpv::Value)]) -> Vec<u8> {
        let Some((_scope, _id, document)) = self.work_request_document(root, request) else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        response(Self::RES_OK, "", Some(&document))
    }

    fn work_create(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        proposed: bool,
    ) -> Vec<u8> {
        let title = map_string(request, &rmpv::Value::String("title".into())).unwrap_or_default();
        let content = map_string(request, &rmpv::Value::String("content".into())).unwrap_or_default();
        if title.is_empty() || content.len() > Self::WORK_DOC_LIMIT {
            return response(Self::RES_INVALID_REQ, "Invalid work document", None);
        }
        let id = self.work_get_next_id(root);
        let scope = if proposed { "proposed" } else { "active" };
        let author = hex::encode(remote);
        let document = rmpv::Value::Map(vec![
            (rmpv::Value::String("id".into()), rmpv::Value::from(id)),
            (rmpv::Value::String("content".into()), rmpv::Value::String(content.into())),
            (rmpv::Value::String("comments".into()), rmpv::Value::Array(Vec::new())),
            (
                rmpv::Value::String("meta".into()),
                rmpv::Value::Map(vec![
                    (rmpv::Value::String("title".into()), rmpv::Value::String(title.into())),
                    (rmpv::Value::String("author".into()), rmpv::Value::String(author.into())),
                    (rmpv::Value::String("format".into()), rmpv::Value::String("markdown".into())),
                    (rmpv::Value::String("created".into()), rmpv::Value::from(unix_now())),
                    (rmpv::Value::String("edited".into()), rmpv::Value::from(unix_now())),
                ]),
            ),
        ]);
        if let Err(error) = self.work_save_document(&self.work_document_path(root, scope, id), &document) {
            return response(Self::RES_REMOTE_FAIL, error, None);
        }
        response(Self::RES_OK, "", Some(&rmpv::Value::Map(vec![
            (rmpv::Value::String("id".into()), rmpv::Value::from(id)),
            (rmpv::Value::String("scope".into()), rmpv::Value::String(scope.into())),
        ])))
    }

    fn work_edit(&self, root: &Path, request: &[(rmpv::Value, rmpv::Value)]) -> Vec<u8> {
        let Some((scope, id, mut document)) = self.work_request_document(root, request) else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        let Some(content) = map_string(request, &rmpv::Value::String("content".into())) else {
            return response(Self::RES_INVALID_REQ, "No content specified", None);
        };
        if let rmpv::Value::Map(map) = &mut document {
            if let Some((_, value)) = map.iter_mut().find(|(key, _)| key == &rmpv::Value::String("content".into())) {
                *value = rmpv::Value::String(content.into());
            }
        }
        match self.work_save_document(&self.work_document_path(root, &scope, id), &document) {
            Ok(()) => vec![Self::RES_OK],
            Err(error) => response(Self::RES_REMOTE_FAIL, error, None),
        }
    }

    fn work_delete(&self, root: &Path, request: &[(rmpv::Value, rmpv::Value)]) -> Vec<u8> {
        let Some((scope, id, _)) = self.work_request_document(root, request) else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        match fs::remove_dir_all(root.join(scope).join(id.to_string())) {
            Ok(()) => vec![Self::RES_OK],
            Err(error) => response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        }
    }

    fn work_comment(&self, root: &Path, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        let Some((scope, id, mut document)) = self.work_request_document(root, request) else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        let Some(content) = map_string(request, &rmpv::Value::String("content".into())) else {
            return response(Self::RES_INVALID_REQ, "No content specified", None);
        };
        let comment_id = self.work_get_next_comment_id(&document);
        if let rmpv::Value::Map(map) = &mut document {
            if let Some((_, rmpv::Value::Array(comments))) = map
                .iter_mut()
                .find(|(key, _)| key == &rmpv::Value::String("comments".into()))
            {
                comments.push(rmpv::Value::Map(vec![
                    (rmpv::Value::String("id".into()), rmpv::Value::from(comment_id)),
                    (rmpv::Value::String("author".into()), rmpv::Value::String(hex::encode(remote).into())),
                    (rmpv::Value::String("content".into()), rmpv::Value::String(content.into())),
                    (rmpv::Value::String("created".into()), rmpv::Value::from(unix_now())),
                ]));
            }
        }
        match self.work_save_document(&self.work_document_path(root, &scope, id), &document) {
            Ok(()) => response(Self::RES_OK, "", Some(&rmpv::Value::Map(vec![
                (rmpv::Value::String("id".into()), rmpv::Value::from(comment_id)),
            ]))),
            Err(error) => response(Self::RES_REMOTE_FAIL, error, None),
        }
    }

    fn work_move(&self, root: &Path, request: &[(rmpv::Value, rmpv::Value)], from: &str, to: &str) -> Vec<u8> {
        let Some((scope, id, _)) = self.work_request_document(root, request) else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        if scope != from {
            return response(Self::RES_INVALID_REQ, "Invalid document scope", None);
        }
        if let Err(error) = fs::create_dir_all(root.join(to)) {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        match fs::rename(root.join(from).join(id.to_string()), root.join(to).join(id.to_string())) {
            Ok(()) => response(Self::RES_OK, "", Some(&rmpv::Value::Map(vec![
                (rmpv::Value::String("id".into()), rmpv::Value::from(id)),
                (rmpv::Value::String("scope".into()), rmpv::Value::String(to.into())),
            ]))),
            Err(error) => response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        }
    }

    fn work_permissions(&self, root: &Path, request: &[(rmpv::Value, rmpv::Value)]) -> Vec<u8> {
        let Some(id) = map_value(request, &rmpv::Value::String("doc_id".into())).and_then(rmpv::Value::as_u64) else {
            return response(Self::RES_INVALID_REQ, "No document ID specified", None);
        };
        let content = fs::read_to_string(root.join(format!("{id}.allowed"))).unwrap_or_default();
        response(Self::RES_OK, "", Some(&rmpv::Value::Map(vec![
            (rmpv::Value::String("content".into()), rmpv::Value::String(content.into())),
        ])))
    }
}

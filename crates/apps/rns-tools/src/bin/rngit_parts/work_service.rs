impl ReticulumGitNode {
    fn valid_work_document_request(request: &[(rmpv::Value, rmpv::Value)]) -> bool {
        let Some(doc_id) = map_value(request, &rmpv::Value::String("doc_id".into())) else {
            return false;
        };
        if doc_id.as_u64().is_none()
            && doc_id
                .as_str()
                .and_then(|value| value.parse::<u64>().ok())
                .is_none()
        {
            return false;
        }
        let Some(scope) = map_value(request, &rmpv::Value::String("scope".into())) else {
            return true;
        };
        matches!(
            scope.as_str(),
            Some("active" | "completed" | "proposed" | "all")
        )
    }

    fn valid_work_list_scope(request: &[(rmpv::Value, rmpv::Value)]) -> bool {
        let Some(scope) = map_value(request, &rmpv::Value::String("scope".into())) else {
            return true;
        };
        matches!(
            scope.as_str(),
            Some("active" | "completed" | "proposed" | "all")
        )
    }

    fn work_permission_path(root: &Path, id: u64) -> PathBuf {
        root.join(format!("{id}.allowed"))
    }

    fn work_write_permissions(root: &Path, id: u64, content: &str) -> Result<(), String> {
        fs::create_dir_all(root).map_err(|error| error.to_string())?;
        let path = Self::work_permission_path(root, id);
        let parent = path
            .parent()
            .ok_or_else(|| "work permission path has no parent".to_string())?;
        let temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
        fs::write(temporary.path(), content).map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        fs::rename(temporary.path(), path).map_err(|error| error.to_string())
    }

    fn work_remove_permissions(root: &Path, id: u64) -> Result<(), String> {
        match fs::remove_file(Self::work_permission_path(root, id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn work_format(request: &[(rmpv::Value, rmpv::Value)]) -> String {
        match map_string(request, &rmpv::Value::String("format".into())).as_deref() {
            Some("micron") => "micron".to_string(),
            _ => "markdown".to_string(),
        }
    }

    fn work_optional_binary(
        request: &[(rmpv::Value, rmpv::Value)],
        key: &str,
    ) -> rmpv::Value {
        value_bytes(map_value(request, &rmpv::Value::String(key.into())))
            .map(rmpv::Value::Binary)
            .unwrap_or(rmpv::Value::Nil)
    }

    fn work_identity_value(
        request: &[(rmpv::Value, rmpv::Value)],
        peer_identity: Option<Identity>,
    ) -> rmpv::Value {
        if let Some(identity) = peer_identity {
            let mut public_key = Vec::with_capacity(rns_transport::identity::PUBLIC_KEY_LENGTH * 2);
            public_key.extend_from_slice(identity.public_key_bytes());
            public_key.extend_from_slice(identity.verifying_key_bytes());
            rmpv::Value::Binary(public_key)
        } else {
            Self::work_optional_binary(request, "identity")
        }
    }

    fn validate_work_signature(
        request: &[(rmpv::Value, rmpv::Value)],
        peer_identity: Option<Identity>,
    ) -> Result<(), &'static str> {
        let Some(peer_identity) = peer_identity else { return Ok(()) };
        let Some(signature) = value_bytes(map_value(
            request,
            &rmpv::Value::String("signature".into()),
        )) else {
            return Err("No signature provided");
        };
        if signature.len() != rns_transport::identity::PUBLIC_KEY_LENGTH * 2 {
            return Err("Invalid signature length");
        }
        let content = map_string(request, &rmpv::Value::String("content".into()))
            .unwrap_or_default();
        if !rns_transport::identity::verify(
            *peer_identity.verifying_key_bytes(),
            content.as_bytes(),
            &signature,
        ) {
            return Err("Invalid signature");
        }
        Ok(())
    }

    fn work_now() -> rmpv::Value {
        rmpv::Value::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_secs())
                .unwrap_or(0),
        )
    }

    pub fn handle_work_request(
        &mut self,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> Vec<u8> {
        self.handle_work_request_with_peer_identity(request, remote, None)
    }

    pub fn handle_work_request_with_peer_identity(
        &mut self,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        peer_identity: Option<Identity>,
    ) -> Vec<u8> {
        let (group, repository, record) = match self.repository_for_request(request) {
            Ok(value) => value,
            Err(error) => return error,
        };
        let Some(operation) = map_string(request, &rmpv::Value::String("operation".into()))
            .filter(|operation| !operation.is_empty())
        else {
            return response(Self::RES_INVALID_REQ, "Invalid request", None);
        };
        if !self.resolve_permission(&remote, &group, &repository, Self::PERM_READ) {
            return response(Self::RES_NOT_FOUND, "Not found", None);
        }
        let root = Self::work_root(record);

        match operation.as_str() {
            "list" => {
                if !Self::valid_work_list_scope(request) {
                    return response(Self::RES_INVALID_REQ, "Invalid scope", None);
                }
                self.work_list(&root, request, remote, &group, &repository)
            }
            "view" => {
                if !Self::valid_work_document_request(request) {
                    return response(Self::RES_INVALID_REQ, "Invalid document request", None);
                }
                let Some((_, id, _)) = Self::work_view_location(&root, request) else {
                    return response(Self::RES_NOT_FOUND, "Document not found", None);
                };
                if !self.resolve_doc_permission(
                    &remote,
                    &group,
                    &repository,
                    id,
                    Self::PERM_READ,
                ) {
                    return response(Self::RES_DISALLOWED, "Not allowed", None);
                }
                self.work_view(&root, request)
            }
            "comment" => {
                if !Self::valid_work_document_request(request) {
                    return response(Self::RES_INVALID_REQ, "Invalid document request", None);
                }
                let Some((_, id, _, _)) = self.work_request_document(&root, request) else {
                    return response(Self::RES_NOT_FOUND, "Document not found", None);
                };
                let can_read = self.resolve_doc_permission(
                    &remote,
                    &group,
                    &repository,
                    id,
                    Self::PERM_READ,
                );
                let can_write = self.resolve_doc_permission(
                    &remote,
                    &group,
                    &repository,
                    id,
                    Self::PERM_WRITE,
                );
                let can_interact = self.resolve_doc_permission(
                    &remote,
                    &group,
                    &repository,
                    id,
                    Self::PERM_INTERACT,
                );
                if !can_interact || !(can_read || can_write) {
                    return response(Self::RES_DISALLOWED, "Not allowed", None);
                }
                self.work_comment(&root, request, remote)
            }
            "propose" => {
                if !self.resolve_permission(&remote, &group, &repository, Self::PERM_PROPOSE) {
                    return response(Self::RES_DISALLOWED, "Not allowed", None);
                }
                self.work_create(&root, request, remote, true, peer_identity)
            }
            "create" => {
                if !self.resolve_permission(&remote, &group, &repository, Self::PERM_WRITE)
                    || !self.resolve_permission(
                        &remote,
                        &group,
                        &repository,
                        Self::PERM_INTERACT,
                    )
                {
                    return response(Self::RES_DISALLOWED, "Not allowed", None);
                }
                self.work_create(&root, request, remote, false, peer_identity)
            }
            "edit" => {
                if !Self::valid_work_document_request(request) {
                    return response(Self::RES_INVALID_REQ, "Invalid document request", None);
                }
                let Some((_, id, _, document)) = self.work_request_document(&root, request) else {
                    return response(Self::RES_NOT_FOUND, "Document not found", None);
                };
                if !Self::work_author_matches(&document, &remote)
                    || !self.work_manage_allowed(&remote, &group, &repository, id)
                {
                    return response(Self::RES_DISALLOWED, "Not allowed", None);
                }
                self.work_edit(&root, request, peer_identity)
            }
            "delete" => {
                if !Self::valid_work_document_request(request) {
                    return response(Self::RES_INVALID_REQ, "Invalid document request", None);
                }
                let Some((_, id, _, document)) =
                    self.work_request_document_for_delete(&root, request)
                else {
                    return response(Self::RES_REMOTE_FAIL, "Remote error", None);
                };
                let admin = self.resolve_doc_permission(
                    &remote,
                    &group,
                    &repository,
                    id,
                    Self::PERM_ADMIN,
                );
                if !self.work_manage_allowed(&remote, &group, &repository, id) {
                    return response(Self::RES_DISALLOWED, "Not allowed", None);
                }
                if !Self::work_author_matches(&document, &remote) && !admin {
                    return response(Self::RES_DISALLOWED, "No access, not author", None);
                }
                self.work_delete(&root, request)
            }
            "complete" => {
                self.work_transition(&root, request, remote, &group, &repository, false)
            }
            "activate" => {
                self.work_transition(&root, request, remote, &group, &repository, true)
            }
            "perms" => {
                if !Self::valid_work_document_request(request) {
                    return response(Self::RES_INVALID_REQ, "Invalid document request", None);
                }
                let Some((_, id, _, document)) = self.work_request_document(&root, request) else {
                    return response(Self::RES_NOT_FOUND, "Document not found", None);
                };
                let manage = self.work_manage_allowed(&remote, &group, &repository, id);
                let admin = self.resolve_doc_permission(
                    &remote,
                    &group,
                    &repository,
                    id,
                    Self::PERM_ADMIN,
                );
                if !(admin || (manage && Self::work_author_matches(&document, &remote))) {
                    return response(Self::RES_DISALLOWED, "Not allowed", None);
                }
                self.work_permissions(&root, request)
            }
            _ => response(Self::RES_INVALID_REQ, "Invalid request", None),
        }
    }

    fn work_manage_allowed(
        &self,
        remote: &[u8; 16],
        group: &str,
        repository: &str,
        id: u64,
    ) -> bool {
        self.resolve_doc_permission(remote, group, repository, id, Self::PERM_WRITE)
            && self.resolve_doc_permission(remote, group, repository, id, Self::PERM_INTERACT)
    }
}

impl ReticulumGitNode {
    // Match RNS 1.5.4's handle_work + _work_complete/_work_activate. Resolve
    // the repository once at the caller; never authorize one path and move another.
    fn work_transition(
        &self,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        group: &str,
        repository: &str,
        activate: bool,
    ) -> Vec<u8> {
        if !self.resolve_permission(&remote, group, repository, Self::PERM_READ) {
            return response(Self::RES_NOT_FOUND, "Not found", None);
        }
        if !self.resolve_permission(&remote, group, repository, Self::PERM_WRITE)
            || !self.resolve_permission(&remote, group, repository, Self::PERM_INTERACT)
        {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        let Some(value) = map_value(request, &rmpv::Value::String("doc_id".into())) else {
            return response(Self::RES_INVALID_REQ, "No document ID specified", None);
        };
        let Some(id) = value.as_u64().or_else(|| value.as_str()?.parse::<u64>().ok()) else {
            return response(Self::RES_INVALID_REQ, "Invalid document ID", None);
        };
        let Some(record) = self.groups.get(group).and_then(|state| state.repositories.get(repository)) else {
            return response(Self::RES_NOT_FOUND, "Not found", None);
        };
        let root = Self::work_root(record);
        let scopes: &[&str] = if activate { &["completed", "proposed"] } else { &["active"] };
        let Some(source) = scopes.iter().map(|scope| root.join(scope).join(id.to_string()))
            .find(|path| path.is_dir()) else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        let Some(document) = self.work_load_document(&source.join("root")) else {
            return response(Self::RES_REMOTE_FAIL, "Error loading document", None);
        };
        let is_author = Self::work_meta(&document)
            .and_then(|meta| map_value(meta, &rmpv::Value::String("author".into())))
            .is_some_and(|author| match author {
                // Python serializes identity hashes as binary. Accept the prior
                // Rust hexadecimal representation when reading existing documents.
                rmpv::Value::Binary(hash) => hash.as_slice() == remote,
                rmpv::Value::String(hash) => hash.as_str()
                    .is_some_and(|hash| hash.eq_ignore_ascii_case(&hex::encode(remote))),
                _ => false,
            });
        if !is_author && !self.resolve_doc_permission(&remote, group, repository, id, Self::PERM_ADMIN) {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        let scope = if activate { "active" } else { "completed" };
        let target = root.join(scope).join(id.to_string());
        // A duplicate target is corrupt state, not permission to overwrite a document.
        if target.exists() {
            return response(Self::RES_REMOTE_FAIL, "Document target already exists", None);
        }
        if let Err(error) = fs::create_dir_all(root.join(scope)).and_then(|()| fs::rename(source, target)) {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        response(Self::RES_OK, "", Some(&rmpv::Value::Map(vec![
            (rmpv::Value::from("id"), rmpv::Value::from(id)),
            (rmpv::Value::from("scope"), rmpv::Value::from(scope)),
        ])))
    }
}

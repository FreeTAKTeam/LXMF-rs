impl ReticulumGitNode {
    fn work_delete(&self, root: &Path, request: &[(rmpv::Value, rmpv::Value)]) -> Vec<u8> {
        let Some((_, id, directory, _)) = self.work_request_document_for_delete(root, request) else {
            return response(Self::RES_REMOTE_FAIL, "Remote error", None);
        };
        if Self::work_remove_permissions(root, id).is_err() {
            return response(Self::RES_REMOTE_FAIL, "Remote error", None);
        }
        match fs::remove_dir_all(directory) {
            Ok(()) => vec![Self::RES_OK],
            Err(error) => response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        }
    }

    fn work_comment(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> Vec<u8> {
        let Some((_, _, directory, _)) = self.work_request_document_ignoring_scope(root, request) else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        if !directory.join("root").is_file() {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        }
        let content = map_string(request, &rmpv::Value::String("content".into()))
            .unwrap_or_default()
            .trim()
            .to_string();
        if content.is_empty() {
            return response(Self::RES_INVALID_REQ, "Content is required", None);
        }
        if content.len() > Self::WORK_DOC_LIMIT {
            return response(Self::RES_INVALID_REQ, "Content limit exceeded", None);
        }
        let comment_id = self.work_get_next_comment_id(&directory);
        let now = Self::work_now();
        let comment = rmpv::Value::Map(vec![
            (
                rmpv::Value::String("content".into()),
                rmpv::Value::String(content.into()),
            ),
            (
                rmpv::Value::String("meta".into()),
                rmpv::Value::Map(vec![
                    (
                        rmpv::Value::String("format".into()),
                        rmpv::Value::String(Self::work_format(request).into()),
                    ),
                    (rmpv::Value::String("title".into()), rmpv::Value::Nil),
                    (rmpv::Value::String("created".into()), now.clone()),
                    (rmpv::Value::String("edited".into()), now),
                    (
                        rmpv::Value::String("signature".into()),
                        Self::work_optional_binary(request, "signature"),
                    ),
                    (
                        rmpv::Value::String("author".into()),
                        rmpv::Value::Binary(remote.to_vec()),
                    ),
                ]),
            ),
        ]);
        match self.work_save_document(&directory.join(comment_id.to_string()), &comment) {
            Ok(()) => response(
                Self::RES_OK,
                "",
                Some(&rmpv::Value::Map(vec![(
                    rmpv::Value::String("id".into()),
                    rmpv::Value::from(comment_id),
                )])),
            ),
            Err(error) => response(Self::RES_REMOTE_FAIL, error, None),
        }
    }

    fn work_transition(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        group: &str,
        repository: &str,
        activate: bool,
    ) -> Vec<u8> {
        if !self.resolve_permission(&remote, group, repository, Self::PERM_WRITE)
            || !self.resolve_permission(&remote, group, repository, Self::PERM_INTERACT)
        {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        let Some(value) = map_value(request, &rmpv::Value::String("doc_id".into())) else {
            return response(Self::RES_INVALID_REQ, "No document ID specified", None);
        };
        let Some(id) = value
            .as_u64()
            .or_else(|| value.as_str()?.parse::<u64>().ok())
        else {
            return response(Self::RES_INVALID_REQ, "Invalid document ID", None);
        };
        let source_scopes: &[&str] = if activate {
            &["completed", "proposed"]
        } else {
            &["active"]
        };
        let Some(source) = source_scopes
            .iter()
            .map(|scope| root.join(scope).join(id.to_string()))
            .find(|directory| directory.is_dir())
        else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        let Some(document) = self.work_load_document(&source.join("root")) else {
            return response(Self::RES_REMOTE_FAIL, "Error loading document", None);
        };
        let is_author = Self::work_author_matches(&document, &remote);
        let admin = self.resolve_doc_permission(
            &remote,
            group,
            repository,
            id,
            Self::PERM_ADMIN,
        );
        if !is_author && !admin {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        let scope = if activate { "active" } else { "completed" };
        let target = root.join(scope).join(id.to_string());
        if target.exists() {
            return response(Self::RES_REMOTE_FAIL, "Document target already exists", None);
        }
        if let Err(error) =
            fs::create_dir_all(root.join(scope)).and_then(|()| fs::rename(source, target))
        {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
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

    fn work_permissions(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
    ) -> Vec<u8> {
        let Some(id) = map_value(request, &rmpv::Value::String("doc_id".into()))
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse::<u64>().ok()))
        else {
            return response(Self::RES_INVALID_REQ, "No document ID specified", None);
        };
        let Some((_, _, _, _)) = self.work_request_document(root, request) else {
            return response(Self::RES_NOT_FOUND, "Document not found", None);
        };
        let Some(step) = map_string(request, &rmpv::Value::String("step".into())) else {
            return response(Self::RES_INVALID_REQ, "Invalid step", None);
        };
        let path = Self::work_permission_path(root, id);
        match step.as_str() {
            "get" => {
                let content = match fs::read_to_string(path) {
                    Ok(content) => content,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
                    Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
                };
                response(
                    Self::RES_OK,
                    "",
                    Some(&rmpv::Value::Map(vec![(
                        rmpv::Value::String("content".into()),
                        rmpv::Value::String(content.into()),
                    )])),
                )
            }
            "set" => {
                let content =
                    map_string(request, &rmpv::Value::String("content".into())).unwrap_or_default();
                if let Err(error) = self.validate_allowed_content(&content) {
                    return response(Self::RES_INVALID_REQ, error, None);
                }
                match fs::metadata(&path) {
                    Ok(metadata) if is_executable_file(&metadata) => {
                        return response(
                            Self::RES_DISALLOWED,
                            "Executable permission resolvers are node-owned",
                            None,
                        )
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
                }
                match Self::work_write_permissions(root, id, &content) {
                    Ok(()) => vec![Self::RES_OK],
                    Err(error) => response(Self::RES_REMOTE_FAIL, error, None),
                }
            }
            _ => response(Self::RES_INVALID_REQ, "Invalid step", None),
        }
    }
}

fn map_value_mut<'a>(
    map: &'a mut [(rmpv::Value, rmpv::Value)],
    key: &str,
) -> Option<&'a mut rmpv::Value> {
    map.iter_mut()
        .find(|(candidate, _)| candidate == &rmpv::Value::String(key.into()))
        .map(|(_, value)| value)
}

fn value_map_mut(value: &mut rmpv::Value) -> Option<&mut Vec<(rmpv::Value, rmpv::Value)>> {
    match value {
        rmpv::Value::Map(map) => Some(map),
        _ => None,
    }
}

fn set_map_value(map: &mut Vec<(rmpv::Value, rmpv::Value)>, key: &str, value: rmpv::Value) {
    if let Some(existing) = map_value_mut(map, key) {
        *existing = value;
    } else {
        map.push((rmpv::Value::String(key.into()), value));
    }
}

#[cfg(test)]
include!("issue_612_work_mutation_compat_tests.rs");

impl ReticulumGitNode {
    fn validate_allowed_content(&self, content: &str) -> Result<(), String> {
        for (line_number, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if self.parse_permission(line).is_none() {
                return Err(format!("Invalid permission on line {}", line_number + 1));
            }
        }
        Ok(())
    }

    fn permission_content(&self, path: &Path) -> Result<String, String> {
        let sidecar = permission_sidecar(path).map_err(|error| error.to_string())?;
        let metadata = match fs::metadata(&sidecar) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(String::new())
            }
            Err(error) => return Err(error.to_string()),
        };
        if is_executable_file(&metadata) {
            return Err("Executable permission resolvers are node-owned".to_string());
        }
        fs::read_to_string(sidecar).map_err(|error| error.to_string())
    }

    fn permission_response(&self, path: &Path) -> Vec<u8> {
        let content = match self.permission_content(path) {
            Ok(content) => content,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error, None),
        };
        response(Self::RES_OK, "", Some(&rmpv::Value::Map(vec![(
            rmpv::Value::String("content".into()),
            rmpv::Value::String(content.into()),
        )])))
    }

    pub fn handle_permission_request(
        &mut self,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> Vec<u8> {
        let operation = map_string(request, &rmpv::Value::String("operation".into())).unwrap_or_default();
        let step = map_string(request, &rmpv::Value::String("step".into())).unwrap_or_default();
        match operation.as_str() {
            "gperms" => {
                let Some(group) = map_string(request, &rmpv::Value::from(2_u64)) else {
                    return response(Self::RES_INVALID_REQ, "No group specified", None);
                };
                let Some(state) = self.groups.get(&group) else {
                    return response(Self::RES_NOT_FOUND, "Not found", None);
                };
                if !self.resolve_group_permission(&remote, &group, Self::PERM_ADMIN) {
                    return response(Self::RES_DISALLOWED, "Not allowed", None);
                }
                let allowed_path = state.path.clone();
                match step.as_str() {
                    "get" => self.permission_response(&allowed_path),
                    "set" => self.set_permission_file(&allowed_path, request),
                    _ => response(Self::RES_INVALID_REQ, "Invalid step", None),
                }
            }
            "rperms" => {
                let Some(repository_path) = map_string(request, &repository_key()) else {
                    return response(Self::RES_INVALID_REQ, "No repository specified", None);
                };
                let Some((group, repository)) = self.parse_request_repository_path(&repository_path) else {
                    return response(Self::RES_INVALID_REQ, "Invalid repository path", None);
                };
                let Some(state) = self.groups.get(&group).and_then(|group| group.repositories.get(&repository)) else {
                    return response(Self::RES_NOT_FOUND, "Not found", None);
                };
                if !self.resolve_permission(&remote, &group, &repository, Self::PERM_ADMIN) {
                    return response(Self::RES_DISALLOWED, "Not allowed", None);
                }
                let allowed_path = state.path.clone();
                match step.as_str() {
                    "get" => self.permission_response(&allowed_path),
                    "set" => self.set_permission_file(&allowed_path, request),
                    _ => response(Self::RES_INVALID_REQ, "Invalid step", None),
                }
            }
            _ => response(Self::RES_INVALID_REQ, "Invalid request", None),
        }
    }

    fn set_permission_file(
        &mut self,
        path: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
    ) -> Vec<u8> {
        let content = map_string(request, &rmpv::Value::String("content".into())).unwrap_or_default();
        if let Err(error) = self.validate_allowed_content(&content) {
            return response(Self::RES_INVALID_REQ, error, None);
        }
        let sidecar = match permission_sidecar(path) {
            Ok(sidecar) => sidecar,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        };
        match fs::metadata(&sidecar) {
            Ok(metadata) if is_executable_file(&metadata) => {
                return response(
                    Self::RES_DISALLOWED,
                    "Executable permission resolvers are node-owned",
                    None,
                )
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        }
        let permissions = match self.parse_permissions_strict(&content) {
            Ok(permissions) => permissions,
            Err(error) => return response(Self::RES_INVALID_REQ, error, None),
        };
        let Some(parent) = sidecar.parent() else {
            return response(Self::RES_REMOTE_FAIL, "Permission path has no parent", None);
        };
        let temporary = match tempfile::NamedTempFile::new_in(parent) {
            Ok(temporary) => temporary,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        };
        if let Err(error) = fs::write(temporary.path(), &content) {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        if let Err(error) = temporary.as_file().sync_all() {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        if let Err(error) = fs::rename(temporary.path(), &sidecar) {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        for group in self.groups.values_mut() {
            if companion_path(&group.path, "allowed") == sidecar {
                let configured = self.configured_permissions.get(&group.name).cloned();
                let mut effective = permissions.clone();
                if let Some(configured) = configured {
                    effective.merge(&configured);
                }
                group.permissions = effective;
            }
            for repository in group.repositories.values_mut() {
                if companion_path(&repository.path, "allowed") == sidecar {
                    repository.permissions = permissions.clone();
                }
            }
        }
        vec![Self::RES_OK]
    }
}

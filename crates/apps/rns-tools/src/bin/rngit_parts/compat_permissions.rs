impl ReticulumGitNode {
    pub const PERM_READ: u8 = 0x01;
    pub const PERM_WRITE: u8 = 0x02;
    pub const PERM_READWRITE: u8 = 0x03;
    pub const PERM_CREATE: u8 = 0x04;
    pub const PERM_STATS: u8 = 0x05;
    pub const PERM_RELEASE: u8 = 0x06;
    pub const PERM_INTERACT: u8 = 0x07;
    pub const PERM_PROPOSE: u8 = 0x08;
    pub const PERM_ADMIN: u8 = 0x09;

    pub const TGT_NONE: u8 = 0x01;
    pub const TGT_ALL: u8 = 0x02;

    pub fn parse_permission(&self, permission_string: &str) -> Option<(u8, PermissionTarget)> {
        let mut components = permission_string.split(':');
        let permission = components.next()?.to_ascii_lowercase();
        let target = components.next()?;
        if components.next().is_some() {
            return None;
        }
        let permission = match permission.as_str() {
            "r" | "read" => Self::PERM_READ,
            "w" | "write" => Self::PERM_WRITE,
            "rw" | "readwrite" => Self::PERM_READWRITE,
            "c" | "create" => Self::PERM_CREATE,
            "s" | "stats" => Self::PERM_STATS,
            "rel" | "release" => Self::PERM_RELEASE,
            "i" | "interact" => Self::PERM_INTERACT,
            "p" | "propose" => Self::PERM_PROPOSE,
            "adm" | "admin" => Self::PERM_ADMIN,
            _ => return None,
        };
        let target = match target.to_ascii_lowercase().as_str() {
            "n" | "none" | "nobody" => PermissionTarget::None,
            "a" | "all" | "everyone" => PermissionTarget::All,
            _ => {
                if target.len() != RNGIT_HASH_HEX_LENGTH {
                    return None;
                }
                PermissionTarget::Identity(hex::decode(target).ok()?.try_into().ok()?)
            }
        };
        Some((permission, target))
    }

    pub fn parse_request_repository_path(&self, path: &str) -> Option<(String, String)> {
        let mut components = path.split('/');
        let group = components.next()?;
        let repository = components.next()?;
        if components.next().is_some() || group.len() > 256 || repository.len() > 256 {
            return None;
        }
        Some((group.to_string(), repository.to_string()))
    }

    pub fn parse_request_group_path(&self, path: &str) -> Option<String> {
        if path.contains('/') || path.len() > 256 {
            None
        } else {
            Some(path.to_string())
        }
    }

    pub fn permissions_from_allowed_input(&self, allowed_input: Option<&str>) -> PermissionSet {
        let mut permissions = PermissionSet::default();
        for line in allowed_input.unwrap_or_default().lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((permission, target)) = self.parse_permission(line) {
                permissions.add(permission, target);
            }
        }
        permissions
    }

    fn permission_allowed(
        remote: &[u8; 16],
        repository: &PermissionList,
        group: &PermissionList,
        repository_admin: &PermissionList,
        group_admin: &PermissionList,
    ) -> bool {
        if repository.deny {
            false
        } else if repository.all
            || repository.identities.contains(remote)
            || repository_admin.all
            || repository_admin.identities.contains(remote)
        {
            true
        } else if !repository.is_empty() || group.deny {
            false
        } else if group.all || group.identities.contains(remote) {
            true
        } else {
            group_admin.all || group_admin.identities.contains(remote)
        }
    }

    pub fn resolve_permission(
        &self,
        remote_identity: &[u8; 16],
        group_name: &str,
        repository_name: &str,
        permission: u8,
    ) -> bool {
        if self.blocked_identities.contains(remote_identity) {
            return false;
        }
        let Some(group) = self.groups.get(group_name) else {
            return false;
        };
        let Some(repository) = group.repositories.get(repository_name) else {
            return false;
        };
        let Some(repository_permissions) = repository.permissions.list(permission) else {
            return false;
        };
        let Some(group_permissions) = group.permissions.list(permission) else {
            return false;
        };
        Self::permission_allowed(
            remote_identity,
            repository_permissions,
            group_permissions,
            &repository.permissions.admin,
            &group.permissions.admin,
        )
    }

    pub fn resolve_group_permission(
        &self,
        remote_identity: &[u8; 16],
        group_name: &str,
        permission: u8,
    ) -> bool {
        if self.blocked_identities.contains(remote_identity) {
            return false;
        }
        let Some(group) = self.groups.get(group_name) else {
            return false;
        };
        let Some(group_permissions) = group.permissions.list(permission) else {
            return false;
        };
        !group_permissions.deny
            && (group_permissions.all
                || group_permissions.identities.contains(remote_identity)
                || group.permissions.admin.all
                || group.permissions.admin.identities.contains(remote_identity))
    }

    pub fn resolve_doc_permission(
        &self,
        remote_identity: &[u8; 16],
        group_name: &str,
        repository_name: &str,
        doc_id: u64,
        permission: u8,
    ) -> bool {
        let Some(group) = self.groups.get(group_name) else {
            return false;
        };
        let Some(repository) = group.repositories.get(repository_name) else {
            return false;
        };
        let allowed_path = repository
            .path
            .with_extension("work")
            .join(format!("{doc_id}.allowed"));
        let doc_permissions = fs::read_to_string(allowed_path)
            .ok()
            .map(|value| self.permissions_from_allowed_input(Some(&value)))
            .unwrap_or_default();
        let Some(doc_list) = doc_permissions.list(permission) else {
            return false;
        };
        if doc_list.deny {
            return false;
        }
        if doc_list.all || doc_list.identities.contains(remote_identity) {
            return true;
        }
        self.resolve_permission(remote_identity, group_name, repository_name, permission)
    }
}

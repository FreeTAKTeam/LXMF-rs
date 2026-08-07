impl ReticulumGitNode {
    pub fn load_repository_group(
        &mut self,
        group_name: &str,
        group_path: &Path,
    ) -> io::Result<usize> {
        if let Some(existing) = self.groups.get(group_name) {
            if existing.path != group_path {
                return Ok(0);
            }
        } else {
            self.groups.insert(
                group_name.to_string(),
                RepositoryGroup {
                    name: group_name.to_string(),
                    path: group_path.to_path_buf(),
                    permissions: PermissionSet::default(),
                    repositories: BTreeMap::new(),
                },
            );
        }
        let group_permissions = fs::read_to_string(group_path.with_extension("allowed"))
            .ok()
            .map(|value| self.permissions_from_allowed_input(Some(&value)))
            .unwrap_or_default();
        if let Some(group) = self.groups.get_mut(group_name) {
            group.permissions = group_permissions;
        }
        let mut loaded = 0;
        for entry in fs::read_dir(group_path)? {
            let path = entry?.path();
            if self.load_repository(group_name, &path)? {
                loaded += 1;
            }
        }
        Ok(loaded)
    }

    pub fn load_repository(&mut self, group_name: &str, path: &Path) -> io::Result<bool> {
        if !path.is_dir()
            || path.extension().is_some_and(|ext| ext == "work" || ext == "releases")
        {
            return Ok(false);
        }
        let bare = Command::new("git")
            .args(["config", "--bool", "core.bare"])
            .current_dir(path)
            .output()
            .map(|output| output.status.success() && output.stdout == b"true\n")
            .unwrap_or(false);
        if !bare {
            return Ok(false);
        }
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or_default();
        let permissions = fs::read_to_string(path.with_extension("allowed"))
            .ok()
            .map(|value| self.permissions_from_allowed_input(Some(&value)))
            .unwrap_or_default();
        let record = RepositoryRecord {
            name: name.to_string(),
            path: path.to_path_buf(),
            fork: None,
            mirror: None,
            permissions,
        };
        if let Some(group) = self.groups.get_mut(group_name) {
            group.repositories.insert(name.to_string(), record);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn update_group_permissions(&mut self, group_name: &str) -> io::Result<()> {
        let Some(path) = self.groups.get(group_name).map(|group| group.path.clone()) else {
            return Ok(());
        };
        let permissions = fs::read_to_string(path.with_extension("allowed"))
            .ok()
            .map(|value| self.permissions_from_allowed_input(Some(&value)))
            .unwrap_or_default();
        if let Some(group) = self.groups.get_mut(group_name) {
            group.permissions = permissions;
        }
        Ok(())
    }

    pub fn start(&mut self) {
        self.should_run = true;
    }

    pub fn announce(&mut self) -> u64 {
        self.last_announce = unix_now();
        self.last_announce
    }

    pub fn jobs(&mut self) -> bool {
        self.should_run
    }

    pub fn log_request(&self, _message: &str, remote_identity: Option<&[u8; 16]>) -> bool {
        remote_identity.is_none_or(|identity| !self.blocked_identities.contains(identity))
    }

    pub fn last_upstream_sync(&self, path: &Path) -> u64 {
        let output = match Command::new("git")
            .args(["config", "--get", "repository.rngit.upstream.sync"])
            .current_dir(path)
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                eprintln!("rngit: could not read upstream sync config in {}: {error}", path.display());
                return 0;
            }
        };
        if !output.status.success() {
            return 0;
        }
        let value = match String::from_utf8(output.stdout) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("rngit: upstream sync config is not UTF-8 in {}: {error}", path.display());
                return 0;
            }
        };
        match value.trim().parse::<u64>() {
            Ok(value) => value,
            Err(error) => {
                eprintln!("rngit: upstream sync config is not an integer in {}: {error}", path.display());
                0
            }
        }
    }

    pub fn view_succeeded(
        &mut self,
        group: Option<&str>,
        repository: Option<&str>,
        ignored: bool,
    ) {
        if !ignored {
            if let (Some(group), Some(repository)) = (group, repository) {
                self.record_repository_view(group, repository);
            } else if let Some(group) = group {
                self.record_group_view(group);
            } else {
                self.record_page_view("front");
            }
        }
    }

    pub fn fetch_succeeded(&mut self, group: &str, repository: &str, ignored: bool) {
        if !ignored {
            self.record_fetch(group, repository);
        }
    }

    pub fn push_succeeded(&mut self, group: &str, repository: &str, ignored: bool) {
        if !ignored {
            self.record_push(group, repository);
        }
    }

    pub fn download_succeeded(&mut self, group: &str, repository: &str, ignored: bool) {
        if !ignored {
            self.record_download(group, repository);
        }
    }

    pub fn release_download_succeeded(
        &mut self,
        group: &str,
        repository: &str,
        ignored: bool,
    ) {
        if !ignored {
            self.record_release_download(group, repository);
        }
    }

    fn increment(&mut self, group: Option<&str>, repository: Option<&str>, key: &str) {
        let day = unix_day();
        match (group, repository) {
            (Some(group), Some(repository)) => {
                self.stats
                    .groups
                    .entry(group.to_string())
                    .or_default()
                    .entry(repository.to_string())
                    .or_default()
                    .entry(format!("{key}:{day}"))
                    .and_modify(|value| *value += 1)
                    .or_insert(1);
            }
            (Some(group), None) => {
                self.stats
                    .groups
                    .entry(group.to_string())
                    .or_default()
                    .entry("_group".to_string())
                    .or_default()
                    .entry(format!("{key}:{day}"))
                    .and_modify(|value| *value += 1)
                    .or_insert(1);
            }
            (None, None) => {
                self.stats
                    .pages
                    .entry(key.to_string())
                    .or_default()
                    .entry(day)
                    .and_modify(|value| *value += 1)
                    .or_insert(1);
            }
            (None, Some(_)) => {}
        }
    }

    pub fn record_page_view(&mut self, page: &str) {
        self.increment(None, None, page);
    }

    pub fn record_group_view(&mut self, group: &str) {
        self.increment(Some(group), None, "view");
    }

    pub fn record_repository_view(&mut self, group: &str, repository: &str) {
        self.increment(Some(group), Some(repository), "view");
    }

    pub fn record_fetch(&mut self, group: &str, repository: &str) {
        self.increment(Some(group), Some(repository), "fetch");
    }

    pub fn record_push(&mut self, group: &str, repository: &str) {
        self.increment(Some(group), Some(repository), "push");
    }

    pub fn record_download(&mut self, group: &str, repository: &str) {
        self.increment(Some(group), Some(repository), "download");
    }

    pub fn record_release_download(&mut self, group: &str, repository: &str) {
        self.increment(Some(group), Some(repository), "release_download");
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

fn unix_day() -> String {
    (unix_now() / 86_400).to_string()
}

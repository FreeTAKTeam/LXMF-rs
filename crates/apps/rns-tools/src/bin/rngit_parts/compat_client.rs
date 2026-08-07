#[derive(Debug, Clone)]
pub struct ReticulumGitClient {
    pub destination_aliases: BTreeMap<String, String>,
    pub link_ready: bool,
    pub link_failed: bool,
    pub path_timeout_secs: u64,
    pub link_timeout_secs: u64,
    pub last_remote: Option<RemoteRepository>,
    pub local_node: Option<Arc<Mutex<ReticulumGitNode>>>,
}

impl Default for ReticulumGitClient {
    fn default() -> Self {
        Self {
            destination_aliases: BTreeMap::new(),
            link_ready: false,
            link_failed: false,
            path_timeout_secs: 15,
            link_timeout_secs: 15,
            last_remote: None,
            local_node: None,
        }
    }
}

impl ReticulumGitClient {
    pub const PROTO_SPEC: &'static str = "rns://";

    pub fn abort(&self, message: impl Into<String>) -> Result<(), String> {
        Err(message.into())
    }

    fn resolve_destination_alias(&self, value: &str) -> String {
        self.destination_aliases
            .get(value)
            .cloned()
            .unwrap_or_else(|| value.to_string())
    }

    fn parse_destination(&self, value: &str) -> Result<[u8; 16], String> {
        let value = self.resolve_destination_alias(value);
        if value.len() != RNGIT_HASH_HEX_LENGTH {
            return Err("Invalid destination hash length".to_string());
        }
        let bytes =
            hex::decode(value).map_err(|error| format!("Invalid destination hash: {error}"))?;
        bytes
            .try_into()
            .map_err(|_| "Invalid destination hash length".to_string())
    }

    fn components<'a>(&self, remote: &'a str) -> Result<Vec<&'a str>, String> {
        if remote.len() < Self::PROTO_SPEC.len()
            || !remote[..Self::PROTO_SPEC.len()].eq_ignore_ascii_case(Self::PROTO_SPEC)
        {
            return Err("Invalid protocol in remote URL".to_string());
        }
        Ok(remote[Self::PROTO_SPEC.len()..].split('/').collect())
    }

    pub fn parse_remote_url(&self, remote: &str) -> Result<RemoteRepository, String> {
        let components = self.components(remote)?;
        if components.len() != 3 {
            return Err("Invalid number of URL components".to_string());
        }
        Ok(RemoteRepository {
            destination: self.parse_destination(components[0])?,
            group: components[1].to_string(),
            repository: components[2].to_string(),
        })
    }

    pub fn parse_remote_group_url(&self, remote: &str) -> Result<RemoteGroup, String> {
        let components = self.components(remote)?;
        if components.len() != 2 {
            return Err("Invalid number of URL components".to_string());
        }
        Ok(RemoteGroup {
            destination: self.parse_destination(components[0])?,
            group: components[1].to_string(),
        })
    }

    pub fn parse_remote_destination_url(&self, remote: &str) -> Result<[u8; 16], String> {
        let components = self.components(remote)?;
        let Some(destination) = components.first() else {
            return Err("Invalid number of URL components".to_string());
        };
        self.parse_destination(destination)
    }

    pub fn connect_remote(&mut self, remote: &str) -> Result<RemoteRepository, String> {
        let parsed = self.parse_remote_url(remote)?;
        self.link_ready = false;
        self.link_failed = false;
        self.last_remote = Some(parsed.clone());
        if self.local_node.is_some() {
            self.link_established();
        }
        Ok(parsed)
    }

    pub fn attach_local_node(&mut self, node: ReticulumGitNode) {
        self.local_node = Some(Arc::new(Mutex::new(node)));
        self.link_established();
    }

    pub fn link_established(&mut self) {
        self.link_ready = true;
        self.link_failed = false;
    }

    pub fn link_closed(&mut self) {
        if !self.link_ready {
            self.link_failed = true;
        }
        self.link_ready = false;
    }

    pub fn send_request(&self, path: &str, data: &[u8]) -> Result<Vec<u8>, String> {
        if !self.link_ready {
            return Err("Link not ready at request time".to_string());
        }
        if let Some(node) = &self.local_node {
            return node
                .lock()
                .map_err(|_| "Local rngit node lock is poisoned".to_string())
                .map(|mut node| node.handle_request(path, data, [0_u8; 16]));
        }
        Err("No Reticulum request transport is attached".to_string())
    }

    fn request_repository(
        &self,
        path: &str,
        repository: &str,
        extra: impl IntoIterator<Item = (rmpv::Value, rmpv::Value)>,
    ) -> Result<Vec<u8>, String> {
        let mut entries = vec![(repository_key(), rmpv::Value::String(repository.into()))];
        entries.extend(extra);
        self.send_request(path, &pack_request(entries)?)
    }

    fn ensure_repository_remote(&mut self, remote: &str) -> Result<RemoteRepository, String> {
        self.connect_remote(remote)
    }

    pub fn create_repository(&mut self, remote: &str) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        self.request_repository(
            RNGIT_PATH_CREATE,
            &format!("{}/{}", parsed.group, parsed.repository),
            [],
        )
    }

    pub fn fork_repository(&mut self, source: &str, target: &str) -> Result<Vec<u8>, String> {
        self.clone_repository(source, target, RNGIT_PATH_FORK)
    }

    pub fn mirror_repository(&mut self, source: &str, target: &str) -> Result<Vec<u8>, String> {
        self.clone_repository(source, target, RNGIT_PATH_MIRROR)
    }

    fn clone_repository(&mut self, source: &str, target: &str, path: &str) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(target)?;
        self.request_repository(
            path,
            &format!("{}/{}", parsed.group, parsed.repository),
            [(rmpv::Value::String("source".into()), rmpv::Value::String(source.into()))],
        )
    }

    pub fn sync_repository(&mut self, remote: &str) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        self.request_repository(
            RNGIT_PATH_SYNC,
            &format!("{}/{}", parsed.group, parsed.repository),
            [],
        )
    }

    pub fn list_releases(&mut self, remote: &str) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        self.request_repository(
            RNGIT_PATH_RELEASE,
            &format!("{}/{}", parsed.group, parsed.repository),
            [(rmpv::Value::String("operation".into()), rmpv::Value::String("list".into()))],
        )
    }

    pub fn view_release(&mut self, remote: &str, target: &str) -> Result<Vec<u8>, String> {
        self.release_request(remote, "view", Some(target))
    }

    pub fn fetch_release(&mut self, remote: &str, target: &str) -> Result<Vec<u8>, String> {
        self.release_request(remote, "fetch", Some(target))
    }

    pub fn create_release(&mut self, remote: &str, target: &str) -> Result<Vec<u8>, String> {
        self.release_request(remote, "create", Some(target))
    }

    pub fn delete_release(&mut self, remote: &str, target: &str) -> Result<Vec<u8>, String> {
        self.release_request(remote, "delete", Some(target))
    }

    pub fn latest_release(&mut self, remote: &str, target: &str) -> Result<Vec<u8>, String> {
        self.release_request(remote, "latest", Some(target))
    }

    fn release_request(
        &mut self,
        remote: &str,
        operation: &str,
        target: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        let mut extra = vec![(rmpv::Value::String("operation".into()), rmpv::Value::String(operation.into()))];
        if let Some(target) = target {
            extra.push((rmpv::Value::String("target".into()), rmpv::Value::String(target.into())));
        }
        self.request_repository(
            RNGIT_PATH_RELEASE,
            &format!("{}/{}", parsed.group, parsed.repository),
            extra,
        )
    }

    pub fn group_permissions(&mut self, remote: &str) -> Result<Vec<u8>, String> {
        let parsed = self.parse_remote_group_url(remote)?;
        self.connect_remote(remote)?;
        self.send_request(
            RNGIT_PATH_PERMS,
            &pack_request([
                (rmpv::Value::from(2_u64), rmpv::Value::String(parsed.group.into())),
                (rmpv::Value::String("operation".into()), rmpv::Value::String("gperms".into())),
                (rmpv::Value::String("step".into()), rmpv::Value::String("get".into())),
            ])?,
        )
    }

    pub fn repository_permissions(&mut self, remote: &str) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        self.request_repository(
            RNGIT_PATH_PERMS,
            &format!("{}/{}", parsed.group, parsed.repository),
            [
                (rmpv::Value::String("operation".into()), rmpv::Value::String("rperms".into())),
                (rmpv::Value::String("step".into()), rmpv::Value::String("get".into())),
            ],
        )
    }

    pub fn work_list(&mut self, remote: &str, scope: &str) -> Result<Vec<u8>, String> {
        self.work_request(remote, "list", None, Some(scope), None, None)
    }

    pub fn work_view(&mut self, remote: &str, doc_id: u64, scope: &str) -> Result<Vec<u8>, String> {
        self.work_request(remote, "view", Some(doc_id), Some(scope), None, None)
    }

    pub fn work_create(&mut self, remote: &str, title: &str, content: &str) -> Result<Vec<u8>, String> {
        self.work_request(remote, "create", None, None, Some(title), Some(content))
    }

    pub fn work_propose(&mut self, remote: &str, title: &str, content: &str) -> Result<Vec<u8>, String> {
        self.work_request(remote, "propose", None, None, Some(title), Some(content))
    }

    pub fn work_edit(&mut self, remote: &str, doc_id: u64, title: &str, content: &str, scope: &str) -> Result<Vec<u8>, String> {
        self.work_request(remote, "edit", Some(doc_id), Some(scope), Some(title), Some(content))
    }

    pub fn work_delete(&mut self, remote: &str, doc_id: u64, scope: &str) -> Result<Vec<u8>, String> {
        self.work_request(remote, "delete", Some(doc_id), Some(scope), None, None)
    }

    pub fn work_comment(&mut self, remote: &str, doc_id: u64, scope: &str, content: &str) -> Result<Vec<u8>, String> {
        self.work_request(remote, "comment", Some(doc_id), Some(scope), None, Some(content))
    }

    pub fn work_complete(&mut self, remote: &str, doc_id: u64) -> Result<Vec<u8>, String> {
        self.work_request(remote, "complete", Some(doc_id), None, None, None)
    }

    pub fn work_activate(&mut self, remote: &str, doc_id: u64) -> Result<Vec<u8>, String> {
        self.work_request(remote, "activate", Some(doc_id), None, None, None)
    }

    pub fn work_permissions(&mut self, remote: &str, doc_id: u64) -> Result<Vec<u8>, String> {
        self.work_request(remote, "perms", Some(doc_id), None, None, None)
    }

    fn work_request(
        &mut self,
        remote: &str,
        operation: &str,
        doc_id: Option<u64>,
        scope: Option<&str>,
        title: Option<&str>,
        content: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        let mut extra = vec![(rmpv::Value::String("operation".into()), rmpv::Value::String(operation.into()))];
        if let Some(doc_id) = doc_id {
            extra.push((rmpv::Value::String("doc_id".into()), rmpv::Value::from(doc_id)));
        }
        if let Some(scope) = scope {
            extra.push((rmpv::Value::String("scope".into()), rmpv::Value::String(scope.into())));
        }
        if let Some(title) = title {
            extra.push((rmpv::Value::String("title".into()), rmpv::Value::String(title.into())));
        }
        if let Some(content) = content {
            extra.push((rmpv::Value::String("content".into()), rmpv::Value::String(content.into())));
        }
        self.request_repository(
            RNGIT_PATH_WORK,
            &format!("{}/{}", parsed.group, parsed.repository),
            extra,
        )
    }

    pub fn connect_server(&mut self, remote: &str) -> Result<RemoteRepository, String> {
        self.connect_remote(remote)
    }

    pub fn run(&mut self) -> Result<(), String> {
        if self.link_ready {
            Ok(())
        } else {
            Err("rngit client link is not established".to_string())
        }
    }

    pub fn handle_git_list(&mut self, remote: &str, for_push: bool) -> Result<Vec<String>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        let response = self.request_repository(
            RNGIT_PATH_LIST,
            &format!("{}/{}", parsed.group, parsed.repository),
            [(rmpv::Value::String("for_push".into()), rmpv::Value::Boolean(for_push))],
        )?;
        if response.first().copied() != Some(RNGIT_RES_OK) {
            return Err(String::from_utf8_lossy(response.get(1..).unwrap_or_default()).into_owned());
        }
        Ok(String::from_utf8_lossy(response.get(1..).unwrap_or_default())
            .lines()
            .map(ToOwned::to_owned)
            .collect())
    }

    pub fn process_fetch_queue(
        &mut self,
        remote: &str,
        refs: &[String],
    ) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        if refs.iter().any(|reference| san_ref(reference).is_none()) {
            return Err("invalid Git reference in fetch queue".to_string());
        }
        let refs = refs
            .iter()
            .map(|reference| {
                rmpv::Value::Map(vec![(
                    rmpv::Value::String("ref".into()),
                    rmpv::Value::String(reference.clone().into()),
                )])
            })
            .collect();
        self.request_repository(
            RNGIT_PATH_FETCH,
            &format!("{}/{}", parsed.group, parsed.repository),
            [(rmpv::Value::String("refs".into()), rmpv::Value::Array(refs))],
        )
    }

    pub fn process_push_queue(
        &mut self,
        remote: &str,
        local_ref: &str,
        remote_ref: &str,
        bundle: &[u8],
        force: bool,
    ) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        if san_ref(local_ref).is_none() || san_ref(remote_ref).is_none() {
            return Err("invalid Git reference in push queue".to_string());
        }
        self.request_repository(
            RNGIT_PATH_PUSH,
            &format!("{}/{}", parsed.group, parsed.repository),
            [
                (rmpv::Value::String("local_ref".into()), rmpv::Value::String(local_ref.into())),
                (rmpv::Value::String("remote_ref".into()), rmpv::Value::String(remote_ref.into())),
                (rmpv::Value::String("bundle".into()), rmpv::Value::Binary(bundle.to_vec())),
                (rmpv::Value::String("force".into()), rmpv::Value::Boolean(force)),
            ],
        )
    }
}

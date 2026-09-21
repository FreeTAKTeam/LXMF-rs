const RNGIT_PATH_LIST: &str = "/git/list";
const RNGIT_PATH_FETCH: &str = "/git/fetch";
const RNGIT_PATH_PUSH: &str = "/git/push";
const RNGIT_PATH_DELETE: &str = "/git/delete";
const RNGIT_PATH_CREATE: &str = "/git/create";
const RNGIT_PATH_FORK: &str = "/git/fork";
const RNGIT_PATH_SYNC: &str = "/git/sync";
const RNGIT_PATH_MIRROR: &str = "/git/mirror";
const RNGIT_PATH_RELEASE: &str = "/mgmt/release";
const RNGIT_PATH_WORK: &str = "/mgmt/work";
const RNGIT_PATH_PERMS: &str = "/mgmt/perms";

use rns_transport::identity::Identity;

pub(crate) fn rngit_paths() -> &'static [&'static str] {
    &[
        RNGIT_PATH_LIST,
        RNGIT_PATH_FETCH,
        RNGIT_PATH_PUSH,
        RNGIT_PATH_DELETE,
        RNGIT_PATH_CREATE,
        RNGIT_PATH_FORK,
        RNGIT_PATH_SYNC,
        RNGIT_PATH_MIRROR,
        RNGIT_PATH_RELEASE,
        RNGIT_PATH_WORK,
        RNGIT_PATH_PERMS,
    ]
}

const RNGIT_RES_OK: u8 = 0x00;
const RNGIT_RES_DISALLOWED: u8 = 0x01;
const RNGIT_RES_INVALID_REQ: u8 = 0x02;
const RNGIT_RES_NOT_FOUND: u8 = 0x03;
const RNGIT_RES_REMOTE_FAIL: u8 = 0xff;

fn pack_value(value: &rmpv::Value) -> Result<Vec<u8>, String> {
    let mut encoded = Vec::new();
    rmpv::encode::write_value(&mut encoded, value)
        .map_err(|error| format!("could not encode rngit value: {error}"))?;
    Ok(encoded)
}

fn pack_request(entries: impl IntoIterator<Item = (rmpv::Value, rmpv::Value)>) -> Result<Vec<u8>, String> {
    pack_value(&rmpv::Value::Map(entries.into_iter().collect()))
}

fn unpack_request(data: &[u8]) -> Result<Vec<(rmpv::Value, rmpv::Value)>, String> {
    let value = rmpv::decode::read_value(&mut std::io::Cursor::new(data))
        .map_err(|error| format!("invalid rngit request: {error}"))?;
    value.as_map().cloned().ok_or_else(|| "rngit request is not a map".to_string())
}

fn map_value<'a>(map: &'a [(rmpv::Value, rmpv::Value)], key: &rmpv::Value) -> Option<&'a rmpv::Value> {
    map.iter().find_map(|(candidate, value)| (candidate == key).then_some(value))
}

fn map_string(map: &[(rmpv::Value, rmpv::Value)], key: &rmpv::Value) -> Option<String> {
    map_value(map, key)?.as_str().map(ToOwned::to_owned)
}

fn map_bool(map: &[(rmpv::Value, rmpv::Value)], key: &rmpv::Value) -> bool {
    map_value(map, key).and_then(rmpv::Value::as_bool).unwrap_or(false)
}

fn response(code: u8, message: impl AsRef<[u8]>, payload: Option<&rmpv::Value>) -> Vec<u8> {
    let message = message.as_ref();
    let mut result = Vec::with_capacity(1 + message.len() + 32);
    result.push(code);
    if code != RNGIT_RES_OK {
        result.extend_from_slice(message);
    } else if let Some(payload) = payload {
        if let Ok(encoded) = pack_value(payload) {
            result.extend_from_slice(&encoded);
        }
    }
    result
}

fn repository_key() -> rmpv::Value {
    rmpv::Value::from(0_u64)
}

fn value_bytes(value: Option<&rmpv::Value>) -> Option<Vec<u8>> {
    value.and_then(rmpv::Value::as_slice).map(ToOwned::to_owned)
}

impl ReticulumGitNode {
    pub const RES_OK: u8 = RNGIT_RES_OK;
    pub const RES_DISALLOWED: u8 = RNGIT_RES_DISALLOWED;
    pub const RES_INVALID_REQ: u8 = RNGIT_RES_INVALID_REQ;
    pub const RES_NOT_FOUND: u8 = RNGIT_RES_NOT_FOUND;
    pub const RES_REMOTE_FAIL: u8 = RNGIT_RES_REMOTE_FAIL;

    /// Dispatch the wire paths registered by Python rngit's destination.
    ///
    /// This is deliberately transport-neutral: the live Reticulum adapter can
    /// pass request bytes here, while tests and local tooling can use the same
    /// byte-for-byte service through `ReticulumGitClient::attach_local_node`.
    pub fn handle_request(
        &mut self,
        path: &str,
        data: &[u8],
        remote_identity: [u8; 16],
    ) -> Vec<u8> {
        self.handle_request_with_peer_identity(path, data, remote_identity, None)
    }

    pub fn handle_request_with_peer_identity(
        &mut self,
        path: &str,
        data: &[u8],
        remote_identity: [u8; 16],
        peer_identity: Option<Identity>,
    ) -> Vec<u8> {
        let request = match unpack_request(data) {
            Ok(request) => request,
            Err(error) => return response(Self::RES_INVALID_REQ, error, None),
        };
        match path {
            RNGIT_PATH_LIST => self.handle_list(&request, remote_identity),
            RNGIT_PATH_FETCH => self.handle_fetch(&request, remote_identity),
            RNGIT_PATH_PUSH => self.handle_push(&request, remote_identity),
            RNGIT_PATH_DELETE => self.handle_delete(&request, remote_identity),
            RNGIT_PATH_CREATE => self.handle_create(&request, remote_identity),
            RNGIT_PATH_FORK => self.handle_fork(&request, remote_identity),
            RNGIT_PATH_MIRROR => self.handle_mirror(&request, remote_identity),
            RNGIT_PATH_SYNC => self.handle_sync(&request, remote_identity),
            RNGIT_PATH_RELEASE => self.handle_release(&request, remote_identity),
            RNGIT_PATH_WORK => {
                self.handle_work_with_peer_identity(&request, remote_identity, peer_identity)
            }
            RNGIT_PATH_PERMS => self.handle_perms(&request, remote_identity),
            _ => response(Self::RES_NOT_FOUND, "Unknown request path", None),
        }
    }

    pub fn register_request_handlers(&self) -> Vec<&'static str> {
        let mut paths = rngit_paths().to_vec();
        paths.extend_from_slice(page_paths());
        paths
    }

    pub fn remote_connected(&self) -> bool {
        true
    }

    pub fn remote_disconnected(&self) -> bool {
        true
    }

    pub fn remote_identified(&self, identity: [u8; 16]) -> bool {
        !self.blocked_identities.contains(&identity)
    }

    fn repository_for_request(
        &self,
        request: &[(rmpv::Value, rmpv::Value)],
    ) -> Result<(String, String, &RepositoryRecord), Vec<u8>> {
        let Some(repository_path) = map_string(request, &repository_key()) else {
            return Err(response(Self::RES_INVALID_REQ, "No repository specified", None));
        };
        let Some((group_name, repository_name)) = self.parse_request_repository_path(&repository_path)
        else {
            return Err(response(Self::RES_INVALID_REQ, "Invalid repository path", None));
        };
        let Some(group) = self.groups.get(&group_name) else {
            return Err(response(Self::RES_NOT_FOUND, "Not found", None));
        };
        let Some(repository) = group.repositories.get(&repository_name) else {
            return Err(response(Self::RES_NOT_FOUND, "Not found", None));
        };
        Ok((group_name, repository_name, repository))
    }

    fn run_git(path: &Path, args: &[&str]) -> Result<std::process::Output, String> {
        Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .map_err(|error| format!("git invocation failed: {error}"))
    }

    pub fn handle_list(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        let (group, repository, record) = match self.repository_for_request(request) {
            Ok(value) => value,
            Err(error) => return error,
        };
        let permission = if map_bool(request, &rmpv::Value::String("for_push".into())) {
            Self::PERM_WRITE
        } else {
            Self::PERM_READ
        };
        if !self.resolve_permission(&remote, &group, &repository, permission) {
            return response(Self::RES_NOT_FOUND, "Not found", None);
        }
        let output = match Self::run_git(&record.path, &["for-each-ref", "--format", "%(objectname) %(refname)"]) {
            Ok(output) if output.status.success() => output,
            Ok(output) => return response(Self::RES_REMOTE_FAIL, output.stderr, None),
            Err(error) => return response(Self::RES_REMOTE_FAIL, error, None),
        };
        let head = fs::read_to_string(record.path.join("HEAD"))
            .ok()
            .and_then(|head| head.strip_prefix("ref: ").map(|value| value.trim().to_string()))
            .unwrap_or_else(|| "master".to_string());
        let mut listing = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !listing.is_empty() {
            listing.push('\n');
        }
        listing.push('@');
        listing.push_str(&head);
        listing.push_str(" HEAD\n");
        let mut result = vec![Self::RES_OK];
        result.extend_from_slice(listing.as_bytes());
        self.view_succeeded(Some(&group), Some(&repository), false);
        result
    }

    pub fn handle_fetch(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        let (group, repository, record) = match self.repository_for_request(request) {
            Ok(value) => value,
            Err(error) => return error,
        };
        if !self.resolve_permission(&remote, &group, &repository, Self::PERM_READ) {
            return response(Self::RES_NOT_FOUND, "Not found", None);
        }
        let Some(refs) = map_value(request, &rmpv::Value::String("refs".into())).and_then(rmpv::Value::as_array) else {
            return response(Self::RES_INVALID_REQ, "No refs specified", None);
        };
        let refs = refs.iter().filter_map(rmpv::Value::as_map).filter_map(|map| {
            map_string(map, &rmpv::Value::String("ref".into()))
        }).collect::<Vec<_>>();
        if refs.is_empty() || refs.iter().any(|reference| san_ref(reference).is_none()) {
            return response(Self::RES_INVALID_REQ, "Invalid request", None);
        }
        let tempdir = match tempfile::tempdir() {
            Ok(tempdir) => tempdir,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        };
        let bundle = tempdir.path().join("fetch.bundle");
        let bundle_arg = bundle.to_string_lossy();
        let mut args = vec!["bundle", "create", "--no-progress", bundle_arg.as_ref()];
        args.extend(refs.iter().map(String::as_str));
        let output = match Self::run_git(&record.path, &args) {
            Ok(output) => output,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error, None),
        };
        if !output.status.success() {
            if String::from_utf8_lossy(&output.stderr).to_ascii_lowercase().contains("empty bundle") {
                return vec![Self::RES_OK];
            }
            return response(Self::RES_REMOTE_FAIL, output.stderr, None);
        }
        let bytes = match fs::read(bundle) {
            Ok(bytes) => bytes,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        };
        self.fetch_succeeded(&group, &repository, false);
        let mut result = vec![Self::RES_OK];
        result.extend(bytes);
        result
    }

    pub fn handle_push(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        let (group, repository, record) = match self.repository_for_request(request) {
            Ok(value) => value,
            Err(error) => return error,
        };
        if !self.resolve_permission(&remote, &group, &repository, Self::PERM_WRITE) {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        let Some(local_ref) = map_string(request, &rmpv::Value::String("local_ref".into())) else {
            return response(Self::RES_INVALID_REQ, "Missing ref specification", None);
        };
        let Some(remote_ref) = map_string(request, &rmpv::Value::String("remote_ref".into())) else {
            return response(Self::RES_INVALID_REQ, "Missing ref specification", None);
        };
        if san_ref(&local_ref).is_none() || san_ref(&remote_ref).is_none() {
            return response(Self::RES_INVALID_REQ, "Invalid request", None);
        }
        let Some(bundle) = value_bytes(map_value(request, &rmpv::Value::String("bundle".into()))) else {
            return response(Self::RES_INVALID_REQ, "Invalid request data", None);
        };
        let tempdir = match tempfile::tempdir() {
            Ok(tempdir) => tempdir,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        };
        let bundle_path = tempdir.path().join("push.bundle");
        if let Err(error) = fs::write(&bundle_path, bundle) {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        let verify = match Self::run_git(&record.path, &["bundle", "verify", bundle_path.to_string_lossy().as_ref()]) {
            Ok(output) => output,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error, None),
        };
        if !verify.status.success() {
            return response(Self::RES_REMOTE_FAIL, verify.stderr, None);
        }
        let bundle_arg = bundle_path.to_string_lossy();
        let mut fetch_args = vec!["fetch", bundle_arg.as_ref()];
        let refspec = format!("{local_ref}:{remote_ref}");
        fetch_args.push(&refspec);
        if map_bool(request, &rmpv::Value::String("force".into())) {
            fetch_args.push("--force");
        }
        let output = match Self::run_git(&record.path, &fetch_args) {
            Ok(output) => output,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error, None),
        };
        if !output.status.success() {
            return response(Self::RES_REMOTE_FAIL, output.stderr, None);
        }
        self.push_succeeded(&group, &repository, false);
        vec![Self::RES_OK]
    }

    pub fn handle_delete(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        let (group, repository, record) = match self.repository_for_request(request) {
            Ok(value) => value,
            Err(error) => return error,
        };
        if !self.resolve_permission(&remote, &group, &repository, Self::PERM_WRITE) {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        let Some(reference) = map_string(request, &rmpv::Value::String("ref".into())) else {
            return response(Self::RES_INVALID_REQ, "Invalid request", None);
        };
        if san_ref(&reference).is_none() || !reference.starts_with("refs/") {
            return response(Self::RES_INVALID_REQ, "Invalid request", None);
        }
        let output = match Self::run_git(&record.path, &["update-ref", "-d", &reference]) {
            Ok(output) => output,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error, None),
        };
        if !output.status.success() {
            return response(Self::RES_REMOTE_FAIL, output.stderr, None);
        }
        self.push_succeeded(&group, &repository, false);
        vec![Self::RES_OK]
    }

    pub fn handle_create(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        let Some(repository_path) = map_string(request, &repository_key()) else {
            return response(Self::RES_INVALID_REQ, "No repository specified", None);
        };
        let Some((group_name, repository_name)) = self.parse_request_repository_path(&repository_path) else {
            return response(Self::RES_INVALID_REQ, "Invalid request", None);
        };
        let Some(group) = self.groups.get(&group_name).cloned() else {
            return response(Self::RES_NOT_FOUND, "Not found", None);
        };
        if !self.resolve_group_permission(&remote, &group_name, Self::PERM_CREATE) {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        let repository_path = group.path.join(&repository_name);
        if repository_path.exists() {
            return response(Self::RES_DISALLOWED, "Repository already exists", None);
        }
        if let Err(error) = fs::create_dir_all(&repository_path) {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        let output = match Command::new("git").args(["init", "--bare"]).current_dir(&repository_path).output() {
            Ok(output) => output,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        };
        if !output.status.success() {
            return response(Self::RES_REMOTE_FAIL, output.stderr, None);
        }
        let allowed = format!("read:all\nwrite:{}\n", hex::encode(remote));
        if let Err(error) = fs::write(companion_path(&repository_path, "allowed"), allowed) {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        if let Err(error) = self.load_repository(&group_name, &repository_path) {
            return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
        }
        vec![Self::RES_OK]
    }

    fn handle_clone(
        &mut self,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        mirror: bool,
    ) -> Vec<u8> {
        let Some(repository_path) = map_string(request, &repository_key()) else {
            return response(Self::RES_INVALID_REQ, "No repository specified", None);
        };
        let Some(source) = map_string(request, &rmpv::Value::String("source".into())) else {
            return response(Self::RES_INVALID_REQ, "No source specified", None);
        };
        let Some((group_name, repository_name)) = self.parse_request_repository_path(&repository_path) else {
            return response(Self::RES_INVALID_REQ, "Invalid request", None);
        };
        let Some(group) = self.groups.get(&group_name).cloned() else {
            return response(Self::RES_NOT_FOUND, "Not found", None);
        };
        if !self.resolve_group_permission(&remote, &group_name, Self::PERM_CREATE) {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        if source.starts_with("rns://") {
            return response(Self::RES_REMOTE_FAIL, "Remote clone requires a Reticulum transport", None);
        }
        let destination = group.path.join(repository_name);
        if destination.exists() {
            return response(Self::RES_DISALLOWED, "Repository already exists", None);
        }
        let mode = if mirror { "--mirror" } else { "--bare" };
        let output = Command::new("git").args(["clone", mode, &source, destination.to_string_lossy().as_ref()]).output();
        match output {
            Ok(output) if output.status.success() => match self.load_repository(&group_name, &destination) {
                Ok(_) => vec![Self::RES_OK],
                Err(error) => response(Self::RES_REMOTE_FAIL, error.to_string(), None),
            },
            Ok(output) => response(Self::RES_REMOTE_FAIL, output.stderr, None),
            Err(error) => response(Self::RES_REMOTE_FAIL, error.to_string(), None),
        }
    }

    pub fn handle_fork(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        self.handle_clone(request, remote, false)
    }

    pub fn handle_mirror(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        self.handle_clone(request, remote, true)
    }

    pub fn handle_sync(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        let Ok((group, repository, record)) = self.repository_for_request(request) else {
            return response(Self::RES_NOT_FOUND, "Not found", None);
        };
        if !self.resolve_permission(&remote, &group, &repository, Self::PERM_WRITE) {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        let output = match Self::run_git(&record.path, &["remote", "update"]) {
            Ok(output) => output,
            Err(error) => return response(Self::RES_REMOTE_FAIL, error, None),
        };
        if output.status.success() {
            vec![Self::RES_OK]
        } else {
            response(Self::RES_REMOTE_FAIL, output.stderr, None)
        }
    }

    fn handle_release(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        self.handle_release_request(request, remote)
    }

    pub fn handle_work(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        self.handle_work_request(request, remote)
    }

    pub fn handle_work_with_peer_identity(
        &mut self,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        peer_identity: Option<Identity>,
    ) -> Vec<u8> {
        self.handle_work_request_with_peer_identity(request, remote, peer_identity)
    }

    pub fn handle_perms(&mut self, request: &[(rmpv::Value, rmpv::Value)], remote: [u8; 16]) -> Vec<u8> {
        self.handle_permission_request(request, remote)
    }
}

include!("work_storage.rs");
include!("work_service.rs");
include!("work_documents.rs");
include!("work_mutations.rs");
include!("permissions_service.rs");
include!("release_service.rs");
include!("stats_service.rs");

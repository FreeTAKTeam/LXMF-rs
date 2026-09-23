use std::env as page_env;
use std::fmt::Write as FmtWrite;
use std::sync::atomic::{AtomicU64, Ordering};

const PAGE_INDEX: &str = "/page/index.mu";
const PAGE_GROUP: &str = "/page/group.mu";
const PAGE_REPO: &str = "/page/repo.mu";
const PAGE_TREE: &str = "/page/tree.mu";
const PAGE_BLOB: &str = "/page/blob.mu";
const PAGE_COMMITS: &str = "/page/commits.mu";
const PAGE_COMMIT: &str = "/page/commit.mu";
const PAGE_REFS: &str = "/page/refs.mu";
const PAGE_STATS: &str = "/page/stats.mu";
const PAGE_RELEASES: &str = "/page/releases.mu";
const PAGE_RELEASE: &str = "/page/release.mu";
const PAGE_WORK: &str = "/page/work.mu";
const PAGE_WORK_DOC: &str = "/page/work_doc.mu";
const NULL_IDENTITY_HASH: [u8; 16] = [
    0xd7, 0xdb, 0x22, 0xf6, 0x3b, 0x45, 0x3c, 0x23, 0xbb, 0x06, 0x88, 0xdd, 0xe5, 0x65, 0xb7,
    0xc1,
];
const PAGE_MEDIA: &str = "/media";
const FILE_ARTIFACT: &str = "/file/artifact";
const FILE_DOWNLOAD: &str = "/file/download";
const FILE_WORKDOC: &str = "/file/workdoc";

const PAGE_PATHS: &[&str] = &[
    PAGE_INDEX,
    PAGE_GROUP,
    PAGE_REPO,
    PAGE_TREE,
    PAGE_BLOB,
    PAGE_COMMITS,
    PAGE_COMMIT,
    PAGE_REFS,
    PAGE_STATS,
    PAGE_RELEASES,
    PAGE_RELEASE,
    PAGE_WORK,
    PAGE_WORK_DOC,
    PAGE_MEDIA,
    FILE_ARTIFACT,
    FILE_DOWNLOAD,
    FILE_WORKDOC,
];

const DEFAULT_BASE_TEMPLATE: &str = "#!c=0\n> {NODE_NAME}\n\n{NAVIGATION}\n{PAGE_CONTENT}\n<\n-\n`a`F666`[Served by rngit {VERSION}`:/page/index.mu] - {GEN_TIME}`f";
const DEFAULT_NO_IDENT_TEMPLATE: &str = ">>No Identity\n\nThis page requires identification, and none was received.\n";
const DEFAULT_FRONT_TEMPLATE: &str = "> Groups\n\n{PAGE_CONTENT}";

static MEDIA_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PageResponse {
    pub data: Vec<u8>,
    pub metadata: Option<Vec<u8>>,
    pub response_is_false: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DecodedPageRequest {
    pub path: &'static str,
    pub requested_at: f64,
    pub data: rmpv::Value,
}

#[derive(Default)]
pub(crate) struct PageLinkCleanup {
    pub removed_directories: usize,
    pub failures: Vec<PageLinkCleanupFailure>,
}

pub(crate) struct PageLinkCleanupFailure {
    pub link_id: [u8; 16],
    pub directory: PathBuf,
    pub error: io::Error,
}

fn log_page_media_cleanup_failure(
    context: &str,
    link_id: [u8; 16],
    directory: &Path,
    error: &io::Error,
) {
    // Keep unrecovered temp-data failures visible even when routine output is silent.
    eprintln!(
        "rngit: failed to remove temporary media directory {} for link {} during {context}; retained for retry: {error}",
        directory.display(),
        hex::encode(link_id),
    );
}

pub(crate) fn page_paths() -> &'static [&'static str] {
    PAGE_PATHS
}

pub(crate) fn decode_page_request(data: &[u8]) -> Result<Option<DecodedPageRequest>, String> {
    let mut cursor = std::io::Cursor::new(data);
    let value = rmpv::decode::read_value(&mut cursor)
        .map_err(|error| format!("invalid NomadNet request: {error}"))?;
    if cursor.position() != data.len() as u64 {
        return Err("NomadNet request has trailing bytes".to_string());
    }
    let Some(values) = value.as_array() else { return Ok(None) };
    if values.len() != 3 {
        return Ok(None);
    }
    let requested_at = values[0]
        .as_f64()
        .or_else(|| values[0].as_u64().map(|value| value as f64))
        .ok_or_else(|| "NomadNet request timestamp is not numeric".to_string())?;
    let Some(path_hash) = values[1].as_slice() else { return Ok(None) };
    let Some(path) = PAGE_PATHS
        .iter()
        .copied()
        .find(|path| rns_transport::hash::address_hash(path.as_bytes()) == path_hash)
    else {
        return Ok(None);
    };
    Ok(Some(DecodedPageRequest { path, requested_at, data: values[2].clone() }))
}

fn map_string_value(map: &[(rmpv::Value, rmpv::Value)], key: &str) -> Option<String> {
    map_value(map, &rmpv::Value::String(key.into())).and_then(|value| {
        value.as_str().map(ToOwned::to_owned).or_else(|| {
            value.as_slice().and_then(|value| match String::from_utf8(value.to_vec()) {
                Ok(value) => Some(value),
                Err(error) => {
                    eprintln!("rngit: request field {key:?} is not UTF-8: {error}");
                    None
                }
            })
        })
    })
}

fn page_param(map: &[(rmpv::Value, rmpv::Value)], key: &str) -> Option<String> {
    map_string_value(map, key).or_else(|| map_string_value(map, &format!("var_{key}")))
}

fn request_map(value: &rmpv::Value) -> &[(rmpv::Value, rmpv::Value)] {
    value.as_map().map(Vec::as_slice).unwrap_or(&[])
}

fn null_identity(identity: &[u8; 16]) -> bool {
    identity.iter().all(|value| *value == 0)
}

fn page_response(data: Vec<u8>, metadata: Option<Vec<u8>>) -> PageResponse {
    PageResponse { data, metadata, response_is_false: false }
}

fn page_denial_response() -> PageResponse {
    PageResponse { data: Vec::new(), metadata: None, response_is_false: true }
}

impl ReticulumGitNode {
    pub fn load_repository_root(&mut self, root: &Path) -> io::Result<usize> {
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("rngit root is not a directory: {}", root.display()),
            ));
        }
        let mut loaded = 0;
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if path.is_dir() && !name.starts_with('.') {
                loaded += self.load_repository_group(name, &path)?;
            }
        }
        Ok(loaded)
    }

    pub fn set_page_template(&mut self, name: impl Into<String>, template: impl Into<String>) {
        self.page_templates.insert(name.into(), template.into());
    }

    pub fn load_page_templates(&mut self, directory: &Path) -> io::Result<usize> {
        if !directory.is_dir() {
            return Ok(0);
        }
        let mut loaded = 0;
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("mu") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            let content = match fs::metadata(&path) {
                Ok(metadata) if is_executable_file(&metadata) => run_permission_resolver(&path)?,
                Ok(_) => fs::read_to_string(&path)?,
                Err(error) => return Err(error),
            };
            if content.len() > 256 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("page template is too large: {}", path.display()),
                ));
            }
            self.set_page_template(name, content);
            loaded += 1;
        }
        Ok(loaded)
    }

    pub fn page_link_connected(&mut self, link_id: [u8; 16]) {
        self.active_page_links.entry(link_id).or_default();
    }

    pub(crate) fn active_page_link_ids(&self) -> Vec<[u8; 16]> {
        self.active_page_links.keys().copied().collect()
    }

    pub(crate) fn clean_page_links(&mut self, link_ids: &[[u8; 16]]) -> PageLinkCleanup {
        let mut cleanup = PageLinkCleanup::default();
        for link_id in link_ids {
            let link_cleanup = self.page_link_closed(*link_id);
            cleanup.removed_directories += link_cleanup.removed_directories;
            cleanup.failures.extend(link_cleanup.failures);
        }
        cleanup
    }

    pub(crate) fn page_link_closed(&mut self, link_id: [u8; 16]) -> PageLinkCleanup {
        let paths = self
            .active_page_links
            .get(&link_id)
            .map(|paths| paths.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut cleanup = PageLinkCleanup::default();
        for directory in paths {
            match Self::remove_tracked_page_media_directory(
                &mut self.active_page_links,
                link_id,
                &directory,
                |path| fs::remove_dir_all(path),
            ) {
                Ok(true) => cleanup.removed_directories += 1,
                Ok(false) => {}
                Err(error) => {
                    cleanup.failures.push(PageLinkCleanupFailure { link_id, directory, error })
                }
            }
        }
        if cleanup.failures.is_empty() {
            self.active_page_links.remove(&link_id);
        }
        cleanup
    }

    pub(crate) fn next_media_directory(&mut self, link_id: [u8; 16]) -> io::Result<PathBuf> {
        let sequence = MEDIA_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = page_env::temp_dir().join(format!(
            "rngit-media-{}-{sequence}",
            hex::encode(link_id)
        ));
        fs::create_dir(&directory)?;
        self.active_page_links.entry(link_id).or_default().insert(directory.clone());
        Ok(directory)
    }

    fn template(&self, name: &str) -> String {
        self.page_templates.get(name).cloned().unwrap_or_else(|| match name {
            "base" => DEFAULT_BASE_TEMPLATE.to_string(),
            "front" => DEFAULT_FRONT_TEMPLATE.to_string(),
            _ => "{PAGE_CONTENT}".to_string(),
        })
    }

    fn render_template(&self, name: &str, content: &str, navigation: &str) -> Vec<u8> {
        let content = self.template(name).replace("{PAGE_CONTENT}", content);
        let mut base = self.template("base");
        base = base.replace("{NODE_NAME}", &self.page_node_name);
        base = base.replace("{VERSION}", env!("CARGO_PKG_VERSION"));
        base = base.replace("{NAVIGATION}", navigation);
        base = base.replace("{GEN_TIME}", "local");
        base = base.replace("{PAGE_CONTENT}", &content);
        base.into_bytes()
    }

    fn no_ident(&self) -> PageResponse {
        let content = if self.page_templates.contains_key("no_ident") {
            ""
        } else {
            DEFAULT_NO_IDENT_TEMPLATE
        };
        page_response(self.render_template("no_ident", content, ""), None)
    }

    fn not_found(&self, navigation: &str, message: &str) -> PageResponse {
        page_response(self.render_template("fallback", &format!(">Not Found\n\n{message}\n"), navigation), None)
    }

    pub(crate) fn handle_page_request(
        &mut self,
        path: &str,
        data: &rmpv::Value,
        remote_identity: [u8; 16],
        link_id: [u8; 16],
    ) -> Option<PageResponse> {
        let map = request_map(data);
        if path == PAGE_MEDIA {
            return self.serve_media(map, remote_identity, link_id);
        }
        if path == FILE_ARTIFACT {
            return self.serve_artifact(map, remote_identity);
        }
        if path == FILE_DOWNLOAD {
            return self.serve_download(map, remote_identity);
        }
        if path == FILE_WORKDOC {
            return self.serve_workdoc(map, remote_identity);
        }
        if !PAGE_PATHS[..13].contains(&path) {
            return None;
        }
        if null_identity(&remote_identity)
            && self.blocked_identities.contains(&NULL_IDENTITY_HASH)
        {
            return Some(self.no_ident());
        }
        match path {
            PAGE_INDEX => Some(self.serve_front(remote_identity)),
            PAGE_GROUP => Some(self.serve_group(map, remote_identity)),
            PAGE_REPO => Some(self.serve_repo(map, remote_identity)),
            PAGE_TREE => Some(self.serve_tree(map, remote_identity)),
            PAGE_BLOB => Some(self.serve_blob(map, remote_identity, link_id)),
            PAGE_COMMITS => Some(self.serve_commits(map, remote_identity)),
            PAGE_COMMIT => Some(self.serve_commit(map, remote_identity)),
            PAGE_REFS => Some(self.serve_refs(map, remote_identity)),
            PAGE_STATS => Some(self.serve_stats(map, remote_identity)),
            PAGE_RELEASES => Some(self.serve_releases(map, remote_identity)),
            PAGE_RELEASE => Some(self.serve_release(map, remote_identity)),
            PAGE_WORK => Some(self.serve_work(map, remote_identity)),
            PAGE_WORK_DOC => Some(self.serve_work_doc(map, remote_identity)),
            _ => None,
        }
    }

    fn remove_tracked_page_media_directory(
        active_page_links: &mut BTreeMap<[u8; 16], BTreeSet<PathBuf>>,
        link_id: [u8; 16],
        directory: &Path,
        remove_directory: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<bool> {
        let removed = match remove_directory(directory) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };

        if let Some(directories) = active_page_links.get_mut(&link_id) {
            directories.remove(directory);
        }
        Ok(removed)
    }

    #[allow(dead_code)]
    pub(crate) fn handle_page_map_request(
        &mut self,
        path: &str,
        data: &[(rmpv::Value, rmpv::Value)],
        remote_identity: [u8; 16],
    ) -> Vec<u8> {
        let value = rmpv::Value::Map(data.to_vec());
        match self.handle_page_request(path, &value, remote_identity, [0; 16]) {
            Some(page_response) => {
                let value = if page_response.response_is_false {
                    rmpv::Value::Boolean(false)
                } else {
                    rmpv::Value::Binary(page_response.data)
                };
                response(Self::RES_OK, "", Some(&value))
            }
            None => response(Self::RES_NOT_FOUND, "Not found", None),
        }
    }

    fn accessible_repository(&self, remote: &[u8; 16], group: &str, repository: &str) -> Option<&RepositoryRecord> {
        let group_state = self.groups.get(group)?;
        let repository_state = group_state.repositories.get(repository)?;
        self.resolve_permission(remote, group, repository, Self::PERM_READ).then_some(repository_state)
    }

    fn navigation(&self, group: Option<&str>, repository: Option<&str>) -> String {
        let mut navigation = String::from(">>\n[Node`:/page/index.mu]");
        if let Some(group) = group {
            let _ = write!(navigation, " / [{group}`:/page/group.mu|g={group}]");
        }
        if let (Some(group), Some(repository)) = (group, repository) {
            let _ = write!(navigation, " / {repository} `:/page/repo.mu|g={group}|r={repository}]");
        }
        navigation.push('\n');
        navigation
    }

}

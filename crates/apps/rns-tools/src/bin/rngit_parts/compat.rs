const RNGIT_HASH_HEX_LENGTH: usize = 32;

use std::fs;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRepository {
    pub destination: [u8; 16],
    pub group: String,
    pub repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteGroup {
    pub destination: [u8; 16],
    pub group: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionTarget {
    None,
    All,
    Identity([u8; 16]),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionList {
    pub deny: bool,
    pub all: bool,
    pub identities: BTreeSet<[u8; 16]>,
}

impl PermissionList {
    fn add(&mut self, target: PermissionTarget) {
        match target {
            PermissionTarget::None => self.deny = true,
            PermissionTarget::All => self.all = true,
            PermissionTarget::Identity(identity) => {
                self.identities.insert(identity);
            }
        }
    }

    fn is_empty(&self) -> bool {
        !self.deny && !self.all && self.identities.is_empty()
    }

    fn merge(&mut self, other: &Self) {
        self.deny |= other.deny;
        self.all |= other.all;
        self.identities.extend(other.identities.iter().copied());
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionSet {
    pub read: PermissionList,
    pub write: PermissionList,
    pub create: PermissionList,
    pub stats: PermissionList,
    pub release: PermissionList,
    pub interact: PermissionList,
    pub propose: PermissionList,
    pub admin: PermissionList,
}

impl PermissionSet {
    fn list_mut(&mut self, permission: u8) -> Option<&mut PermissionList> {
        match permission {
            ReticulumGitNode::PERM_READ => Some(&mut self.read),
            ReticulumGitNode::PERM_WRITE => Some(&mut self.write),
            ReticulumGitNode::PERM_CREATE => Some(&mut self.create),
            ReticulumGitNode::PERM_STATS => Some(&mut self.stats),
            ReticulumGitNode::PERM_RELEASE => Some(&mut self.release),
            ReticulumGitNode::PERM_INTERACT => Some(&mut self.interact),
            ReticulumGitNode::PERM_PROPOSE => Some(&mut self.propose),
            ReticulumGitNode::PERM_ADMIN => Some(&mut self.admin),
            _ => None,
        }
    }

    fn list(&self, permission: u8) -> Option<&PermissionList> {
        match permission {
            ReticulumGitNode::PERM_READ => Some(&self.read),
            ReticulumGitNode::PERM_WRITE => Some(&self.write),
            ReticulumGitNode::PERM_CREATE => Some(&self.create),
            ReticulumGitNode::PERM_STATS => Some(&self.stats),
            ReticulumGitNode::PERM_RELEASE => Some(&self.release),
            ReticulumGitNode::PERM_INTERACT => Some(&self.interact),
            ReticulumGitNode::PERM_PROPOSE => Some(&self.propose),
            ReticulumGitNode::PERM_ADMIN => Some(&self.admin),
            _ => None,
        }
    }

    fn add(&mut self, permission: u8, target: PermissionTarget) {
        if permission == ReticulumGitNode::PERM_READWRITE {
            self.read.add(target.clone());
            self.write.add(target);
        } else if let Some(list) = self.list_mut(permission) {
            list.add(target);
        }
    }

    fn merge(&mut self, other: &Self) {
        self.read.merge(&other.read);
        self.write.merge(&other.write);
        self.create.merge(&other.create);
        self.stats.merge(&other.stats);
        self.release.merge(&other.release);
        self.interact.merge(&other.interact);
        self.propose.merge(&other.propose);
        self.admin.merge(&other.admin);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryRecord {
    pub name: String,
    pub path: PathBuf,
    pub fork: Option<String>,
    pub mirror: Option<String>,
    pub permissions: PermissionSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryGroup {
    pub name: String,
    pub path: PathBuf,
    pub permissions: PermissionSet,
    pub repositories: BTreeMap<String, RepositoryRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RngitStats {
    pub pages: BTreeMap<String, BTreeMap<String, u64>>,
    pub groups: BTreeMap<String, BTreeMap<String, BTreeMap<String, u64>>>,
}

#[derive(Debug, Clone)]
pub struct ReticulumGitNode {
    pub groups: BTreeMap<String, RepositoryGroup>,
    pub configured_permissions: BTreeMap<String, PermissionSet>,
    pub identity_aliases: BTreeMap<String, [u8; 16]>,
    pub blocked_identities: BTreeSet<[u8; 16]>,
    pub stats: RngitStats,
    pub should_run: bool,
    pub last_announce: u64,
    pub page_node_name: String,
    pub page_templates: BTreeMap<String, String>,
    pub media_conversion: bool,
    pub media_quality: u8,
    pub media_max_dimension: Option<u32>,
    pub active_page_links: BTreeMap<[u8; 16], BTreeSet<PathBuf>>,
}

impl Default for ReticulumGitNode {
    fn default() -> Self {
        Self {
            groups: BTreeMap::new(),
            configured_permissions: BTreeMap::new(),
            identity_aliases: BTreeMap::new(),
            blocked_identities: BTreeSet::new(),
            stats: RngitStats::default(),
            should_run: false,
            last_announce: 0,
            page_node_name: "Anonymous Git Node".to_string(),
            page_templates: BTreeMap::new(),
            media_conversion: true,
            media_quality: 85,
            media_max_dimension: None,
            active_page_links: BTreeMap::new(),
        }
    }
}

include!("protocol.rs");
include!("compat_client.rs");
include!("release_client.rs");
include!("native_client.rs");
include!("compat_permissions.rs");
include!("compat_node.rs");
include!("media.rs");
include!("pages.rs");
include!("page_git.rs");
include!("page_git_work.rs");
include!("page_media.rs");

// Python appends companion suffixes. Replacing an extension makes `repo.git`
// collide with `repo.allowed`/`repo.work` belonging to another repository.
fn companion_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".");
    value.push(suffix);
    PathBuf::from(value)
}

fn permission_sidecar(path: &Path) -> io::Result<PathBuf> {
    let canonical = companion_path(path, "allowed");
    let legacy = path.with_extension("allowed");
    if canonical != legacy && !canonical.try_exists()? {
        let legacy_is_file = match fs::metadata(&legacy) {
            Ok(metadata) => metadata.is_file(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        if legacy_is_file {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "ambiguous legacy permission file {}; migrate node-side to {} before loading",
                    legacy.display(),
                    canonical.display()
                ),
            ));
        }
    }
    Ok(canonical)
}

const MAX_DYNAMIC_PERMISSION_OUTPUT: usize = 64 * 1024;
const DYNAMIC_PERMISSION_TIMEOUT: Duration = Duration::from_secs(2);

fn is_executable_file(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn read_bounded(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut exceeded = false;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = MAX_DYNAMIC_PERMISSION_OUTPUT.saturating_sub(output.len());
        let copied = read.min(remaining);
        output.extend_from_slice(&buffer[..copied]);
        exceeded |= read > copied;
    }
    if exceeded {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "permission resolver output exceeds 64 KiB",
        ))
    } else {
        Ok(output)
    }
}

fn spawn_permission_resolver(path: &Path) -> io::Result<std::process::Child> {
    let retry_deadline = Instant::now() + Duration::from_millis(250);
    loop {
        let result = Command::new(path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match result {
            Err(error)
                if error.kind() == io::ErrorKind::ExecutableFileBusy
                    && Instant::now() < retry_deadline =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            result => return result,
        }
    }
}

fn run_permission_resolver(path: &Path) -> io::Result<String> {
    let mut child = spawn_permission_resolver(path)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("permission resolver stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("permission resolver stderr unavailable"))?;
    let stdout_reader = std::thread::spawn(move || read_bounded(stdout));
    let stderr_reader = std::thread::spawn(move || read_bounded(stderr));
    let deadline = Instant::now() + DYNAMIC_PERMISSION_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "permission resolver exceeded 2 second timeout",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| io::Error::other("permission resolver stdout reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| io::Error::other("permission resolver stderr reader panicked"))??;
    if !status.success() {
        return Err(io::Error::other(format!(
            "permission resolver exited with {status}: {}",
            String::from_utf8_lossy(&stderr).trim()
        )));
    }
    String::from_utf8(stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("permission resolver output is not UTF-8: {error}"),
        )
    })
}

impl ReticulumGitNode {
    fn read_companion_permissions(&self, path: &Path) -> io::Result<PermissionSet> {
        let sidecar = permission_sidecar(path)?;
        let metadata = match fs::metadata(&sidecar) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(PermissionSet::default())
            }
            Err(error) => return Err(error),
        };
        let content = if is_executable_file(&metadata) {
            run_permission_resolver(&sidecar)?
        } else {
            fs::read_to_string(&sidecar)?
        };
        self.parse_permissions_strict(&content).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid permissions in {}: {error}", sidecar.display()),
            )
        })
    }

    pub fn set_configured_group_permissions(
        &mut self,
        group_name: &str,
        content: &str,
    ) -> Result<(), String> {
        let permissions = self.parse_permissions_strict(content)?;
        let effective = if let Some(group) = self.groups.get(group_name) {
            let mut effective = self
                .read_companion_permissions(&group.path)
                .map_err(|error| error.to_string())?;
            effective.merge(&permissions);
            Some(effective)
        } else {
            None
        };

        self.configured_permissions
            .insert(group_name.to_string(), permissions);
        if let (Some(group), Some(effective)) = (self.groups.get_mut(group_name), effective) {
            group.permissions = effective;
        }
        Ok(())
    }

    pub fn set_identity_alias(&mut self, alias: impl Into<String>, identity: [u8; 16]) {
        self.identity_aliases.insert(alias.into(), identity);
    }
}

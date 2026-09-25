const RNGIT_HASH_HEX_LENGTH: usize = 32;

use process_wrap::std::{StdChildWrapper, StdCommandWrap};
#[cfg(unix)]
use nix::fcntl::{fcntl, FcntlArg, OFlag};
#[cfg(unix)]
use process_wrap::std::ProcessGroup;
#[cfg(windows)]
use process_wrap::std::JobObject;
use std::fs;
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::os::fd::AsFd;

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

include!("bounded_executable.rs");
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

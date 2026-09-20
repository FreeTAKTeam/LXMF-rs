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

#[derive(Debug, Clone, Default)]
pub struct ReticulumGitNode {
    pub groups: BTreeMap<String, RepositoryGroup>,
    pub blocked_identities: BTreeSet<[u8; 16]>,
    pub stats: RngitStats,
    pub should_run: bool,
    pub last_announce: u64,
}

include!("protocol.rs");
include!("compat_client.rs");
include!("compat_permissions.rs");
include!("compat_node.rs");

// Python appends companion suffixes; replacing an extension makes repo.git
// collide with repo.allowed/repo.work belonging to a different repository.
fn companion_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".");
    value.push(suffix);
    PathBuf::from(value)
}

include!("work_transitions.rs");


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
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!(
                "ambiguous legacy permission file {}; migrate node-side to {} before loading",
                legacy.display(), canonical.display(),
            )));
        }
    }
    Ok(canonical)
}

impl ReticulumGitNode {
    fn read_companion_permissions(&self, path: &Path) -> io::Result<PermissionSet> {
        let path = permission_sidecar(path)?;
        let input = match fs::read_to_string(path) {
            Ok(input) => Some(input),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(self.permissions_from_allowed_input(input.as_deref()))
    }
}

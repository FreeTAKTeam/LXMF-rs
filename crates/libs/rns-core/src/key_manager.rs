use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::RnsError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyPurpose {
    IdentitySigning,
    TransportDh,
    SharedSecret,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredKey {
    pub key_id: String,
    pub purpose: KeyPurpose,
    pub material: Vec<u8>,
}

pub trait KeyManagerBackend {
    fn backend_id(&self) -> &'static str;
    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError>;
    fn put(&self, key: StoredKey) -> Result<(), RnsError>;
    fn delete(&self, key_id: &str) -> Result<(), RnsError>;
    fn list_ids(&self) -> Result<Vec<String>, RnsError>;
}

#[cfg(feature = "std")]
#[derive(Default)]
pub struct InMemoryKeyManager {
    keys: std::sync::RwLock<BTreeMap<String, StoredKey>>,
}

#[cfg(feature = "std")]
impl InMemoryKeyManager {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(feature = "std")]
impl KeyManagerBackend for InMemoryKeyManager {
    fn backend_id(&self) -> &'static str {
        "in-memory"
    }

    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
        let keys = self.keys.read().map_err(|_| RnsError::ConnectionError)?;
        Ok(keys.get(key_id).cloned())
    }

    fn put(&self, key: StoredKey) -> Result<(), RnsError> {
        let mut keys = self.keys.write().map_err(|_| RnsError::ConnectionError)?;
        keys.insert(key.key_id.clone(), key);
        Ok(())
    }

    fn delete(&self, key_id: &str) -> Result<(), RnsError> {
        let mut keys = self.keys.write().map_err(|_| RnsError::ConnectionError)?;
        keys.remove(key_id);
        Ok(())
    }

    fn list_ids(&self) -> Result<Vec<String>, RnsError> {
        let keys = self.keys.read().map_err(|_| RnsError::ConnectionError)?;
        Ok(keys.keys().cloned().collect())
    }
}

#[cfg(feature = "std")]
pub struct FileKeyManager {
    root: std::path::PathBuf,
}

#[cfg(feature = "std")]
impl FileKeyManager {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Result<Self, RnsError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|_| RnsError::ConnectionError)?;
        Ok(Self { root })
    }

    fn path_for_key(&self, key_id: &str) -> Result<std::path::PathBuf, RnsError> {
        if !is_valid_key_id(key_id) {
            return Err(RnsError::InvalidArgument);
        }
        Ok(self.root.join(format!("{key_id}.key")))
    }
}

#[cfg(feature = "std")]
impl KeyManagerBackend for FileKeyManager {
    fn backend_id(&self) -> &'static str {
        "file"
    }

    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
        let path = self.path_for_key(key_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(path).map_err(|_| RnsError::ConnectionError)?;
        let key = rmp_serde::from_slice::<StoredKey>(&bytes).map_err(|_| RnsError::PacketError)?;
        Ok(Some(key))
    }

    fn put(&self, key: StoredKey) -> Result<(), RnsError> {
        let path = self.path_for_key(key.key_id.as_str())?;
        let tmp_path = path.with_extension("tmp");
        let bytes = rmp_serde::to_vec_named(&key).map_err(|_| RnsError::PacketError)?;
        std::fs::write(&tmp_path, bytes).map_err(|_| RnsError::ConnectionError)?;
        std::fs::rename(&tmp_path, &path).map_err(|_| RnsError::ConnectionError)?;
        Ok(())
    }

    fn delete(&self, key_id: &str) -> Result<(), RnsError> {
        let path = self.path_for_key(key_id)?;
        match std::fs::remove_file(path) {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(RnsError::ConnectionError),
        }
    }

    fn list_ids(&self) -> Result<Vec<String>, RnsError> {
        let entries = std::fs::read_dir(&self.root).map_err(|_| RnsError::ConnectionError)?;
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| RnsError::ConnectionError)?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("key") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                ids.push(stem.to_string());
            }
        }
        ids.sort();
        Ok(ids)
    }
}

pub trait OsKeyStoreHook {
    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError>;
    fn put(&self, key: StoredKey) -> Result<(), RnsError>;
    fn delete(&self, key_id: &str) -> Result<(), RnsError>;
    fn list_ids(&self) -> Result<Vec<String>, RnsError>;
}

pub struct OsKeyStoreKeyManager<H> {
    hook: H,
}

impl<H> OsKeyStoreKeyManager<H> {
    pub fn new(hook: H) -> Self {
        Self { hook }
    }
}

impl<H: OsKeyStoreHook> KeyManagerBackend for OsKeyStoreKeyManager<H> {
    fn backend_id(&self) -> &'static str {
        "os-keystore"
    }

    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
        self.hook.get(key_id)
    }

    fn put(&self, key: StoredKey) -> Result<(), RnsError> {
        self.hook.put(key)
    }

    fn delete(&self, key_id: &str) -> Result<(), RnsError> {
        self.hook.delete(key_id)
    }

    fn list_ids(&self) -> Result<Vec<String>, RnsError> {
        self.hook.list_ids()
    }
}

pub trait HsmKeyStoreHook {
    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError>;
    fn put(&self, key: StoredKey) -> Result<(), RnsError>;
    fn delete(&self, key_id: &str) -> Result<(), RnsError>;
    fn list_ids(&self) -> Result<Vec<String>, RnsError>;
}

pub struct HsmKeyManager<H> {
    hook: H,
}

impl<H> HsmKeyManager<H> {
    pub fn new(hook: H) -> Self {
        Self { hook }
    }
}

impl<H: HsmKeyStoreHook> KeyManagerBackend for HsmKeyManager<H> {
    fn backend_id(&self) -> &'static str {
        "hsm"
    }

    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
        self.hook.get(key_id)
    }

    fn put(&self, key: StoredKey) -> Result<(), RnsError> {
        self.hook.put(key)
    }

    fn delete(&self, key_id: &str) -> Result<(), RnsError> {
        self.hook.delete(key_id)
    }

    fn list_ids(&self) -> Result<Vec<String>, RnsError> {
        self.hook.list_ids()
    }
}

pub struct FallbackKeyManager<Primary, Secondary> {
    primary: Primary,
    secondary: Secondary,
}

impl<Primary, Secondary> FallbackKeyManager<Primary, Secondary> {
    pub fn new(primary: Primary, secondary: Secondary) -> Self {
        Self { primary, secondary }
    }
}

impl<Primary, Secondary> KeyManagerBackend for FallbackKeyManager<Primary, Secondary>
where
    Primary: KeyManagerBackend,
    Secondary: KeyManagerBackend,
{
    fn backend_id(&self) -> &'static str {
        "fallback"
    }

    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
        match self.primary.get(key_id) {
            Ok(Some(key)) => Ok(Some(key)),
            Ok(None) => self.secondary.get(key_id),
            Err(_) => self.secondary.get(key_id),
        }
    }

    fn put(&self, key: StoredKey) -> Result<(), RnsError> {
        match self.primary.put(key.clone()) {
            Ok(_) => Ok(()),
            Err(_) => self.secondary.put(key),
        }
    }

    fn delete(&self, key_id: &str) -> Result<(), RnsError> {
        let primary_result = self.primary.delete(key_id);
        let secondary_result = self.secondary.delete(key_id);
        if primary_result.is_ok() || secondary_result.is_ok() {
            Ok(())
        } else {
            Err(RnsError::ConnectionError)
        }
    }

    fn list_ids(&self) -> Result<Vec<String>, RnsError> {
        match (self.primary.list_ids(), self.secondary.list_ids()) {
            (Ok(primary_ids), Ok(secondary_ids)) => Ok(merge_key_ids(primary_ids, secondary_ids)),
            (Ok(primary_ids), Err(_)) => Ok(primary_ids),
            (Err(_), Ok(secondary_ids)) => Ok(secondary_ids),
            (Err(_), Err(_)) => Err(RnsError::ConnectionError),
        }
    }
}

fn merge_key_ids(mut first: Vec<String>, second: Vec<String>) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for id in first.drain(..) {
        ids.insert(id);
    }
    for id in second {
        ids.insert(id);
    }
    ids.into_iter().collect()
}

fn is_valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::{
        FallbackKeyManager, FileKeyManager, HsmKeyManager, HsmKeyStoreHook, InMemoryKeyManager,
        KeyManagerBackend, KeyPurpose, OsKeyStoreHook, OsKeyStoreKeyManager, StoredKey,
    };
    use crate::RnsError;
    use alloc::collections::BTreeMap;
    use std::sync::RwLock;

    #[derive(Default)]
    struct FailingKeyManager;

    impl KeyManagerBackend for FailingKeyManager {
        fn backend_id(&self) -> &'static str {
            "failing"
        }

        fn get(&self, _key_id: &str) -> Result<Option<StoredKey>, RnsError> {
            Err(RnsError::ConnectionError)
        }

        fn put(&self, _key: StoredKey) -> Result<(), RnsError> {
            Err(RnsError::ConnectionError)
        }

        fn delete(&self, _key_id: &str) -> Result<(), RnsError> {
            Err(RnsError::ConnectionError)
        }

        fn list_ids(&self) -> Result<Vec<String>, RnsError> {
            Err(RnsError::ConnectionError)
        }
    }

    #[derive(Default)]
    struct HookMemoryStore {
        keys: RwLock<BTreeMap<String, StoredKey>>,
    }

    impl HookMemoryStore {
        fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
            let guard = self.keys.read().map_err(|_| RnsError::ConnectionError)?;
            Ok(guard.get(key_id).cloned())
        }

        fn put(&self, key: StoredKey) -> Result<(), RnsError> {
            let mut guard = self.keys.write().map_err(|_| RnsError::ConnectionError)?;
            guard.insert(key.key_id.clone(), key);
            Ok(())
        }

        fn delete(&self, key_id: &str) -> Result<(), RnsError> {
            let mut guard = self.keys.write().map_err(|_| RnsError::ConnectionError)?;
            guard.remove(key_id);
            Ok(())
        }

        fn list_ids(&self) -> Result<Vec<String>, RnsError> {
            let guard = self.keys.read().map_err(|_| RnsError::ConnectionError)?;
            Ok(guard.keys().cloned().collect())
        }
    }

    impl OsKeyStoreHook for HookMemoryStore {
        fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
            HookMemoryStore::get(self, key_id)
        }

        fn put(&self, key: StoredKey) -> Result<(), RnsError> {
            HookMemoryStore::put(self, key)
        }

        fn delete(&self, key_id: &str) -> Result<(), RnsError> {
            HookMemoryStore::delete(self, key_id)
        }

        fn list_ids(&self) -> Result<Vec<String>, RnsError> {
            HookMemoryStore::list_ids(self)
        }
    }

    impl HsmKeyStoreHook for HookMemoryStore {
        fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
            HookMemoryStore::get(self, key_id)
        }

        fn put(&self, key: StoredKey) -> Result<(), RnsError> {
            HookMemoryStore::put(self, key)
        }

        fn delete(&self, key_id: &str) -> Result<(), RnsError> {
            HookMemoryStore::delete(self, key_id)
        }

        fn list_ids(&self) -> Result<Vec<String>, RnsError> {
            HookMemoryStore::list_ids(self)
        }
    }

    fn sample_key(key_id: &str) -> StoredKey {
        StoredKey {
            key_id: key_id.to_string(),
            purpose: KeyPurpose::IdentitySigning,
            material: vec![1, 2, 3, 4],
        }
    }

    #[test]
    fn key_manager_in_memory_roundtrip_and_delete() {
        let manager = InMemoryKeyManager::new();
        manager.put(sample_key("node-signing")).expect("store key");
        let loaded = manager.get("node-signing").expect("load key").expect("key exists");
        assert_eq!(loaded.material, vec![1, 2, 3, 4]);
        assert_eq!(manager.list_ids().expect("list ids"), vec!["node-signing".to_owned()]);

        manager.delete("node-signing").expect("delete key");
        assert!(manager.get("node-signing").expect("load key").is_none());
    }

    #[test]
    fn key_manager_file_roundtrip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = FileKeyManager::new(temp.path()).expect("file manager");
        manager.put(sample_key("node-signing")).expect("store");
        let loaded = manager.get("node-signing").expect("load").expect("exists");
        assert_eq!(loaded.key_id, "node-signing");
        assert_eq!(manager.list_ids().expect("list ids"), vec!["node-signing".to_owned()]);
    }

    #[test]
    fn key_manager_file_rejects_invalid_key_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = FileKeyManager::new(temp.path()).expect("file manager");
        let invalid = manager.put(sample_key("../escape"));
        assert!(matches!(invalid, Err(RnsError::InvalidArgument)));
    }

    #[test]
    fn key_manager_os_keystore_hook_roundtrip() {
        let manager = OsKeyStoreKeyManager::new(HookMemoryStore::default());
        assert_eq!(manager.backend_id(), "os-keystore");
        manager.put(sample_key("os-signing")).expect("store");
        let loaded = manager.get("os-signing").expect("get").expect("exists");
        assert_eq!(loaded.key_id, "os-signing");
    }

    #[test]
    fn key_manager_hsm_hook_roundtrip() {
        let manager = HsmKeyManager::new(HookMemoryStore::default());
        assert_eq!(manager.backend_id(), "hsm");
        manager.put(sample_key("hsm-signing")).expect("store");
        let loaded = manager.get("hsm-signing").expect("get").expect("exists");
        assert_eq!(loaded.key_id, "hsm-signing");
    }

    #[test]
    fn key_manager_fallback_reads_from_secondary_on_primary_failure() {
        let fallback_store = InMemoryKeyManager::new();
        fallback_store.put(sample_key("fallback-key")).expect("store fallback key");
        let manager = FallbackKeyManager::new(FailingKeyManager, fallback_store);
        let loaded = manager.get("fallback-key").expect("fallback read").expect("key exists");
        assert_eq!(loaded.key_id, "fallback-key");
    }

    #[test]
    fn key_manager_fallback_writes_to_secondary_on_primary_failure() {
        let fallback_store = InMemoryKeyManager::new();
        let manager = FallbackKeyManager::new(FailingKeyManager, fallback_store);
        manager.put(sample_key("secondary-write")).expect("fallback write");
        let loaded = manager.get("secondary-write").expect("fallback read").expect("key exists");
        assert_eq!(loaded.key_id, "secondary-write");
    }

    #[test]
    fn key_manager_fallback_list_ids_merges_when_primary_fails() {
        let secondary = InMemoryKeyManager::new();
        secondary.put(sample_key("secondary-a")).expect("store secondary-a");
        secondary.put(sample_key("secondary-b")).expect("store secondary-b");
        let manager = FallbackKeyManager::new(FailingKeyManager, secondary);

        let ids = manager.list_ids().expect("list ids");
        assert_eq!(ids, vec!["secondary-a".to_owned(), "secondary-b".to_owned()]);
    }
}

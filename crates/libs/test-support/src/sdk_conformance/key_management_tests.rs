use rns_core::key_manager::{
    FallbackKeyManager, FileKeyManager, HsmKeyManager, HsmKeyStoreHook, InMemoryKeyManager,
    KeyManagerBackend, KeyPurpose, OsKeyStoreHook, OsKeyStoreKeyManager, StoredKey,
};
use rns_core::RnsError;
use std::collections::BTreeMap;
use std::sync::RwLock;

#[derive(Default)]
struct FailingBackend;

impl KeyManagerBackend for FailingBackend {
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
struct HookStore {
    keys: RwLock<BTreeMap<String, StoredKey>>,
}

impl HookStore {
    fn load(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
        let guard = self.keys.read().map_err(|_| RnsError::ConnectionError)?;
        Ok(guard.get(key_id).cloned())
    }

    fn store(&self, key: StoredKey) -> Result<(), RnsError> {
        let mut guard = self.keys.write().map_err(|_| RnsError::ConnectionError)?;
        guard.insert(key.key_id.clone(), key);
        Ok(())
    }

    fn remove(&self, key_id: &str) -> Result<(), RnsError> {
        let mut guard = self.keys.write().map_err(|_| RnsError::ConnectionError)?;
        guard.remove(key_id);
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>, RnsError> {
        let guard = self.keys.read().map_err(|_| RnsError::ConnectionError)?;
        Ok(guard.keys().cloned().collect())
    }
}

impl OsKeyStoreHook for HookStore {
    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
        self.load(key_id)
    }

    fn put(&self, key: StoredKey) -> Result<(), RnsError> {
        self.store(key)
    }

    fn delete(&self, key_id: &str) -> Result<(), RnsError> {
        self.remove(key_id)
    }

    fn list_ids(&self) -> Result<Vec<String>, RnsError> {
        self.list()
    }
}

impl HsmKeyStoreHook for HookStore {
    fn get(&self, key_id: &str) -> Result<Option<StoredKey>, RnsError> {
        self.load(key_id)
    }

    fn put(&self, key: StoredKey) -> Result<(), RnsError> {
        self.store(key)
    }

    fn delete(&self, key_id: &str) -> Result<(), RnsError> {
        self.remove(key_id)
    }

    fn list_ids(&self) -> Result<Vec<String>, RnsError> {
        self.list()
    }
}

fn sample_key(key_id: &str) -> StoredKey {
    StoredKey {
        key_id: key_id.to_owned(),
        purpose: KeyPurpose::IdentitySigning,
        material: vec![0xAA, 0xBB, 0xCC],
    }
}

#[test]
fn sdk_conformance_key_management_fallback_reads_secondary_backend() {
    let secondary = InMemoryKeyManager::new();
    secondary.put(sample_key("identity-a")).expect("store secondary key");
    let manager = FallbackKeyManager::new(FailingBackend, secondary);

    let loaded = manager.get("identity-a").expect("fallback read").expect("key should exist");
    assert_eq!(loaded.key_id, "identity-a");
}

#[test]
fn sdk_conformance_key_management_fallback_writes_secondary_backend() {
    let secondary = InMemoryKeyManager::new();
    let manager = FallbackKeyManager::new(FailingBackend, secondary);
    manager.put(sample_key("identity-b")).expect("fallback write");

    let loaded = manager.get("identity-b").expect("fallback read").expect("key should exist");
    assert_eq!(loaded.key_id, "identity-b");
}

#[test]
fn sdk_conformance_key_management_file_backend_rejects_invalid_ids() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manager = FileKeyManager::new(temp.path()).expect("file manager");
    let result = manager.put(sample_key("../escape"));
    assert!(matches!(result, Err(RnsError::InvalidArgument)));
}

#[test]
fn sdk_conformance_key_management_hook_adapters_roundtrip() {
    let os = OsKeyStoreKeyManager::new(HookStore::default());
    os.put(sample_key("os-key")).expect("store os key");
    assert_eq!(os.backend_id(), "os-keystore");
    assert_eq!(
        os.get("os-key").expect("get os key").expect("os key should exist").key_id,
        "os-key"
    );

    let hsm = HsmKeyManager::new(HookStore::default());
    hsm.put(sample_key("hsm-key")).expect("store hsm key");
    assert_eq!(hsm.backend_id(), "hsm");
    assert_eq!(
        hsm.get("hsm-key").expect("get hsm key").expect("hsm key should exist").key_id,
        "hsm-key"
    );
}

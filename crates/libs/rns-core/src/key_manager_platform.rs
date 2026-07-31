use super::*;

#[cfg(feature = "std")]
impl FileKeyManager {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Result<Self, RnsError> {
        let root = root.into();
        crate::secure_storage::ensure_private_directory(&root)
            .map_err(|_| RnsError::ConnectionError)?;
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
        let bytes = rmp_serde::to_vec_named(&key).map_err(|_| RnsError::PacketError)?;
        crate::secure_storage::atomic_write_private(&path, &bytes)
            .map_err(|_| RnsError::ConnectionError)?;
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
                ids.push(String::from(stem));
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

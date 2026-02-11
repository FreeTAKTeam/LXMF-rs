use std::fs;
use std::path::{Path, PathBuf};

use crate::error::LxmfError;

pub struct PropagationStore {
    root: PathBuf,
}

impl PropagationStore {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    pub fn save(&self, transient_id: &[u8], data: &[u8]) -> Result<(), LxmfError> {
        fs::create_dir_all(&self.root).map_err(|e| LxmfError::Io(e.to_string()))?;
        let name = hex::encode(transient_id);
        let path = self.root.join(name);
        fs::write(path, data).map_err(|e| LxmfError::Io(e.to_string()))
    }

    pub fn get(&self, transient_id: &[u8]) -> Result<Vec<u8>, LxmfError> {
        let name = hex::encode(transient_id);
        let path = self.root.join(name);
        fs::read(path).map_err(|e| LxmfError::Io(e.to_string()))
    }

    pub fn list_ids(&self) -> Result<Vec<Vec<u8>>, LxmfError> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(&self.root).map_err(|e| LxmfError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| LxmfError::Io(e.to_string()))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.len() == 64 {
                if let Ok(bytes) = hex::decode(name.as_ref()) {
                    out.push(bytes);
                }
            }
        }
        Ok(out)
    }
}

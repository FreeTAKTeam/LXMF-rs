use super::{AnnounceCacheState, Hash, HashSet, PathTablePersistence};

impl PathTablePersistence {
    pub(in crate::transport) fn fingerprint_map_len_now(&self) -> Option<usize> {
        let state = self.state.try_lock().ok()?;
        state.as_ref().map(|state| state.files.len())
    }
}

impl Default for PathTablePersistence {
    fn default() -> Self {
        Self { save_gate: tokio::sync::Semaphore::new(1), state: tokio::sync::Mutex::new(None) }
    }
}

impl AnnounceCacheState {
    pub(super) fn retain_current_files(&mut self, current_hashes: &HashSet<Hash>) {
        self.files.retain(|hash, _| current_hashes.contains(hash));
    }
}

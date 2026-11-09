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
    pub(super) fn retain_current_files(
        &mut self,
        current_hashes: impl ExactSizeIterator<Item = Hash>,
    ) {
        // Keep this ownership synchronous: the temporary membership set must
        // be dropped before packet-cache or metadata I/O starts.
        let mut retained = HashSet::with_capacity(current_hashes.len());
        retained.extend(current_hashes);
        self.files.retain(|hash, _| retained.contains(hash));
    }
}

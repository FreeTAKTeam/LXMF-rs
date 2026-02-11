use super::*;

impl Router {
    pub fn enable_propagation(&mut self, store_root: &Path, target_cost: u32) {
        let store = PropagationStore::new(store_root);
        self.propagation_service = Some(PropagationService::new(store, target_cost));
    }

    pub fn propagation_enabled(&self) -> bool {
        self.propagation_service.is_some()
    }

    pub fn ingest_propagation(&mut self, bytes: &[u8]) -> Result<usize, LxmfError> {
        let Some(service) = &self.propagation_service else {
            return Ok(0);
        };

        let count = service.ingest(bytes)?;
        self.stats.propagation_ingested_total += count;
        self.last_ingest_count = count;
        Ok(count)
    }

    pub fn fetch_propagated(&self, transient_id: &[u8]) -> Result<Vec<u8>, LxmfError> {
        let Some(service) = &self.propagation_service else {
            return Err(LxmfError::Io("propagation disabled".into()));
        };

        service.fetch(transient_id)
    }

    pub fn propagation_ingested_total(&self) -> usize {
        self.stats.propagation_ingested_total
    }

    pub fn last_ingest_count(&self) -> usize {
        self.last_ingest_count
    }

    pub fn request_propagation_transfer(
        &mut self,
        transient_id: impl Into<Vec<u8>>,
    ) -> PropagationTransferState {
        let now = unix_now();
        let state = PropagationTransferState::requested(transient_id.into(), now);
        self.propagation_transfers.insert(state.transient_id.clone(), state.clone());
        self.stats.propagation_requests_total += 1;
        state
    }

    pub fn update_propagation_transfer_progress(
        &mut self,
        transient_id: &[u8],
        progress: u8,
    ) -> bool {
        let Some(state) = self.propagation_transfers.get_mut(transient_id) else {
            return false;
        };
        state.phase = TransferPhase::InProgress;
        state.progress = progress.min(100);
        state.updated_at = unix_now();
        true
    }

    pub fn complete_propagation_transfer(&mut self, transient_id: &[u8]) -> bool {
        let Some(state) = self.propagation_transfers.get_mut(transient_id) else {
            return false;
        };
        state.phase = TransferPhase::Completed;
        state.progress = 100;
        state.reason = None;
        state.updated_at = unix_now();
        self.stats.propagation_completed_total += 1;
        true
    }

    pub fn cancel_propagation_transfer(
        &mut self,
        transient_id: &[u8],
        reason: impl Into<String>,
    ) -> bool {
        let Some(state) = self.propagation_transfers.get_mut(transient_id) else {
            return false;
        };
        state.phase = TransferPhase::Cancelled;
        state.reason = Some(reason.into());
        state.updated_at = unix_now();
        self.stats.propagation_cancelled_total += 1;
        true
    }

    pub fn propagation_transfer_state(
        &self,
        transient_id: &[u8],
    ) -> Option<&PropagationTransferState> {
        self.propagation_transfers.get(transient_id)
    }
}

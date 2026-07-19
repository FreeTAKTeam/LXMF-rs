impl RpcDaemon {

    fn select_propagation_payloads_for_destination_with_budget_outcome(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
        let destination_hex = hex::encode(destination);
        let per_message_overhead = 16usize;
        let mut cumulative_size = 24usize;
        let mut messages = Vec::new();
        let mut transfer_limited_ids = Vec::new();
        let mut served_ids = HashSet::new();
        for transient_id in wanted {
            if transient_id.len() != 32 {
                continue;
            }
            let transient_hex = hex::encode(transient_id);
            if !served_ids.insert(transient_hex.clone()) {
                continue;
            }
            let stored_entry = match self.store.get_propagation_entry(transient_hex.as_str()) {
                Ok(entry) => entry,
                Err(error) => {
                    log::error!(
                        "failed to read propagation entry transient_id={transient_hex}: {error}"
                    );
                    None
                }
            };
            let payload = match stored_entry
                .filter(|entry| entry.destination == destination_hex)
                .and_then(|entry| match hex::decode(entry.payload_hex.as_str()) {
                    Ok(payload) => Some(payload),
                    Err(error) => {
                        log::error!(
                            "invalid stored propagation payload transient_id={transient_hex}: {error}"
                        );
                        None
                    }
                }) {
                Some(payload) => payload,
                None => {
                    let payload_hex = {
                        let guard = self
                            .propagation_payloads
                            .lock()
                            .expect("propagation payload mutex poisoned");
                        let Some(payload_hex) = guard.get(transient_hex.as_str()) else {
                            continue;
                        };
                        payload_hex.clone()
                    };
                    let Ok(payload) = hex::decode(payload_hex) else {
                        continue;
                    };
                    if !propagation_payload_matches_destination(payload.as_slice(), destination) {
                        continue;
                    }
                    payload
                }
            };
            let stored_size = payload.len().saturating_add(PROPAGATION_STAMP_SIZE);
            let transfer_size = stored_size.saturating_add(per_message_overhead);
            if transfer_limit_bytes.is_some_and(|limit| transfer_size > limit) {
                transfer_limited_ids.push(transient_hex);
                continue;
            }
            let next_size = cumulative_size.saturating_add(transfer_size);
            if transfer_limit_bytes.is_some_and(|limit| next_size > limit) {
                continue;
            }
            cumulative_size = next_size;
            messages.push((transient_hex, payload));
        }

        (messages, transfer_limited_ids)
    }

    pub fn purge_propagation_payloads_for_destination(
        &self,
        destination: &[u8; 16],
        haves: &[Vec<u8>],
    ) -> usize {
        let destination_hex = hex::encode(destination);
        let haves_hex = haves.iter().map(hex::encode).collect::<Vec<_>>();
        let mut removed_snapshot_ids = Vec::new();
        for transient_hex in &haves_hex {
            let entry = match self.store.get_propagation_entry(transient_hex.as_str()) {
                Ok(entry) => entry,
                Err(error) => {
                    log::error!(
                        "failed to read propagation entry during purge transient_id={transient_hex}: {error}"
                    );
                    None
                }
            };
            if entry.is_some_and(|entry| {
                entry.destination.eq_ignore_ascii_case(destination_hex.as_str())
            }) {
                removed_snapshot_ids.push(transient_hex.clone());
            }
        }
        let mut purged = match self
            .store
            .purge_propagation_entries_for_destination(destination_hex.as_str(), &haves_hex)
        {
            Ok(purged) => purged,
            Err(error) => {
                log::error!(
                    "failed to purge propagation entries destination={destination_hex}: {error}"
                );
                0
            }
        };
        {
            let mut guard =
                self.propagation_payloads.lock().expect("propagation payload mutex poisoned");
            for transient_id in haves {
                if transient_id.len() != 32 {
                    continue;
                }
                let transient_hex = hex::encode(transient_id);
                let should_remove = guard
                    .get(transient_hex.as_str())
                    .and_then(|payload_hex| hex::decode(payload_hex).ok())
                    .is_some_and(|payload| {
                        propagation_payload_matches_destination(payload.as_slice(), destination)
                    });
                if should_remove && guard.remove(transient_hex.as_str()).is_some() {
                    purged += 1;
                    if !removed_snapshot_ids
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(&transient_hex))
                    {
                        removed_snapshot_ids.push(transient_hex);
                    }
                }
            }
        }
        for transient_id in removed_snapshot_ids {
            self.remove_peer_queue_snapshot_id(transient_id.as_str());
        }
        purged
    }

    pub fn record_propagation_offer_peer(&self, peer: &str) -> Result<(), std::io::Error> {
        let record = self.ensure_peer_for_sync(peer, now_i64())?;
        self.queue_existing_propagation_for_peer(record.peer.as_str())
    }
}

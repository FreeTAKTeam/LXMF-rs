impl RpcDaemon {

    pub fn ingest_propagation_payload_hex_at_cost(
        &self,
        payload_hex: &str,
        transient_id: Option<&str>,
        stamp_cost: u32,
    ) -> Result<String, std::io::Error> {
        let normalized_payload = if !payload_hex.is_empty() {
            Some(normalize_propagation_payload_hex(payload_hex, stamp_cost)?)
        } else {
            None
        };
        let canonical_transient_id =
            normalized_payload.as_ref().map(|(transient_id, _payload_hex)| transient_id.clone());
        if let (Some(provided_transient_id), Some(canonical_transient_id)) =
            (transient_id, canonical_transient_id.as_ref())
        {
            if !provided_transient_id.eq_ignore_ascii_case(canonical_transient_id) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "transient_id does not match propagation payload",
                ));
            }
        }
        let transient_id =
            transient_id.map(normalize_propagation_transient_key).unwrap_or_else(|| {
                canonical_transient_id.unwrap_or_else(|| {
                    let mut hasher = Sha256::new();
                    hasher.update(payload_hex.as_bytes());
                    encode_hex(hasher.finalize())
                })
            });

        let already_known = if normalized_payload.is_some() && !transient_id.is_empty() {
            let already_stored = self
                .store
                .get_propagation_entry(transient_id.as_str())
                .map_err(std::io::Error::other)?
                .is_some();
            let already_processed = self
                .store
                .local_propagation_processed_mark_exists(transient_id.as_str())
                .map_err(std::io::Error::other)?;
            already_stored || already_processed
        } else {
            false
        };

        if let Some((_canonical_transient_id, payload_hex)) = normalized_payload {
            if hex::decode(payload_hex.as_str())
                .ok()
                .is_some_and(|payload| self.propagation_payload_destination_is_ignored(&payload))
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "ignored propagation destination",
                ));
            }
            self.store_propagation_payload_hex(transient_id.as_str(), payload_hex.as_str())?;
            self.queue_propagation_entry_for_active_peers(transient_id.as_str())?;
            self.propagation_payloads
                .lock()
                .expect("propagation payload mutex poisoned")
                .insert(transient_id.clone(), payload_hex);
            self.store
                .mark_local_propagation_processed(transient_id.as_str())
                .map_err(std::io::Error::other)?;
            self.prune_propagation_payloads_to_storage_limit()?;
        }

        self.note_client_propagation_messages_received(usize::from(
            !payload_hex.is_empty() && !transient_id.is_empty() && !already_known,
        ));

        Ok(transient_id)
    }

    pub fn ingest_propagation_payload_bytes(
        &self,
        payload: &[u8],
        transient_id: Option<&str>,
    ) -> Result<String, std::io::Error> {
        let target_cost =
            self.propagation_state.lock().expect("propagation mutex poisoned").target_cost;
        self.ingest_propagation_payload_bytes_at_cost(payload, transient_id, target_cost)
    }

    pub fn ingest_propagation_payload_bytes_at_cost(
        &self,
        payload: &[u8],
        transient_id: Option<&str>,
        stamp_cost: u32,
    ) -> Result<String, std::io::Error> {
        let payload_hex = hex::encode(payload);
        self.ingest_propagation_payload_hex_at_cost(payload_hex.as_str(), transient_id, stamp_cost)
    }

    pub fn ingest_client_propagation_payload_bytes_at_cost(
        &self,
        payload: &[u8],
        transient_id: Option<&str>,
        stamp_cost: u32,
    ) -> Result<String, std::io::Error> {
        let (canonical_transient_id, normalized_payload) =
            normalize_propagation_payload_bytes(payload, stamp_cost)?;
        let canonical_transient_id = hex::encode(canonical_transient_id);
        if let Some(provided_transient_id) = transient_id {
            if !provided_transient_id.eq_ignore_ascii_case(canonical_transient_id.as_str()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "transient_id does not match propagation payload",
                ));
            }
        }
        let transient_id =
            transient_id.map(normalize_propagation_transient_key).unwrap_or(canonical_transient_id);
        if self.propagation_payload_destination_is_ignored(normalized_payload) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "ignored propagation destination",
            ));
        }
        let already_known = self
            .store
            .get_propagation_entry(transient_id.as_str())
            .map_err(std::io::Error::other)?
            .is_some()
            || self
                .store
                .local_propagation_processed_mark_exists(transient_id.as_str())
                .map_err(std::io::Error::other)?;
        let payload_hex = hex::encode(normalized_payload);
        self.store_propagation_payload_hex(transient_id.as_str(), payload_hex.as_str())?;
        self.propagation_payloads
            .lock()
            .expect("propagation payload mutex poisoned")
            .insert(transient_id.clone(), payload_hex);
        self.store
            .mark_local_propagation_processed(transient_id.as_str())
            .map_err(std::io::Error::other)?;
        self.prune_propagation_payloads_to_storage_limit()?;
        self.note_client_propagation_messages_received(usize::from(!already_known));
        Ok(transient_id)
    }

    fn propagation_payload_destination_is_ignored(&self, payload: &[u8]) -> bool {
        if payload.len() < 16 {
            return false;
        }
        let destination_hex = hex::encode(&payload[..16]);
        self.delivery_policy
            .lock()
            .expect("policy mutex poisoned")
            .ignored_destinations
            .iter()
            .any(|destination| destination_hex.eq_ignore_ascii_case(destination.trim()))
    }

    pub fn ingest_peer_propagation_payload_bytes_at_cost(
        &self,
        payload: &[u8],
        transient_id: Option<&str>,
        stamp_cost: u32,
        source_peer: &str,
    ) -> Result<String, std::io::Error> {
        let source_peer = source_peer.trim().to_ascii_lowercase();
        if source_peer.is_empty() {
            return self.ingest_propagation_payload_bytes_at_cost(
                payload,
                transient_id,
                stamp_cost,
            );
        }
        let (canonical_transient_id, normalized_payload) =
            normalize_propagation_payload_bytes(payload, stamp_cost)?;
        let canonical_transient_id = hex::encode(canonical_transient_id);
        if let Some(provided_transient_id) = transient_id {
            if !provided_transient_id.eq_ignore_ascii_case(canonical_transient_id.as_str()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "transient_id does not match propagation payload",
                ));
            }
        }
        let transient_id =
            transient_id.map(normalize_propagation_transient_key).unwrap_or(canonical_transient_id);
        if self.propagation_payload_destination_is_ignored(normalized_payload) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "ignored propagation destination",
            ));
        }
        let payload_hex = hex::encode(normalized_payload);
        self.store_propagation_payload_hex(transient_id.as_str(), payload_hex.as_str())?;
        let source_active_peer = self
            .active_peer_ids()
            .into_iter()
            .find(|peer| peer.eq_ignore_ascii_case(source_peer.as_str()));
        let source_peer_key = source_active_peer.as_deref().unwrap_or(source_peer.as_str());
        let already_received = self
            .store
            .peer_received_propagation_mark_exists(source_peer_key, transient_id.as_str())
            .map_err(std::io::Error::other)?;
        self.queue_propagation_entry_from_source_for_active_peers(
            source_peer.as_str(),
            transient_id.as_str(),
        )?;
        if !already_received {
            if let Some(peer) = source_active_peer {
                self.record_inbound_peer_activity(peer.as_str(), normalized_payload.len());
            } else {
                self.record_unpeered_propagation_attempt(normalized_payload.len());
            }
        }
        self.propagation_payloads
            .lock()
            .expect("propagation payload mutex poisoned")
            .insert(transient_id.clone(), payload_hex);
        self.prune_propagation_payloads_to_storage_limit()?;
        Ok(transient_id)
    }

    pub fn relay_accepted_peer_propagation_payload_bytes_at_cost(
        &self,
        payload: &[u8],
        transient_id: Option<&str>,
        stamp_cost: u32,
        source_peer: &str,
    ) -> Result<String, std::io::Error> {
        let source_peer = source_peer.trim().to_ascii_lowercase();
        if source_peer.is_empty() {
            return self.ingest_propagation_payload_bytes_at_cost(
                payload,
                transient_id,
                stamp_cost,
            );
        }
        let (canonical_transient_id, normalized_payload) =
            normalize_propagation_payload_bytes(payload, stamp_cost)?;
        let canonical_transient_id = hex::encode(canonical_transient_id);
        if let Some(provided_transient_id) = transient_id {
            if !provided_transient_id.eq_ignore_ascii_case(canonical_transient_id.as_str()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "transient_id does not match propagation payload",
                ));
            }
        }
        let transient_id =
            transient_id.map(normalize_propagation_transient_key).unwrap_or(canonical_transient_id);
        if self.propagation_payload_destination_is_ignored(normalized_payload) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "ignored propagation destination",
            ));
        }
        let payload_hex = hex::encode(normalized_payload);
        self.store_propagation_payload_hex(transient_id.as_str(), payload_hex.as_str())?;
        self.queue_propagation_entry_from_source_for_active_peers(
            source_peer.as_str(),
            transient_id.as_str(),
        )?;
        self.propagation_payloads
            .lock()
            .expect("propagation payload mutex poisoned")
            .insert(transient_id.clone(), payload_hex);
        self.prune_propagation_payloads_to_storage_limit()?;
        Ok(transient_id)
    }

    pub fn has_propagation_payload(&self, transient_id: &str) -> bool {
        if self
            .store
            .get_propagation_entry(normalize_propagation_transient_key(transient_id).as_str())
            .ok()
            .flatten()
            .is_some()
        {
            return true;
        }
        self.propagation_payloads
            .lock()
            .expect("propagation payload mutex poisoned")
            .contains_key(normalize_propagation_transient_key(transient_id).as_str())
    }

    fn peer_store_key_or_input(&self, peer: &str) -> String {
        self.peers
            .lock()
            .expect("peers mutex poisoned")
            .keys()
            .find(|existing| existing.eq_ignore_ascii_case(peer))
            .cloned()
            .unwrap_or_else(|| peer.to_string())
    }

    pub fn record_peer_received_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<(), std::io::Error> {
        let transient_id = normalize_propagation_transient_key(transient_id);
        let peer_key = self.peer_store_key_or_input(peer);
        self.store
            .mark_peer_received_propagation(peer_key.as_str(), transient_id.as_str())
            .map_err(std::io::Error::other)?;
        self.record_peer_queue_handled_id(peer_key.as_str(), transient_id.as_str());
        Ok(())
    }

    pub fn record_existing_peer_received_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<bool, std::io::Error> {
        let transient_id = normalize_propagation_transient_key(transient_id);
        let peer_key = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard.keys().find(|existing| existing.eq_ignore_ascii_case(peer)).cloned()
        };
        let Some(peer_key) = peer_key else {
            return Ok(false);
        };
        self.store
            .mark_peer_received_propagation(peer_key.as_str(), transient_id.as_str())
            .map_err(std::io::Error::other)?;
        self.record_peer_queue_handled_id(peer_key.as_str(), transient_id.as_str());
        Ok(true)
    }

    pub fn record_peer_unhandled_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<(), std::io::Error> {
        let transient_id = normalize_propagation_transient_key(transient_id);
        let peer_key = self.peer_store_key_or_input(peer);
        self.store
            .mark_peer_unhandled_propagation(peer_key.as_str(), transient_id.as_str())
            .map_err(std::io::Error::other)?;
        self.record_peer_queue_unhandled_id(peer_key.as_str(), transient_id.as_str());
        Ok(())
    }

    pub fn has_peer_completed_propagation_mark(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<bool, std::io::Error> {
        let peer_key = self.peer_store_key_or_input(peer);
        self.store
            .peer_completed_propagation_mark_exists(
                peer_key.as_str(),
                normalize_propagation_transient_key(transient_id).as_str(),
            )
            .map_err(std::io::Error::other)
    }

    pub fn has_peer_propagation_mark(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<bool, std::io::Error> {
        let peer_key = self.peer_store_key_or_input(peer);
        self.store
            .peer_propagation_mark_exists(
                peer_key.as_str(),
                normalize_propagation_transient_key(transient_id).as_str(),
            )
            .map_err(std::io::Error::other)
    }

    pub fn record_peer_transferred_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<(), std::io::Error> {
        let transient_id = normalize_propagation_transient_key(transient_id);
        let peer_key = self.peer_store_key_or_input(peer);
        self.store
            .mark_peer_transferred_propagation(peer_key.as_str(), transient_id.as_str())
            .map_err(std::io::Error::other)?;
        self.record_peer_queue_handled_id(peer_key.as_str(), transient_id.as_str());
        Ok(())
    }

    pub fn record_peer_transfer_limited_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<(), std::io::Error> {
        let transient_id = normalize_propagation_transient_key(transient_id);
        let peer_key = self.peer_store_key_or_input(peer);
        self.store
            .mark_peer_transfer_limited_propagation(peer_key.as_str(), transient_id.as_str())
            .map_err(std::io::Error::other)?;
        self.record_peer_queue_handled_id(peer_key.as_str(), transient_id.as_str());
        Ok(())
    }

    pub fn list_propagation_payloads_for_destination(
        &self,
        destination: &[u8; 16],
    ) -> Vec<(Vec<u8>, usize)> {
        let destination_hex = hex::encode(destination);
        let mut entries = self
            .store
            .list_propagation_entries_for_destination(destination_hex.as_str())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| {
                let transient_id = hex::decode(entry.transient_id).ok()?;
                (transient_id.len() == 32).then_some((transient_id, entry.size_bytes as usize))
            })
            .collect::<Vec<_>>();
        let known = entries
            .iter()
            .map(|(transient_id, _)| hex::encode(transient_id))
            .collect::<HashSet<_>>();
        entries.extend(
            self.propagation_payloads
                .lock()
                .expect("propagation payload mutex poisoned")
                .iter()
                .filter_map(|(transient_id, payload_hex)| {
                    if known.contains(transient_id) {
                        return None;
                    }
                    let transient_id = hex::decode(transient_id).ok()?;
                    if transient_id.len() != 32 {
                        return None;
                    }
                    let payload = hex::decode(payload_hex).ok()?;
                    propagation_payload_matches_destination(payload.as_slice(), destination)
                        .then_some((transient_id, payload.len()))
                }),
        );
        entries.sort_by_key(|(_transient_id, size)| *size);
        entries
    }

    pub fn fetch_propagation_payloads_for_destination(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<Vec<u8>> {
        self.fetch_propagation_payloads_for_destination_with_ids(
            destination,
            wanted,
            transfer_limit_bytes,
        )
        .into_iter()
        .map(|(_transient_id, payload)| payload)
        .collect()
    }

    pub fn fetch_propagation_payloads_for_destination_with_ids(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<(String, Vec<u8>)> {
        let messages = self.select_propagation_payloads_for_destination_with_ids(
            destination,
            wanted,
            transfer_limit_bytes,
        );

        if !messages.is_empty() {
            let state = {
                let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
                guard.client_propagation_messages_served =
                    guard.client_propagation_messages_served.saturating_add(messages.len());
                guard.clone()
            };
            self.update_daemon_status_snapshot(|snapshot| {
                snapshot.propagation = state;
            });
        }

        messages
    }

    pub fn preview_propagation_payloads_for_destination_with_ids(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<(String, Vec<u8>)> {
        self.select_propagation_payloads_for_destination_with_ids(
            destination,
            wanted,
            transfer_limit_bytes,
        )
    }

    pub fn transfer_limited_propagation_payload_ids_for_destination(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<String> {
        self.select_propagation_payloads_for_destination_with_budget_outcome(
            destination,
            wanted,
            transfer_limit_bytes,
        )
        .1
    }

    fn select_propagation_payloads_for_destination_with_ids(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<(String, Vec<u8>)> {
        self.select_propagation_payloads_for_destination_with_budget_outcome(
            destination,
            wanted,
            transfer_limit_bytes,
        )
        .0
    }
}

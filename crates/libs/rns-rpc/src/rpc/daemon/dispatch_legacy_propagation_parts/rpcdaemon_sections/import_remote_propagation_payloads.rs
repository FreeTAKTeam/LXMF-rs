impl RpcDaemon {

    fn import_remote_propagation_payloads(
        &self,
        result: &JsonValue,
        operation: &str,
        peer: Option<&str>,
    ) -> Result<RemotePropagationImportSummary, std::io::Error> {
        let Some(messages) = [
            result.get("messages"),
            result.get("payloads"),
            result.get("propagation").and_then(|propagation| propagation.get("messages")),
            result.get("propagation").and_then(|propagation| propagation.get("payloads")),
        ]
        .into_iter()
        .flatten()
        .find_map(JsonValue::as_array) else {
            return Ok(RemotePropagationImportSummary {
                imported_count: 0,
                duplicate_count: 0,
                imported_ids: Vec::new(),
                accepted_ids: Vec::new(),
                transferred_bytes: 0,
            });
        };

        let mut imported_count = 0usize;
        let mut duplicate_count = 0usize;
        let mut imported_ids = Vec::new();
        let mut accepted_ids: Vec<String> = Vec::new();
        let mut transferred_bytes = 0usize;
        let mut validated = Vec::new();
        for message in messages {
            let Some((payload, payload_hex)) = remote_propagation_message_payload(message)? else {
                continue;
            };
            let raw_transient_id = {
                let mut hasher = Sha256::new();
                hasher.update(payload.as_slice());
                encode_hex(hasher.finalize())
            };
            let provided_transient_id = message
                .get("transient_id")
                .and_then(JsonValue::as_str)
                .map(normalize_propagation_transient_key);
            let (transient_id, normalized_payload_hex, normalized_payload_len) =
                match provided_transient_id {
                    Some(transient_id) if transient_id == raw_transient_id => {
                        (transient_id, payload_hex.trim().to_ascii_lowercase(), payload.len())
                    }
                    Some(transient_id) => {
                        let normalized = normalize_propagation_payload_bytes(payload.as_slice(), 0);
                        match normalized {
                            Ok((canonical_transient_id, normalized_payload))
                                if transient_id == hex::encode(canonical_transient_id) =>
                            {
                                (
                                    transient_id,
                                    hex::encode(normalized_payload),
                                    normalized_payload.len(),
                                )
                            }
                            _ => {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidInput,
                                    "transient_id does not match propagation payload",
                                ));
                            }
                        }
                    }
                    None => {
                        (raw_transient_id, payload_hex.trim().to_ascii_lowercase(), payload.len())
                    }
                };
            let normalized_payload = hex::decode(normalized_payload_hex.as_str()).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("invalid remote propagation payload hex: {err}"),
                )
            })?;
            if self.propagation_payload_destination_is_ignored(normalized_payload.as_slice()) {
                self.emit_ignored_propagation_drop_event(
                    normalized_payload.as_slice(),
                    Some(transient_id.as_str()),
                    operation,
                    peer,
                );
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "ignored propagation destination",
                ));
            }
            let destination = message
                .get("destination")
                .and_then(JsonValue::as_str)
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    if normalized_payload.len() >= 16 {
                        hex::encode(&normalized_payload[..16])
                    } else {
                        String::new()
                    }
                });
            let received_at =
                message.get("received_at").and_then(JsonValue::as_i64).unwrap_or_else(now_i64);
            let stamp_value = message
                .get("stamp_value")
                .and_then(JsonValue::as_u64)
                .and_then(|value| u32::try_from(value).ok());
            let record = PropagationEntryRecord {
                transient_id: transient_id.clone(),
                destination,
                payload_hex: normalized_payload_hex,
                received_at,
                size_bytes: normalized_payload_len as u64,
                stamp_value,
            };
            let already_known_store = self
                .store
                .get_propagation_entry(transient_id.as_str())
                .map_err(std::io::Error::other)?
                .is_some();
            let already_processed = self
                .store
                .local_propagation_processed_mark_exists(transient_id.as_str())
                .map_err(std::io::Error::other)?;
            let already_accepted =
                accepted_ids.iter().any(|id| id.eq_ignore_ascii_case(transient_id.as_str()));
            let is_duplicate = already_known_store || already_processed || already_accepted;
            let accept_for_peer_queue = !already_accepted && (!is_duplicate || already_known_store);
            if accept_for_peer_queue {
                transferred_bytes = transferred_bytes.saturating_add(normalized_payload_len);
                accepted_ids.push(transient_id.clone());
            }
            if is_duplicate {
                duplicate_count = duplicate_count.saturating_add(1);
                self.emit_remote_duplicate_propagation_drop_event(
                    normalized_payload.as_slice(),
                    transient_id.as_str(),
                    operation,
                    peer,
                    remote_duplicate_detail(
                        already_known_store,
                        already_processed,
                        already_accepted,
                    ),
                );
            } else {
                imported_count = imported_count.saturating_add(1);
                imported_ids.push(transient_id.clone());
                validated.push(record);
            }
        }
        for record in validated {
            self.store.upsert_propagation_entry(&record).map_err(std::io::Error::other)?;
            self.store
                .mark_local_propagation_processed(record.transient_id.as_str())
                .map_err(std::io::Error::other)?;
            self.propagation_payloads
                .lock()
                .expect("propagation payload mutex poisoned")
                .insert(record.transient_id, record.payload_hex);
        }
        self.prune_propagation_payloads_to_storage_limit()?;
        if !messages.is_empty() {
            self.note_client_propagation_messages_received(imported_count);
        }
        Ok(RemotePropagationImportSummary {
            imported_count,
            duplicate_count,
            imported_ids,
            accepted_ids,
            transferred_bytes,
        })
    }

    fn queue_remote_sync_imports_for_peers(
        &self,
        source_peer: &str,
        imported_ids: &[String],
        transferred_bytes: usize,
    ) -> Result<(), std::io::Error> {
        if imported_ids.is_empty() {
            return Ok(());
        }

        let active_peers = self.active_peer_ids();
        let source_active_peer =
            active_peers.iter().find(|peer| peer.eq_ignore_ascii_case(source_peer)).cloned();
        let source_peer_key = source_active_peer.as_deref().unwrap_or(source_peer);
        let mut source_received_count = 0usize;
        let mut source_received_bytes = 0usize;
        for transient_id in imported_ids {
            let already_received = self
                .store
                .peer_received_propagation_mark_exists(source_peer_key, transient_id.as_str())
                .unwrap_or(false);
            if !already_received {
                source_received_count = source_received_count.saturating_add(1);
                source_received_bytes = source_received_bytes.saturating_add(
                    self.store
                        .get_propagation_entry(transient_id.as_str())
                        .map_err(std::io::Error::other)?
                        .map(|entry| entry.size_bytes as usize)
                        .unwrap_or(0),
                );
            }
            self.store
                .mark_peer_received_propagation(source_peer_key, transient_id.as_str())
                .map_err(std::io::Error::other)?;
            self.record_peer_queue_handled_id(source_peer_key, transient_id.as_str());
            for peer in &active_peers {
                if peer.eq_ignore_ascii_case(source_peer) {
                    continue;
                }
                self.store
                    .mark_peer_unhandled_propagation(peer.as_str(), transient_id.as_str())
                    .map_err(std::io::Error::other)?;
                self.record_peer_queue_unhandled_id(peer.as_str(), transient_id.as_str());
            }
        }
        if source_received_count > 0 {
            self.record_inbound_propagation_peer_activity_count(
                source_peer_key,
                source_received_bytes.min(transferred_bytes),
                source_received_count,
            );
        }
        Ok(())
    }

    fn queue_remote_imports_from_source_for_active_peers(
        &self,
        source_peer: &str,
        imported_ids: &[String],
        transferred_bytes: usize,
    ) -> Result<(), std::io::Error> {
        if imported_ids.is_empty() {
            return Ok(());
        }

        let active_peers = self.active_peer_ids();
        let source_active_peer =
            active_peers.iter().find(|peer| peer.eq_ignore_ascii_case(source_peer)).cloned();
        let source_peer_key = source_active_peer.as_deref().unwrap_or(source_peer);
        let mut source_received_count = 0usize;
        let mut source_received_bytes = 0usize;
        for transient_id in imported_ids {
            let already_received = self
                .store
                .peer_received_propagation_mark_exists(source_peer_key, transient_id.as_str())
                .unwrap_or(false);
            if !already_received {
                source_received_count = source_received_count.saturating_add(1);
                source_received_bytes = source_received_bytes.saturating_add(
                    self.store
                        .get_propagation_entry(transient_id.as_str())
                        .map_err(std::io::Error::other)?
                        .map(|entry| entry.size_bytes as usize)
                        .unwrap_or(0),
                );
            }
            self.store
                .mark_peer_received_propagation(source_peer_key, transient_id.as_str())
                .map_err(std::io::Error::other)?;
            self.record_peer_queue_handled_id(source_peer_key, transient_id.as_str());
            for peer in &active_peers {
                if peer.eq_ignore_ascii_case(source_peer) {
                    continue;
                }
                self.store
                    .mark_peer_unhandled_propagation(peer.as_str(), transient_id.as_str())
                    .map_err(std::io::Error::other)?;
                self.record_peer_queue_unhandled_id(peer.as_str(), transient_id.as_str());
            }
        }
        if source_received_count > 0 {
            self.record_successful_remote_propagation_peer_activity_count(
                source_peer_key,
                source_received_bytes.min(transferred_bytes),
                source_received_count,
            );
        }
        Ok(())
    }

    pub fn note_client_propagation_messages_received(&self, ingested_count: usize) {
        let state = {
            let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
            guard.last_ingest_count = ingested_count;
            guard.total_ingested += ingested_count;
            guard.client_propagation_messages_received =
                guard.client_propagation_messages_received.saturating_add(ingested_count);
            guard.clone()
        };
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.propagation = state;
        });
    }

    pub fn mark_local_propagation_processed(
        &self,
        transient_id: &str,
    ) -> Result<bool, std::io::Error> {
        self.store
            .mark_local_propagation_processed(transient_id)
            .map_err(std::io::Error::other)
    }

    pub fn local_propagation_processed_mark_exists(
        &self,
        transient_id: &str,
    ) -> Result<bool, std::io::Error> {
        self.store
            .local_propagation_processed_mark_exists(transient_id)
            .map_err(std::io::Error::other)
    }

    pub fn canonical_propagation_payload_hex(
        &self,
        payload_hex: &str,
    ) -> Result<String, std::io::Error> {
        let target_cost =
            self.propagation_state.lock().expect("propagation mutex poisoned").target_cost;
        canonical_propagation_transient_hex(payload_hex, target_cost)
    }

    pub fn canonical_propagation_payload_hex_at_cost(
        &self,
        payload_hex: &str,
        stamp_cost: u32,
    ) -> Result<String, std::io::Error> {
        canonical_propagation_transient_hex(payload_hex, stamp_cost)
    }

    pub fn canonical_propagation_payload_bytes(
        &self,
        payload: &[u8],
    ) -> Result<String, std::io::Error> {
        let target_cost =
            self.propagation_state.lock().expect("propagation mutex poisoned").target_cost;
        Ok(hex::encode(canonical_propagation_transient_bytes(payload, target_cost)?))
    }

    pub fn canonical_propagation_payload_bytes_at_cost(
        &self,
        payload: &[u8],
        stamp_cost: u32,
    ) -> Result<String, std::io::Error> {
        Ok(hex::encode(canonical_propagation_transient_bytes(payload, stamp_cost)?))
    }

    pub fn propagation_target_cost(&self) -> u32 {
        self.propagation_state.lock().expect("propagation mutex poisoned").target_cost
    }

    pub fn propagation_min_accepted_stamp_cost(&self) -> u32 {
        let state = self.propagation_state.lock().expect("propagation mutex poisoned");
        state.target_cost.saturating_sub(state.stamp_cost_flexibility)
    }

    pub fn throttle_propagation_peer_for_invalid_stamp(&self, peer: &str) {
        self.throttle_propagation_peer_key(peer);
    }

    fn throttle_propagation_peer_key(&self, peer: &str) {
        let peer = peer.trim().to_ascii_lowercase();
        if peer.is_empty() {
            return;
        }
        self.throttled_propagation_peers
            .lock()
            .expect("throttled propagation peers mutex poisoned")
            .insert(peer, now_i64().saturating_add(PN_STAMP_THROTTLE_SECS));
    }

    pub fn throttle_propagation_peer_offer(&self, peer: &str) {
        if let Some(key) = propagation_offer_throttle_key(peer) {
            self.throttle_propagation_peer_key(key.as_str());
        }
    }

    pub fn propagation_peer_is_throttled(&self, peer: &str) -> bool {
        let peer = peer.trim().to_ascii_lowercase();
        if peer.is_empty() {
            return false;
        }
        let now = now_i64();
        let mut guard = self
            .throttled_propagation_peers
            .lock()
            .expect("throttled propagation peers mutex poisoned");
        match guard.get(peer.as_str()).copied() {
            Some(deadline) if deadline > now => true,
            Some(_) => {
                guard.remove(peer.as_str());
                false
            }
            None => false,
        }
    }

    pub fn propagation_peer_offer_is_throttled(&self, peer: &str) -> bool {
        propagation_offer_throttle_key(peer)
            .is_some_and(|key| self.propagation_peer_is_throttled(key.as_str()))
    }

    pub fn ingest_propagation_payload_bytes_with_aliases(
        &self,
        payload: &[u8],
        transient_id: &str,
        aliases: &[String],
    ) -> Result<String, std::io::Error> {
        let target_cost =
            self.propagation_state.lock().expect("propagation mutex poisoned").target_cost;
        let normalized = if payload.is_empty() {
            None
        } else {
            Some(normalize_propagation_payload_bytes(payload, target_cost)?)
        };
        let transient_id = normalize_propagation_transient_key(transient_id);
        let already_known = if normalized.is_some() && !transient_id.is_empty() {
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
        let has_payload = normalized.is_some();
        if let Some((_canonical_transient_id, payload)) = normalized {
            if self.propagation_payload_destination_is_ignored(payload) {
                self.emit_ignored_propagation_drop_event(
                    payload,
                    Some(transient_id.as_str()),
                    "ingest_propagation_payload_with_aliases",
                    None,
                );
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "ignored propagation destination",
                ));
            }
            let payload_hex = hex::encode(payload);
            self.store_propagation_payload_hex(transient_id.as_str(), payload_hex.as_str())?;
            self.queue_propagation_entry_for_active_peers(transient_id.as_str())?;
            let mut guard =
                self.propagation_payloads.lock().expect("propagation payload mutex poisoned");
            guard.insert(transient_id.clone(), payload_hex.clone());
            for alias in aliases {
                self.store_propagation_payload_hex(alias, payload_hex.as_str())?;
                guard.insert(normalize_propagation_transient_key(alias), payload_hex.clone());
            }
            self.store
                .mark_local_propagation_processed(transient_id.as_str())
                .map_err(std::io::Error::other)?;
            drop(guard);
            self.prune_propagation_payloads_to_storage_limit()?;
        }

        let state = {
            let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
            let ingested_count =
                usize::from(has_payload && !transient_id.is_empty() && !already_known);
            guard.last_ingest_count = ingested_count;
            guard.total_ingested += ingested_count;
            guard.client_propagation_messages_received =
                guard.client_propagation_messages_received.saturating_add(ingested_count);
            guard.clone()
        };
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.propagation = state;
        });

        Ok(transient_id)
    }

    pub fn ingest_propagation_payload_hex(
        &self,
        payload_hex: &str,
        transient_id: Option<&str>,
    ) -> Result<String, std::io::Error> {
        let target_cost =
            self.propagation_state.lock().expect("propagation mutex poisoned").target_cost;
        self.ingest_propagation_payload_hex_at_cost(payload_hex, transient_id, target_cost)
    }

    fn emit_remote_duplicate_propagation_drop_event(
        &self,
        payload: &[u8],
        transient_id: &str,
        operation: &str,
        peer: Option<&str>,
        detail: &'static str,
    ) {
        let raw_destination_hex = payload.get(..16).map(hex::encode).unwrap_or_default();
        let resolved_destination_hex = raw_destination_hex.clone();
        let mut event_payload = json!({
            "reason": "duplicate",
            "delivery_kind": "propagation",
            "raw_destination_hash": raw_destination_hex,
            "resolved_destination_hash": resolved_destination_hex,
            "payload_mode": "full_wire",
            "bytes_len": payload.len(),
            "detail": detail,
            "operation": operation,
            "transient_id": transient_id,
        });
        if let Some(peer) = peer.filter(|value| !value.trim().is_empty()) {
            event_payload["peer"] = JsonValue::String(peer.to_string());
        }
        self.publish_event(RpcEvent {
            event_type: "inbound_dropped".to_string(),
            payload: event_payload,
        });
    }
}

fn remote_duplicate_detail(
    already_known_store: bool,
    already_processed: bool,
    already_accepted: bool,
) -> &'static str {
    if already_accepted {
        "duplicate propagation payload in remote response"
    } else if already_processed {
        "remote propagation payload already processed locally"
    } else if already_known_store {
        "remote propagation payload already stored locally"
    } else {
        "remote propagation payload duplicate"
    }
}

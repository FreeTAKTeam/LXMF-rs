use super::*;

const PN_STAMP_THROTTLE_SECS: i64 = 180;

const PR_REQUEST_SENT: u32 = 0x04;
const PR_COMPLETE: u32 = 0x07;
const PR_IDLE: u32 = 0x00;
const PR_FAILED: u32 = 0xfe;

struct RemotePropagationImportSummary {
    imported_count: usize,
    duplicate_count: usize,
    imported_ids: Vec<String>,
    accepted_ids: Vec<String>,
    transferred_bytes: usize,
}

impl RpcDaemon {
    fn publish_failed_remote_peer_sync_event(
        &self,
        peer_id: &str,
        remote: &str,
        error: &str,
        transfer_limit: Option<u64>,
        sync_limit: Option<u64>,
        postpone_reason: Option<&str>,
    ) {
        let peer = self.peers.lock().expect("peers mutex poisoned").get(peer_id).cloned();
        let Some(peer) = peer else {
            return;
        };
        let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes) =
            self.peer_message_stats(peer.peer.as_str()).unwrap_or((0, 0, 0, 0, 0, 0));
        let handled_ids =
            self.store.list_peer_handled_propagation_ids(peer.peer.as_str()).unwrap_or_default();
        let unhandled_ids =
            self.store.list_peer_unhandled_propagation_ids(peer.peer.as_str()).unwrap_or_default();
        let peering_key = super::dispatch_legacy_messages::peer_peering_key_value(
            &peer,
            self.identity_hash.as_str(),
        );
        let peering_key_status =
            super::dispatch_legacy_messages::peer_peering_key_status(&peer, peering_key);
        let acceptance_rate = super::dispatch_legacy_messages::peer_acceptance_rate_for_reporting(
            peer.acceptance_rate,
            outgoing,
            offered,
            peer.alive,
        );
        let peer_status_type =
            if self.is_static_peer(peer.peer.as_str()) { "static" } else { "discovered" };
        let messages = json!({
            "offered": offered,
            "outgoing": outgoing,
            "incoming": incoming,
            "unhandled": unhandled,
            "offered_bytes": offered_bytes,
            "unhandled_bytes": unhandled_bytes,
            "handled_ids": handled_ids,
            "unhandled_ids": unhandled_ids,
        });
        let mut propagation = json!({
            "remote_sync": true,
            "synced": false,
            "error": error,
            "rejected": 0,
            "rejected_bytes": 0,
            "rejected_ids": [],
            "peering_key": peering_key,
            "peering_key_status": peering_key_status,
            "transfer_limit": transfer_limit,
            "sync_limit": sync_limit,
        });
        if let Some(reason) = postpone_reason {
            propagation["postponed"] = json!(true);
            propagation["postpone_reason"] = json!(reason);
        }
        let mut payload = json!({
            "peer": peer.peer,
            "peer_type": peer.peer_type,
            "type": peer_status_type,
            "timestamp": now_i64(),
            "name": peer.name,
            "name_source": peer.name_source,
            "remote": remote,
            "remote_sync": true,
            "synced": false,
            "state": 0,
            "sync_strategy": peer.sync_strategy,
            "ler": 0,
            "peering_timebase": peer.peering_timebase,
            "network_distance": peer.network_distance,
            "alive": peer.alive,
            "last_heard": peer.last_seen,
            "first_seen": peer.first_seen,
            "seen_count": peer.seen_count,
            "rx_bytes": peer.rx_bytes,
            "tx_bytes": peer.tx_bytes,
            "acceptance_rate": acceptance_rate,
            "last_sync_attempt": peer.last_sync_attempt,
            "next_sync_attempt": peer.next_sync_attempt,
            "sync_backoff": peer.sync_backoff,
            "sync_transfer_rate": peer.sync_transfer_rate,
            "str": peer.sync_transfer_rate as u64,
            "propagation_transfer_limit": peer.propagation_transfer_limit,
            "propagation_sync_limit": peer.propagation_sync_limit,
            "propagation_stamp_cost": peer.propagation_stamp_cost,
            "propagation_stamp_cost_flexibility": peer.propagation_stamp_cost_flexibility,
            "peering_key": peering_key,
            "peering_key_status": peering_key_status,
            "transfer_limit": transfer_limit,
            "sync_limit": sync_limit,
            "target_stamp_cost": peer.propagation_stamp_cost,
            "stamp_cost_flexibility": peer.propagation_stamp_cost_flexibility,
            "offered": offered,
            "outgoing": outgoing,
            "incoming": incoming,
            "messages": messages,
            "propagation": propagation,
        });
        if let Some(reason) = postpone_reason {
            payload["postponed"] = json!(true);
            payload["postpone_reason"] = json!(reason);
        }
        self.publish_event(RpcEvent { event_type: "peer_sync".into(), payload });
    }

    fn record_throttled_remote_peer_sync(
        &self,
        peer_id: &str,
        remote: &str,
        error: &str,
        transfer_limit: Option<u64>,
        sync_limit: Option<u64>,
    ) -> Result<(), std::io::Error> {
        let timestamp = now_i64();
        if let Ok(mut peers) = self.peers.lock() {
            if let Some(peer) = peers.get_mut(peer_id) {
                peer.last_sync_attempt = timestamp;
                peer.next_sync_attempt = timestamp.saturating_add(PN_STAMP_THROTTLE_SECS);
            }
        }
        self.record_payload_backed_peer_queue_snapshot(peer_id)?;
        self.publish_failed_remote_peer_sync_event(
            peer_id,
            remote,
            error,
            transfer_limit,
            sync_limit,
            Some("throttled"),
        );
        Ok(())
    }

    fn record_retryable_remote_peer_sync_error(
        &self,
        peer_id: &str,
        remote: &str,
        error: &str,
        transfer_limit: Option<u64>,
        sync_limit: Option<u64>,
    ) -> Result<(), std::io::Error> {
        let timestamp = now_i64();
        if let Ok(mut peers) = self.peers.lock() {
            if let Some(peer) = peers.get_mut(peer_id) {
                peer.last_sync_attempt = timestamp;
                peer.next_sync_attempt = 0;
            }
        }
        self.record_payload_backed_peer_queue_snapshot(peer_id)?;
        self.publish_failed_remote_peer_sync_event(
            peer_id,
            remote,
            error,
            transfer_limit,
            sync_limit,
            None,
        );
        Ok(())
    }

    fn break_remote_peer_sync_peering_on_denied_access(
        &self,
        peer_id: &str,
        remote: &str,
        error: &str,
    ) -> Result<(), std::io::Error> {
        let cleanup = self.unpeer_local_state(peer_id)?;
        let offered = cleanup.messages["offered"].as_u64().unwrap_or(0);
        let outgoing = cleanup.messages["outgoing"].as_u64().unwrap_or(0);
        let incoming = cleanup.messages["incoming"].as_u64().unwrap_or(0);
        self.publish_event(RpcEvent {
            event_type: "peer_unpeer".into(),
            payload: json!({
                "peer": peer_id,
                "remote": remote,
                "removed": cleanup.removed,
                "reason": "access_denied",
                "error": error,
                "propagation_cleared": cleanup.propagation_cleared,
                "propagation_cleared_bytes": cleanup.propagation_cleared_bytes,
                "offered": offered,
                "outgoing": outgoing,
                "incoming": incoming,
                "messages": cleanup.messages,
            }),
        });
        Ok(())
    }

    fn store_propagation_payload_hex(
        &self,
        transient_id: &str,
        payload_hex: &str,
    ) -> Result<(), std::io::Error> {
        let payload = hex::decode(payload_hex).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid propagation payload hex: {err}"),
            )
        })?;
        let destination =
            if payload.len() >= 16 { hex::encode(&payload[..16]) } else { String::new() };
        self.store
            .upsert_propagation_entry(&PropagationEntryRecord {
                transient_id: normalize_propagation_transient_key(transient_id),
                destination,
                payload_hex: payload_hex.to_ascii_lowercase(),
                received_at: now_i64(),
                size_bytes: payload.len() as u64,
                stamp_value: None,
            })
            .map_err(std::io::Error::other)
    }

    fn queue_propagation_entry_for_active_peers(
        &self,
        transient_id: &str,
    ) -> Result<(), std::io::Error> {
        for peer in self.active_peer_ids() {
            self.store
                .mark_peer_unhandled_propagation(peer.as_str(), transient_id)
                .map_err(std::io::Error::other)?;
            self.record_peer_queue_unhandled_id(peer.as_str(), transient_id);
        }
        Ok(())
    }

    fn queue_propagation_entry_from_source_for_active_peers(
        &self,
        source_peer: &str,
        transient_id: &str,
    ) -> Result<(), std::io::Error> {
        let source_peer = source_peer.trim().to_ascii_lowercase();
        let active_peers = self.active_peer_ids();
        let source_peer_key = active_peers
            .iter()
            .find(|peer| peer.eq_ignore_ascii_case(source_peer.as_str()))
            .map(String::as_str)
            .unwrap_or(source_peer.as_str());
        self.store
            .mark_peer_received_propagation(source_peer_key, transient_id)
            .map_err(std::io::Error::other)?;
        self.record_peer_queue_handled_id(source_peer_key, transient_id);
        for peer in active_peers {
            if peer.eq_ignore_ascii_case(source_peer.as_str()) {
                self.record_peer_queue_handled_id(peer.as_str(), transient_id);
            } else {
                self.store
                    .mark_peer_unhandled_propagation(peer.as_str(), transient_id)
                    .map_err(std::io::Error::other)?;
                self.record_peer_queue_unhandled_id(peer.as_str(), transient_id);
            }
        }
        Ok(())
    }

    fn import_remote_propagation_payloads(
        &self,
        result: &JsonValue,
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
            let canonical_transient_id = {
                let mut hasher = Sha256::new();
                hasher.update(payload.as_slice());
                encode_hex(hasher.finalize())
            };
            let transient_id = message
                .get("transient_id")
                .and_then(JsonValue::as_str)
                .map(normalize_propagation_transient_key)
                .unwrap_or_else(|| canonical_transient_id.clone());
            if transient_id != canonical_transient_id {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "transient_id does not match propagation payload",
                ));
            }
            let destination = message
                .get("destination")
                .and_then(JsonValue::as_str)
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    if payload.len() >= 16 {
                        hex::encode(&payload[..16])
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
                payload_hex: payload_hex.trim().to_ascii_lowercase(),
                received_at,
                size_bytes: payload.len() as u64,
                stamp_value,
            };
            let already_known_store = self
                .store
                .get_propagation_entry(transient_id.as_str())
                .map_err(std::io::Error::other)?
                .is_some();
            let already_accepted =
                accepted_ids.iter().any(|id| id.eq_ignore_ascii_case(transient_id.as_str()));
            if !already_accepted {
                transferred_bytes = transferred_bytes.saturating_add(payload.len());
                accepted_ids.push(transient_id.clone());
            }
            if already_known_store || already_accepted {
                duplicate_count = duplicate_count.saturating_add(1);
            } else {
                imported_count = imported_count.saturating_add(1);
                imported_ids.push(transient_id.clone());
            }
            validated.push(record);
        }
        for record in validated {
            self.store.upsert_propagation_entry(&record).map_err(std::io::Error::other)?;
            self.propagation_payloads
                .lock()
                .expect("propagation payload mutex poisoned")
                .insert(record.transient_id, record.payload_hex);
        }
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
            self.record_inbound_propagation_peer_activity_count(
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
            self.store
                .get_propagation_entry(transient_id.as_str())
                .map_err(std::io::Error::other)?
                .is_some()
        } else {
            false
        };
        let has_payload = normalized.is_some();
        if let Some((_canonical_transient_id, payload)) = normalized {
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
            self.store
                .get_propagation_entry(transient_id.as_str())
                .map_err(std::io::Error::other)?
                .is_some()
        } else {
            false
        };

        if let Some((_canonical_transient_id, payload_hex)) = normalized_payload {
            self.store_propagation_payload_hex(transient_id.as_str(), payload_hex.as_str())?;
            self.queue_propagation_entry_for_active_peers(transient_id.as_str())?;
            self.propagation_payloads
                .lock()
                .expect("propagation payload mutex poisoned")
                .insert(transient_id.clone(), payload_hex);
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
            let payload = match self
                .store
                .get_propagation_entry(transient_hex.as_str())
                .ok()
                .flatten()
                .filter(|entry| entry.destination == destination_hex)
                .and_then(|entry| hex::decode(entry.payload_hex.as_str()).ok())
            {
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
            if self.store.get_propagation_entry(transient_hex.as_str()).ok().flatten().is_some_and(
                |entry| entry.destination.eq_ignore_ascii_case(destination_hex.as_str()),
            ) {
                removed_snapshot_ids.push(transient_hex.clone());
            }
        }
        let mut purged = self
            .store
            .purge_propagation_entries_for_destination(destination_hex.as_str(), &haves_hex)
            .unwrap_or_default();
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

    pub(super) fn handle_rpc_legacy_propagation(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "get_delivery_policy" => {
                let policy = self.delivery_policy.lock().expect("policy mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "policy": policy })),
                    error: None,
                })
            }
            "set_delivery_policy" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: DeliveryPolicyParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let policy = {
                    let mut guard = self.delivery_policy.lock().expect("policy mutex poisoned");
                    if let Some(value) = parsed.auth_required {
                        guard.auth_required = value;
                    }
                    if let Some(value) = parsed.allowed_destinations {
                        guard.allowed_destinations = value;
                    }
                    if let Some(value) = parsed.denied_destinations {
                        guard.denied_destinations = value;
                    }
                    if let Some(value) = parsed.ignored_destinations {
                        guard.ignored_destinations = value;
                    }
                    if let Some(value) = parsed.prioritised_destinations {
                        guard.prioritised_destinations = value;
                    }
                    guard.clone()
                };
                self.update_daemon_status_snapshot(|snapshot| {
                    snapshot.delivery_policy = policy.clone();
                });

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "policy": policy })),
                    error: None,
                })
            }
            "propagation_status" => {
                let state =
                    self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "propagation": state })),
                    error: None,
                })
            }
            "propagation_peer_maintenance" => {
                let timestamp = now_i64();
                let culled_peers = self.cull_unreachable_non_static_peers(timestamp)?;
                let rotated_peers = self.rotate_low_acceptance_non_static_peers()?;
                let synced_peer = self.select_peer_for_maintenance_sync(timestamp)?;
                let peer_sync = if let Some(peer) = synced_peer.as_ref() {
                    self.handle_rpc(RpcRequest {
                        id: request.id,
                        method: "peer_sync".to_string(),
                        params: Some(json!({ "peer": peer, "maintenance_claimed": true })),
                    })?
                    .result
                    .unwrap_or(JsonValue::Null)
                } else {
                    JsonValue::Null
                };
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "timestamp": timestamp,
                        "culled": culled_peers.len(),
                        "culled_peers": culled_peers,
                        "rotated": rotated_peers.len(),
                        "rotated_peers": rotated_peers,
                        "synced_peer": synced_peer,
                        "peer_sync": peer_sync,
                        "max_unreachable_secs": super::init::LXMF_PEER_MAX_UNREACHABLE_SECS,
                    })),
                    error: None,
                })
            }
            "propagation_enable" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationEnableParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let mut static_peers_to_activate = None;
                let mut state = {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    guard.enabled = parsed.enabled;
                    if parsed.store_root.is_some() {
                        guard.store_root = parsed.store_root;
                    }
                    if let Some(cost) = parsed.target_cost {
                        guard.target_cost = cost;
                    }
                    if let Some(flexibility) = parsed.stamp_cost_flexibility {
                        guard.stamp_cost_flexibility = flexibility;
                    }
                    if let Some(limit) = parsed.message_storage_limit_mb {
                        guard.message_storage_limit_mb = (limit > 0).then_some(limit);
                    }
                    if let Some(limit) = parsed.delivery_limit {
                        guard.delivery_limit = limit;
                    }
                    if let Some(limit) = parsed.propagation_limit {
                        guard.propagation_limit = limit;
                    }
                    if let Some(limit) = parsed.sync_limit {
                        guard.sync_limit = limit.max(guard.propagation_limit);
                    } else if guard.sync_limit < guard.propagation_limit {
                        guard.sync_limit = guard.propagation_limit;
                    }
                    if let Some(autopeer) = parsed.autopeer {
                        guard.autopeer = autopeer;
                    }
                    if let Some(autopeer_maxdepth) = parsed.autopeer_maxdepth {
                        guard.autopeer_maxdepth = autopeer_maxdepth;
                    }
                    if let Some(static_peers) = parsed.static_peers {
                        let static_peers = Self::normalize_static_peers(&static_peers);
                        static_peers_to_activate = Some(static_peers.clone());
                        guard.static_peers = static_peers;
                    }
                    if let Some(max_peers) = parsed.max_peers {
                        guard.max_peers = Some(max_peers);
                    }
                    if let Some(from_static_only) = parsed.from_static_only {
                        guard.from_static_only = from_static_only;
                    }
                    if let Some(peering_cost) = parsed.peering_cost {
                        guard.peering_cost = Some(peering_cost);
                    }
                    if let Some(remote_peering_cost_max) = parsed.remote_peering_cost_max {
                        guard.remote_peering_cost_max = Some(remote_peering_cost_max);
                    }
                    guard.clone()
                };
                self.update_daemon_status_snapshot(|snapshot| {
                    snapshot.propagation = state.clone();
                });
                if let Some(static_peers_to_activate) = static_peers_to_activate {
                    self.activate_static_peers(&static_peers_to_activate)?;
                }
                self.enforce_autopeer_enabled_policy()?;
                self.enforce_autopeer_maxdepth_policy()?;
                self.enforce_static_only_peer_policy()?;
                state = self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                let selected_node_rejected = {
                    let selected = self
                        .outbound_propagation_node
                        .lock()
                        .expect("propagation node mutex poisoned")
                        .clone();
                    let propagation =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    selected.as_deref().is_some_and(|peer| {
                        propagation.from_static_only
                            && !propagation
                                .static_peers
                                .iter()
                                .any(|candidate| candidate.eq_ignore_ascii_case(peer))
                    })
                };
                if selected_node_rejected {
                    {
                        let mut guard = self
                            .outbound_propagation_node
                            .lock()
                            .expect("propagation node mutex poisoned");
                        *guard = None;
                    }
                    state = {
                        let mut guard =
                            self.propagation_state.lock().expect("propagation mutex poisoned");
                        guard.selected_node = None;
                        guard.clone()
                    };
                    self.update_daemon_status_snapshot(|snapshot| {
                        snapshot.propagation = state.clone();
                    });
                }
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "propagation": state })),
                    error: None,
                })
            }
            "propagation_ingest" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationIngestParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let payload_hex = parsed.payload_hex.unwrap_or_default();
                let target_cost =
                    self.propagation_state.lock().expect("propagation mutex poisoned").target_cost;
                let normalized_payload = if !payload_hex.is_empty() {
                    Some(normalize_propagation_payload_hex(payload_hex.as_str(), target_cost)?)
                } else {
                    None
                };
                if let (Some(provided_transient_id), Some((canonical_transient_id, _payload_hex))) =
                    (parsed.transient_id.as_ref(), normalized_payload.as_ref())
                {
                    if !provided_transient_id.eq_ignore_ascii_case(canonical_transient_id) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "transient_id does not match propagation payload",
                        ));
                    }
                }
                let transient_id = parsed
                    .transient_id
                    .map(|value| normalize_propagation_transient_key(value.as_str()))
                    .unwrap_or_else(|| {
                        normalized_payload
                            .as_ref()
                            .map(|(transient_id, _payload_hex)| transient_id.clone())
                            .unwrap_or_else(|| {
                                let mut hasher = Sha256::new();
                                hasher.update(payload_hex.as_bytes());
                                encode_hex(hasher.finalize())
                            })
                    });
                let already_known = if !payload_hex.is_empty() && !transient_id.is_empty() {
                    self.store
                        .get_propagation_entry(transient_id.as_str())
                        .map_err(std::io::Error::other)?
                        .is_some()
                } else {
                    false
                };
                let has_payload = normalized_payload.is_some();
                let payload_bytes = normalized_payload
                    .as_ref()
                    .and_then(|(_transient_id, payload_hex)| hex::decode(payload_hex).ok())
                    .map(|payload| payload.len())
                    .unwrap_or(0);
                let ingested_count =
                    usize::from(has_payload && !transient_id.is_empty() && !already_known);
                let duplicate_count =
                    usize::from(has_payload && !transient_id.is_empty() && already_known);

                if let Some(payload_hex) =
                    normalized_payload.map(|(_transient_id, payload_hex)| payload_hex)
                {
                    self.store_propagation_payload_hex(
                        transient_id.as_str(),
                        payload_hex.as_str(),
                    )?;
                    self.queue_propagation_entry_for_active_peers(transient_id.as_str())?;
                    self.propagation_payloads
                        .lock()
                        .expect("propagation payload mutex poisoned")
                        .insert(transient_id.clone(), payload_hex);
                }

                let state = {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    guard.last_ingest_count = ingested_count;
                    guard.total_ingested += ingested_count;
                    guard.client_propagation_messages_received =
                        guard.client_propagation_messages_received.saturating_add(ingested_count);
                    guard.clone()
                };
                self.update_daemon_status_snapshot(|snapshot| {
                    snapshot.propagation = state.clone();
                });

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "ingested_count": state.last_ingest_count,
                        "duplicate_count": duplicate_count,
                        "payload_bytes": payload_bytes,
                        "transferred_bytes": payload_bytes,
                        "transient_id": transient_id,
                    })),
                    error: None,
                })
            }
            "propagation_fetch" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationFetchParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let normalized_transient_id =
                    normalize_propagation_transient_key(parsed.transient_id.as_str());
                let payload = self
                    .propagation_payloads
                    .lock()
                    .expect("propagation payload mutex poisoned")
                    .get(normalized_transient_id.as_str())
                    .cloned()
                    .or_else(|| {
                        self.store
                            .get_propagation_entry(normalized_transient_id.as_str())
                            .ok()
                            .flatten()
                            .map(|entry| entry.payload_hex)
                    })
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "transient_id not found")
                    })?;
                let payload_bytes =
                    hex::decode(payload.as_str()).map(|bytes| bytes.len()).unwrap_or(0);
                {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    guard.client_propagation_messages_served =
                        guard.client_propagation_messages_served.saturating_add(1);
                    let state = guard.clone();
                    drop(guard);
                    self.update_daemon_status_snapshot(|snapshot| {
                        snapshot.propagation = state;
                    });
                }

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "transient_id": normalized_transient_id,
                        "payload_hex": payload,
                        "payload_bytes": payload_bytes,
                        "transferred_bytes": payload_bytes,
                    })),
                    error: None,
                })
            }
            "get_outbound_propagation_node" => {
                let selected = self
                    .outbound_propagation_node
                    .lock()
                    .expect("propagation node mutex poisoned")
                    .clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "peer": selected,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "set_outbound_propagation_node" => {
                let parsed = request
                    .params
                    .map(serde_json::from_value::<SetOutboundPropagationNodeParams>)
                    .transpose()
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let requested_peer = parsed
                    .and_then(|value| value.peer)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                let peer = if let Some(peer_id) = requested_peer.as_deref() {
                    let record = self.ensure_peer_for_sync(peer_id, now_i64())?;
                    self.queue_existing_propagation_for_peer(record.peer.as_str())?;
                    Some(record.peer)
                } else {
                    None
                };
                {
                    let mut guard = self
                        .outbound_propagation_node
                        .lock()
                        .expect("propagation node mutex poisoned");
                    *guard = peer.clone();
                }
                let state = {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    guard.selected_node = peer.clone();
                    guard.clone()
                };
                self.update_daemon_status_snapshot(|snapshot| {
                    snapshot.propagation = state;
                });
                let event = RpcEvent {
                    event_type: "propagation_node_selected".into(),
                    payload: json!({ "peer": peer }),
                };
                self.publish_event(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "peer": peer,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "list_propagation_nodes" => {
                let selected = self
                    .outbound_propagation_node
                    .lock()
                    .expect("propagation node mutex poisoned")
                    .clone();
                let announces =
                    self.store.list_announces(500, None, None).map_err(std::io::Error::other)?;
                let mut by_peer: HashMap<String, PropagationNodeRecord> = HashMap::new();
                for announce in announces {
                    if !announce.capabilities.iter().any(|cap| cap == "propagation") {
                        continue;
                    }

                    let key = announce.peer.clone();
                    let entry =
                        by_peer.entry(key.clone()).or_insert_with(|| PropagationNodeRecord {
                            peer: key.clone(),
                            name: announce.name.clone(),
                            last_seen: announce.timestamp,
                            capabilities: announce.capabilities.clone(),
                            selected: selected.as_deref() == Some(key.as_str()),
                        });
                    if announce.timestamp > entry.last_seen {
                        entry.last_seen = announce.timestamp;
                        entry.name = announce.name.clone();
                        entry.capabilities = announce.capabilities.clone();
                    }
                    if selected.as_deref() == Some(key.as_str()) {
                        entry.selected = true;
                    }
                }
                if let Some(selected) = selected.as_ref() {
                    by_peer.entry(selected.clone()).or_insert_with(|| PropagationNodeRecord {
                        peer: selected.clone(),
                        name: None,
                        last_seen: 0,
                        capabilities: vec!["propagation".to_string()],
                        selected: true,
                    });
                }

                let mut nodes = by_peer.into_values().collect::<Vec<_>>();
                nodes.sort_by(|a, b| {
                    b.last_seen.cmp(&a.last_seen).then_with(|| a.peer.cmp(&b.peer))
                });
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "nodes": nodes,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "propagation_remote_status" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationRemoteStatusParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let remote_id = parsed.remote.trim().to_string();
                if remote_id.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "remote is required",
                    ));
                }
                let bridge = self
                    .remote_control_bridge
                    .lock()
                    .expect("remote control bridge mutex poisoned")
                    .clone()
                    .ok_or_else(|| std::io::Error::other("remote control bridge unavailable"))?;
                let timeout_secs = parsed.timeout_secs.unwrap_or(5.0).max(0.1);
                let result = bridge.propagation_remote_status(
                    remote_id.as_str(),
                    parsed.identity_private_key_hex.as_deref(),
                    timeout_secs,
                )?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "remote": remote_id,
                        "status": result,
                    })),
                    error: None,
                })
            }
            "propagation_remote_sync" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationRemotePeerParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let remote_id = parsed.remote.trim().to_string();
                if remote_id.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "remote is required",
                    ));
                }
                let peer_id = parsed.peer.trim().to_string();
                if peer_id.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "peer is required",
                    ));
                }
                let timestamp = now_i64();
                let timeout_secs = parsed.timeout_secs.unwrap_or(5.0).max(0.1);
                let existing_record = {
                    self.peers
                        .lock()
                        .expect("peers mutex poisoned")
                        .values()
                        .find(|record| record.peer.eq_ignore_ascii_case(peer_id.as_str()))
                        .cloned()
                };
                if let Some(record) = existing_record.as_ref() {
                    let peer_transfer_limit_kb =
                        record.propagation_transfer_limit.map(|limit| f64::from(limit) / 1000.0);
                    let request_transfer_limit_kb =
                        parsed.transfer_limit_kb.map(|limit| limit.max(0.0));
                    let transfer_limit_kb = effective_transfer_limit_kb(
                        peer_transfer_limit_kb,
                        request_transfer_limit_kb,
                    );
                    let transfer_limit =
                        transfer_limit_kb.map(|limit| (limit.max(0.0) * 1000.0) as u64);
                    let sync_limit =
                        record.propagation_sync_limit.map(u64::from).or(transfer_limit);
                    if super::dispatch_legacy_messages::peer_sync_backoff_active(
                        timestamp,
                        record.next_sync_attempt,
                    ) {
                        self.record_payload_backed_peer_queue_snapshot(record.peer.as_str())?;
                        return Ok(self.postponed_peer_sync_response(
                            request.id,
                            record,
                            timestamp,
                            "backoff",
                            transfer_limit.map(|limit| limit as usize),
                            sync_limit.map(|limit| limit as usize),
                        ));
                    }
                }
                let bridge = match self
                    .remote_control_bridge
                    .lock()
                    .expect("remote control bridge mutex poisoned")
                    .clone()
                {
                    Some(bridge) => bridge,
                    None => {
                        if let Some(record) = existing_record.as_ref() {
                            let peer_transfer_limit_kb = record
                                .propagation_transfer_limit
                                .map(|limit| f64::from(limit) / 1000.0);
                            let request_transfer_limit_kb =
                                parsed.transfer_limit_kb.map(|limit| limit.max(0.0));
                            let transfer_limit_kb = effective_transfer_limit_kb(
                                peer_transfer_limit_kb,
                                request_transfer_limit_kb,
                            );
                            let transfer_limit =
                                transfer_limit_kb.map(|limit| (limit.max(0.0) * 1000.0) as u64);
                            let sync_limit =
                                record.propagation_sync_limit.map(u64::from).or(transfer_limit);
                            self.update_propagation_sync_state(|state| {
                                state.sync_state = PR_FAILED;
                                state.state_name = "failed".to_string();
                                state.sync_progress = 0.0;
                                state.last_sync_started = Some(timestamp);
                                state.last_sync_completed = None;
                                state.last_sync_error =
                                    Some("remote control bridge unavailable".to_string());
                            });
                            self.record_payload_backed_peer_queue_snapshot(record.peer.as_str())?;
                            self.publish_failed_remote_peer_sync_event(
                                record.peer.as_str(),
                                remote_id.as_str(),
                                "remote control bridge unavailable",
                                transfer_limit,
                                sync_limit,
                                None,
                            );
                        }
                        return Err(std::io::Error::other("remote control bridge unavailable"));
                    }
                };
                let record = self.ensure_peer_for_sync(peer_id.as_str(), timestamp)?;
                let peer_transfer_limit_kb =
                    record.propagation_transfer_limit.map(|limit| f64::from(limit) / 1000.0);
                let request_transfer_limit_kb =
                    parsed.transfer_limit_kb.map(|limit| limit.max(0.0));
                let transfer_limit_kb =
                    effective_transfer_limit_kb(peer_transfer_limit_kb, request_transfer_limit_kb);
                let transfer_limit =
                    transfer_limit_kb.map(|limit| (limit.max(0.0) * 1000.0) as u64);
                let sync_limit = record.propagation_sync_limit.map(u64::from).or(transfer_limit);
                let peer_key = record.peer.clone();
                if super::dispatch_legacy_messages::peer_sync_backoff_active(
                    timestamp,
                    record.next_sync_attempt,
                ) {
                    return Ok(self.postponed_peer_sync_response(
                        request.id,
                        &record,
                        timestamp,
                        "backoff",
                        transfer_limit.map(|limit| limit as usize),
                        sync_limit.map(|limit| limit as usize),
                    ));
                }
                self.update_propagation_sync_state(|state| {
                    state.sync_state = PR_REQUEST_SENT;
                    state.state_name = "syncing".to_string();
                    state.sync_progress = 0.0;
                    state.last_sync_started = Some(now_i64());
                    state.last_sync_completed = None;
                    state.last_sync_error = None;
                });
                let mut peer_sync_result = JsonValue::Null;
                let result = match bridge.propagation_remote_sync(
                    remote_id.as_str(),
                    peer_key.as_str(),
                    parsed.identity_private_key_hex.as_deref(),
                    timeout_secs,
                    transfer_limit_kb,
                ) {
                    Ok(mut result) => {
                        let imported = match self.import_remote_propagation_payloads(&result) {
                            Ok(imported) => imported,
                            Err(err) => {
                                self.update_propagation_sync_state(|state| {
                                    state.sync_state = PR_FAILED;
                                    state.state_name = "failed".to_string();
                                    state.sync_progress = 0.0;
                                    state.last_sync_error = Some(err.to_string());
                                });
                                self.record_outbound_peer_activity(peer_key.as_str(), 0, false);
                                self.record_payload_backed_peer_queue_snapshot(peer_key.as_str())?;
                                self.publish_failed_remote_peer_sync_event(
                                    peer_key.as_str(),
                                    remote_id.as_str(),
                                    err.to_string().as_str(),
                                    transfer_limit,
                                    sync_limit,
                                    None,
                                );
                                return Err(err);
                            }
                        };
                        if let Some(result) = result.as_object_mut() {
                            result.insert(
                                "imported_count".to_string(),
                                json!(imported.imported_count),
                            );
                            result.insert(
                                "duplicate_count".to_string(),
                                json!(imported.duplicate_count),
                            );
                            result.insert("imported_ids".to_string(), json!(imported.imported_ids));
                            result.insert(
                                "transferred_bytes".to_string(),
                                json!(imported.transferred_bytes),
                            );
                        }
                        self.queue_remote_sync_imports_for_peers(
                            peer_key.as_str(),
                            imported.accepted_ids.as_slice(),
                            imported.transferred_bytes,
                        )?;
                        for active_peer in self.active_peer_ids() {
                            self.record_payload_backed_peer_queue_snapshot(active_peer.as_str())?;
                        }
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_COMPLETE;
                            state.state_name = "completed".to_string();
                            state.sync_progress = 1.0;
                            state.last_sync_completed = Some(now_i64());
                            state.last_sync_error = None;
                        });
                        let peer_sync_completed_at = now_i64();
                        if let Ok(mut peers) = self.peers.lock() {
                            if let Some(peer) = peers
                                .values_mut()
                                .find(|record| record.peer.eq_ignore_ascii_case(peer_key.as_str()))
                            {
                                peer.alive = true;
                                peer.last_seen = peer_sync_completed_at;
                                peer.last_sync_attempt = peer_sync_completed_at;
                                peer.sync_backoff = 0;
                                peer.next_sync_attempt = 0;
                            }
                        }
                        let peer = self
                            .peers
                            .lock()
                            .expect("peers mutex poisoned")
                            .values()
                            .find(|record| record.peer.eq_ignore_ascii_case(peer_key.as_str()))
                            .cloned();
                        if let Some(peer) = peer {
                            let (
                                outgoing,
                                incoming,
                                offered,
                                unhandled,
                                offered_bytes,
                                unhandled_bytes,
                            ) = self
                                .peer_message_stats(peer.peer.as_str())
                                .unwrap_or((0, 0, 0, 0, 0, 0));
                            let handled_ids = self
                                .store
                                .list_peer_handled_propagation_ids(peer.peer.as_str())
                                .unwrap_or_default();
                            let unhandled_ids = self
                                .store
                                .list_peer_unhandled_propagation_ids(peer.peer.as_str())
                                .unwrap_or_default();
                            let peering_key =
                                super::dispatch_legacy_messages::peer_peering_key_value(
                                    &peer,
                                    self.identity_hash.as_str(),
                                );
                            let peering_key_status =
                                super::dispatch_legacy_messages::peer_peering_key_status(
                                    &peer,
                                    peering_key,
                                );
                            let acceptance_rate =
                                super::dispatch_legacy_messages::peer_acceptance_rate_for_reporting(
                                    peer.acceptance_rate,
                                    outgoing,
                                    offered,
                                    peer.alive,
                                );
                            let messages = json!({
                                "offered": offered,
                                "outgoing": outgoing,
                                "incoming": incoming,
                                "unhandled": unhandled,
                                "offered_bytes": offered_bytes,
                                "unhandled_bytes": unhandled_bytes,
                                "handled_ids": handled_ids,
                                "unhandled_ids": unhandled_ids,
                            });
                            let propagation = json!({
                                "remote_sync": true,
                                "synced": result.get("synced").and_then(JsonValue::as_bool).unwrap_or(true),
                                "imported_count": imported.imported_count,
                                "duplicate_count": imported.duplicate_count,
                                "imported_ids": imported.imported_ids,
                                "transferred_bytes": imported.transferred_bytes,
                                "rejected": 0,
                                "rejected_bytes": 0,
                                "rejected_ids": [],
                                "peering_key": peering_key,
                                "peering_key_status": peering_key_status,
                                "transfer_limit": transfer_limit,
                                "sync_limit": sync_limit,
                            });
                            let peer_status_type = if self.is_static_peer(peer.peer.as_str()) {
                                "static"
                            } else {
                                "discovered"
                            };
                            let peer_sync = json!({
                                "peer": peer.peer,
                                "peer_type": peer.peer_type,
                                "type": peer_status_type,
                                "timestamp": now_i64(),
                                "name": peer.name,
                                "name_source": peer.name_source,
                                "remote": remote_id.as_str(),
                                "remote_sync": true,
                                "synced": true,
                                "state": 0,
                                "sync_strategy": peer.sync_strategy,
                                "ler": 0,
                                "peering_timebase": peer.peering_timebase,
                                "network_distance": peer.network_distance,
                                "alive": peer.alive,
                                "last_heard": peer.last_seen,
                                "first_seen": peer.first_seen,
                                "seen_count": peer.seen_count,
                                "rx_bytes": peer.rx_bytes,
                                "tx_bytes": peer.tx_bytes,
                                "acceptance_rate": acceptance_rate,
                                "last_sync_attempt": peer.last_sync_attempt,
                                "next_sync_attempt": peer.next_sync_attempt,
                                "sync_backoff": peer.sync_backoff,
                                "sync_transfer_rate": peer.sync_transfer_rate,
                                "str": peer.sync_transfer_rate as u64,
                                "propagation_transfer_limit": peer.propagation_transfer_limit,
                                "propagation_sync_limit": peer.propagation_sync_limit,
                                "propagation_stamp_cost": peer.propagation_stamp_cost,
                                "propagation_stamp_cost_flexibility": peer.propagation_stamp_cost_flexibility,
                                "peering_key": peering_key,
                                "peering_key_status": peering_key_status,
                                "transfer_limit": transfer_limit,
                                "sync_limit": sync_limit,
                                "target_stamp_cost": peer.propagation_stamp_cost,
                                "stamp_cost_flexibility": peer.propagation_stamp_cost_flexibility,
                                "offered": offered,
                                "outgoing": outgoing,
                                "incoming": incoming,
                                "messages": messages,
                                "propagation": propagation,
                            });
                            peer_sync_result = peer_sync.clone();
                            self.publish_event(RpcEvent {
                                event_type: "peer_sync".into(),
                                payload: peer_sync,
                            });
                        }
                        result
                    }
                    Err(err) => {
                        let error = err.to_string();
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_FAILED;
                            state.state_name = "failed".to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_error = Some(error.clone());
                        });
                        if err.kind() == std::io::ErrorKind::WouldBlock {
                            self.record_throttled_remote_peer_sync(
                                peer_key.as_str(),
                                remote_id.as_str(),
                                error.as_str(),
                                transfer_limit,
                                sync_limit,
                            )?;
                        } else if is_retryable_remote_peer_sync_error(&err) {
                            self.record_retryable_remote_peer_sync_error(
                                peer_key.as_str(),
                                remote_id.as_str(),
                                error.as_str(),
                                transfer_limit,
                                sync_limit,
                            )?;
                        } else if is_remote_access_denied_error(&err) {
                            self.break_remote_peer_sync_peering_on_denied_access(
                                peer_key.as_str(),
                                remote_id.as_str(),
                                error.as_str(),
                            )?;
                        } else {
                            self.record_outbound_peer_activity(peer_key.as_str(), 0, false);
                            self.record_payload_backed_peer_queue_snapshot(peer_key.as_str())?;
                            self.publish_failed_remote_peer_sync_event(
                                peer_key.as_str(),
                                remote_id.as_str(),
                                error.as_str(),
                                transfer_limit,
                                sync_limit,
                                None,
                            );
                        }
                        return Err(err);
                    }
                };
                let propagation =
                    self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "remote": remote_id,
                        "peer": peer_key,
                        "propagation": propagation,
                        "peer_sync": peer_sync_result,
                        "result": result,
                    })),
                    error: None,
                })
            }
            "propagation_remote_download" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationRemoteStatusParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let remote_id = parsed.remote.trim().to_string();
                if remote_id.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "remote is required",
                    ));
                }
                let bridge = match self
                    .remote_control_bridge
                    .lock()
                    .expect("remote control bridge mutex poisoned")
                    .clone()
                {
                    Some(bridge) => bridge,
                    None => {
                        let timestamp = now_i64();
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_FAILED;
                            state.state_name = "failed".to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_started = Some(timestamp);
                            state.last_sync_completed = None;
                            state.last_sync_error =
                                Some("remote control bridge unavailable".to_string());
                        });
                        for peer in self.active_peer_ids() {
                            let _ = self.record_payload_backed_peer_queue_snapshot(peer.as_str());
                        }
                        return Err(std::io::Error::other("remote control bridge unavailable"));
                    }
                };
                let timeout_secs = parsed.timeout_secs.unwrap_or(5.0).max(0.1);
                self.update_propagation_sync_state(|state| {
                    state.sync_state = PR_REQUEST_SENT;
                    state.state_name = "downloading".to_string();
                    state.sync_progress = 0.0;
                    state.last_sync_started = Some(now_i64());
                    state.last_sync_completed = None;
                    state.last_sync_error = None;
                });
                let result = match bridge.propagation_remote_download(
                    remote_id.as_str(),
                    parsed.identity_private_key_hex.as_deref(),
                    timeout_secs,
                    parsed.transfer_limit_kb,
                ) {
                    Ok(mut result) => {
                        let imported = match self.import_remote_propagation_payloads(&result) {
                            Ok(imported) => imported,
                            Err(err) => {
                                self.update_propagation_sync_state(|state| {
                                    state.sync_state = PR_FAILED;
                                    state.state_name = "failed".to_string();
                                    state.sync_progress = 0.0;
                                    state.last_sync_error = Some(err.to_string());
                                });
                                for peer in self.active_peer_ids() {
                                    self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                                }
                                return Err(err);
                            }
                        };
                        if let Some(result) = result.as_object_mut() {
                            result.insert(
                                "imported_count".to_string(),
                                json!(imported.imported_count),
                            );
                            result.insert(
                                "duplicate_count".to_string(),
                                json!(imported.duplicate_count),
                            );
                            result.insert("imported_ids".to_string(), json!(imported.imported_ids));
                            result.insert(
                                "transferred_bytes".to_string(),
                                json!(imported.transferred_bytes),
                            );
                        }
                        self.queue_remote_imports_from_source_for_active_peers(
                            remote_id.as_str(),
                            imported.accepted_ids.as_slice(),
                            imported.transferred_bytes,
                        )?;
                        for peer in self.active_peer_ids() {
                            self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                        }
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_COMPLETE;
                            state.state_name = "completed".to_string();
                            state.sync_progress = 1.0;
                            state.last_sync_completed = Some(now_i64());
                            state.last_sync_error = None;
                        });
                        result
                    }
                    Err(err) => {
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_FAILED;
                            state.state_name = "failed".to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_error = Some(err.to_string());
                        });
                        for peer in self.active_peer_ids() {
                            self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                        }
                        return Err(err);
                    }
                };
                let propagation =
                    self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "remote": remote_id,
                        "propagation": propagation,
                        "result": result,
                    })),
                    error: None,
                })
            }
            "propagation_acknowledge_sync_completion" => {
                let parsed = request
                    .params
                    .map(serde_json::from_value::<PropagationAcknowledgeSyncParams>)
                    .transpose()
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
                    .unwrap_or_default();
                let state = {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    if parsed.reset_state || guard.sync_state <= PR_COMPLETE {
                        guard.sync_state = parsed.failure_state.unwrap_or(PR_IDLE);
                        guard.state_name =
                            propagation_sync_state_name(guard.sync_state).to_string();
                        if guard.sync_state == PR_IDLE {
                            guard.last_sync_error = None;
                        }
                    }
                    guard.sync_progress = 0.0;
                    guard.clone()
                };
                self.update_daemon_status_snapshot(|snapshot| {
                    snapshot.propagation = state.clone();
                });
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "propagation": state })),
                    error: None,
                })
            }
            "propagation_remote_fetch" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationRemoteFetchParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let remote_id = parsed.remote.trim().to_string();
                if remote_id.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "remote is required",
                    ));
                }
                let bridge = match self
                    .remote_control_bridge
                    .lock()
                    .expect("remote control bridge mutex poisoned")
                    .clone()
                {
                    Some(bridge) => bridge,
                    None => {
                        let timestamp = now_i64();
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_FAILED;
                            state.state_name = "failed".to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_started = Some(timestamp);
                            state.last_sync_completed = None;
                            state.last_sync_error =
                                Some("remote control bridge unavailable".to_string());
                        });
                        for peer in self.active_peer_ids() {
                            self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                        }
                        return Err(std::io::Error::other("remote control bridge unavailable"));
                    }
                };
                let timeout_secs = parsed.timeout_secs.unwrap_or(8.0).max(0.1);
                self.update_propagation_sync_state(|state| {
                    state.sync_state = PR_REQUEST_SENT;
                    state.state_name = "fetching".to_string();
                    state.sync_progress = 0.0;
                    state.last_sync_started = Some(now_i64());
                    state.last_sync_completed = None;
                    state.last_sync_error = None;
                });
                let mut result = match bridge.propagation_remote_fetch(
                    remote_id.as_str(),
                    parsed.identity_private_key_hex.as_deref(),
                    timeout_secs,
                    parsed.transfer_limit_kb,
                ) {
                    Ok(result) => result,
                    Err(err) => {
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_FAILED;
                            state.state_name = "failed".to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_error = Some(err.to_string());
                        });
                        for peer in self.active_peer_ids() {
                            self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                        }
                        return Err(err);
                    }
                };
                let imported = match self.import_remote_propagation_payloads(&result) {
                    Ok(imported) => imported,
                    Err(err) => {
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_FAILED;
                            state.state_name = "failed".to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_error = Some(err.to_string());
                        });
                        for peer in self.active_peer_ids() {
                            self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                        }
                        return Err(err);
                    }
                };
                if let Some(result) = result.as_object_mut() {
                    result.insert("imported_count".to_string(), json!(imported.imported_count));
                    result.insert("duplicate_count".to_string(), json!(imported.duplicate_count));
                    result.insert("imported_ids".to_string(), json!(imported.imported_ids));
                    result
                        .insert("transferred_bytes".to_string(), json!(imported.transferred_bytes));
                }
                self.queue_remote_imports_from_source_for_active_peers(
                    remote_id.as_str(),
                    imported.accepted_ids.as_slice(),
                    imported.transferred_bytes,
                )?;
                for peer in self.active_peer_ids() {
                    self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                }
                self.update_propagation_sync_state(|state| {
                    state.sync_state = PR_COMPLETE;
                    state.state_name = "completed".to_string();
                    state.sync_progress = 1.0;
                    state.last_sync_completed = Some(now_i64());
                    state.last_sync_error = None;
                });
                let propagation =
                    self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "remote": remote_id,
                        "propagation": propagation,
                        "result": result,
                    })),
                    error: None,
                })
            }
            "propagation_remote_unpeer" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationRemotePeerParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let remote_id = parsed.remote.trim().to_string();
                if remote_id.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "remote is required",
                    ));
                }
                let peer_id = parsed.peer.trim();
                if peer_id.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "peer is required",
                    ));
                }
                let snapshot_peer = {
                    let guard = self.peers.lock().expect("peers mutex poisoned");
                    guard
                        .values()
                        .find(|record| record.peer.eq_ignore_ascii_case(peer_id))
                        .map(|record| record.peer.clone())
                        .unwrap_or_else(|| peer_id.to_string())
                };
                let bridge = match self
                    .remote_control_bridge
                    .lock()
                    .expect("remote control bridge mutex poisoned")
                    .clone()
                {
                    Some(bridge) => bridge,
                    None => {
                        let timestamp = now_i64();
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_FAILED;
                            state.state_name = "failed".to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_started = Some(timestamp);
                            state.last_sync_completed = None;
                            state.last_sync_error =
                                Some("remote control bridge unavailable".to_string());
                        });
                        let _ =
                            self.record_payload_backed_peer_queue_snapshot(snapshot_peer.as_str());
                        return Err(std::io::Error::other("remote control bridge unavailable"));
                    }
                };
                let timeout_secs = parsed.timeout_secs.unwrap_or(5.0).max(0.1);
                let result = match bridge.propagation_remote_unpeer(
                    remote_id.as_str(),
                    snapshot_peer.as_str(),
                    parsed.identity_private_key_hex.as_deref(),
                    timeout_secs,
                ) {
                    Ok(result) => result,
                    Err(err) => {
                        let timestamp = now_i64();
                        let error = err.to_string();
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_FAILED;
                            state.state_name = "failed".to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_started = Some(timestamp);
                            state.last_sync_completed = None;
                            state.last_sync_error = Some(error);
                        });
                        let _ =
                            self.record_payload_backed_peer_queue_snapshot(snapshot_peer.as_str());
                        return Err(err);
                    }
                };
                let cleanup = self.unpeer_local_state(peer_id)?;
                let offered = cleanup.messages["offered"].as_u64().unwrap_or(0);
                let outgoing = cleanup.messages["outgoing"].as_u64().unwrap_or(0);
                let incoming = cleanup.messages["incoming"].as_u64().unwrap_or(0);
                self.publish_event(RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: json!({
                        "peer": cleanup.peer.as_str(),
                        "remote": remote_id.as_str(),
                        "removed": cleanup.removed,
                        "propagation_cleared": cleanup.propagation_cleared,
                        "propagation_cleared_bytes": cleanup.propagation_cleared_bytes,
                        "offered": offered,
                        "outgoing": outgoing,
                        "incoming": incoming,
                        "messages": cleanup.messages,
                        "result": result,
                    }),
                });
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "remote": remote_id,
                        "peer": cleanup.peer.as_str(),
                        "removed": cleanup.removed,
                        "propagation_cleared": cleanup.propagation_cleared,
                        "propagation_cleared_bytes": cleanup.propagation_cleared_bytes,
                        "offered": offered,
                        "outgoing": outgoing,
                        "incoming": incoming,
                        "messages": cleanup.messages,
                        "result": result,
                    })),
                    error: None,
                })
            }
            _ => unreachable!("legacy propagation route: {}", request.method),
        }
    }
}

fn propagation_sync_state_name(state: u32) -> &'static str {
    match state {
        PR_IDLE => "idle",
        PR_REQUEST_SENT => "syncing",
        PR_COMPLETE => "completed",
        PR_FAILED => "failed",
        0x01 => "path_requested",
        0x02 => "link_establishing",
        0x03 => "link_established",
        0x05 => "receiving",
        0x06 => "response_received",
        0xf0 => "no_path",
        0xf1 => "link_failed",
        0xf2 => "transfer_failed",
        0xf3 => "no_identity",
        0xf4 => "no_access",
        _ => "unknown",
    }
}

fn effective_transfer_limit_kb(
    peer_transfer_limit_kb: Option<f64>,
    request_transfer_limit_kb: Option<f64>,
) -> Option<f64> {
    match (peer_transfer_limit_kb, request_transfer_limit_kb) {
        (Some(peer_limit), Some(request_limit)) => Some(peer_limit.min(request_limit)),
        (Some(peer_limit), None) => Some(peer_limit),
        (None, Some(request_limit)) => Some(request_limit),
        (None, None) => None,
    }
}

fn remote_propagation_message_payload(
    message: &JsonValue,
) -> Result<Option<(Vec<u8>, String)>, std::io::Error> {
    if let Some(payload_hex) = message.get("payload_hex").and_then(JsonValue::as_str) {
        let payload = hex::decode(payload_hex.trim()).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid remote propagation payload hex: {err}"),
            )
        })?;
        return Ok(Some((payload, payload_hex.trim().to_ascii_lowercase())));
    }

    for field in ["payload", "payload_bytes"] {
        if let Some(value) = message.get(field) {
            if let Some(payload) = remote_propagation_byte_array(value, field)? {
                let payload_hex = hex::encode(payload.as_slice());
                return Ok(Some((payload, payload_hex)));
            }
        }
    }

    Ok(None)
}

fn remote_propagation_byte_array(
    value: &JsonValue,
    field: &str,
) -> Result<Option<Vec<u8>>, std::io::Error> {
    let Some(items) = value.as_array() else {
        return if field == "payload_bytes" && value.as_u64().is_some() {
            Ok(None)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid remote propagation {field} byte array"),
            ))
        };
    };
    items
        .iter()
        .map(|item| item.as_u64().and_then(|value| u8::try_from(value).ok()))
        .collect::<Option<Vec<_>>>()
        .map(Some)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid remote propagation {field} byte array"),
            )
        })
}

fn is_remote_access_denied_error(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::PermissionDenied
        && err.to_string() == "propagation node denied access"
}

fn is_retryable_remote_peer_sync_error(err: &std::io::Error) -> bool {
    matches!(
        (err.kind(), err.to_string().as_str()),
        (std::io::ErrorKind::PermissionDenied, "propagation node requires identity")
            | (std::io::ErrorKind::PermissionDenied, "propagation peer invalid peering key")
            | (std::io::ErrorKind::PermissionDenied, "propagation peer invalid stamp")
            | (std::io::ErrorKind::InvalidInput, "propagation node rejected the request")
            | (std::io::ErrorKind::InvalidData, "unexpected propagation control response")
            | (std::io::ErrorKind::NotFound, "propagation peer not found")
            | (std::io::ErrorKind::TimedOut, "propagation peer timed out")
    )
}

fn normalize_propagation_transient_key(transient_id: &str) -> String {
    transient_id.trim().to_ascii_lowercase()
}

const PROPAGATION_STAMP_SIZE: usize = 32;
const PROPAGATION_STAMP_WORKBLOCK_ROUNDS: usize = 1000;
// Python rejects propagation-stamped payloads that cannot contain a minimally
// structured LXMF message before validating the trailing stamp.
const MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE: usize = 112 + PROPAGATION_STAMP_SIZE;

pub(super) fn normalize_propagation_payload_hex(
    payload_hex: &str,
    target_cost: u32,
) -> Result<(String, String), std::io::Error> {
    let transient_data = decode_propagation_payload_hex(payload_hex)?;
    let (transient_id, payload) =
        normalize_propagation_payload_bytes(&transient_data, target_cost)?;
    Ok((hex::encode(transient_id), hex::encode(payload)))
}

pub(super) fn canonical_propagation_transient_hex(
    payload_hex: &str,
    target_cost: u32,
) -> Result<String, std::io::Error> {
    let transient_data = decode_propagation_payload_hex(payload_hex)?;
    let transient_id = canonical_propagation_transient_bytes(&transient_data, target_cost)?;
    Ok(hex::encode(transient_id))
}

pub(super) fn decode_propagation_payload_hex(payload_hex: &str) -> Result<Vec<u8>, std::io::Error> {
    hex::decode(payload_hex.trim()).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid propagation payload hex: {err}"),
        )
    })
}

pub(super) fn canonical_propagation_transient_bytes(
    transient_data: &[u8],
    target_cost: u32,
) -> Result<[u8; 32], std::io::Error> {
    if target_cost == 0 {
        let transient_hash =
            Sha256::digest(propagation_payload_hash_input(transient_data, target_cost)?);
        let mut transient_id = [0u8; 32];
        transient_id.copy_from_slice(transient_hash.as_slice());
        return Ok(transient_id);
    }

    if transient_data.len() <= MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "invalid propagation stamp",
        ));
    }

    let split_at = transient_data.len() - PROPAGATION_STAMP_SIZE;
    let lxm_data = &transient_data[..split_at];
    let stamp = &transient_data[split_at..];

    let transient_hash = Sha256::digest(lxm_data);
    let workblock = propagation_stamp_workblock(transient_hash.as_slice());
    if !propagation_stamp_valid(stamp, target_cost, workblock.as_slice()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "invalid propagation stamp",
        ));
    }

    let mut transient_id = [0u8; 32];
    transient_id.copy_from_slice(transient_hash.as_slice());
    Ok(transient_id)
}

pub(super) fn normalize_propagation_payload_bytes(
    transient_data: &[u8],
    target_cost: u32,
) -> Result<([u8; 32], &[u8]), std::io::Error> {
    let lxm_data = propagation_payload_hash_input(transient_data, target_cost)?;

    let transient_hash = Sha256::digest(lxm_data);
    let mut transient_id = [0u8; 32];
    transient_id.copy_from_slice(transient_hash.as_slice());
    Ok((transient_id, lxm_data))
}

pub(super) fn propagation_payload_hash_input(
    transient_data: &[u8],
    target_cost: u32,
) -> Result<&[u8], std::io::Error> {
    if target_cost == 0 {
        return Ok(split_propagation_stamp(transient_data)
            .map(|(lxm_data, _stamp)| lxm_data)
            .unwrap_or(transient_data));
    }

    let (lxm_data, stamp) = split_propagation_stamp(transient_data).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "invalid propagation stamp")
    })?;

    let transient_hash = Sha256::digest(lxm_data);
    let workblock = propagation_stamp_workblock(transient_hash.as_slice());
    if !propagation_stamp_valid(stamp, target_cost, workblock.as_slice()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "invalid propagation stamp",
        ));
    }

    Ok(lxm_data)
}

fn propagation_offer_throttle_key(peer: &str) -> Option<String> {
    let peer = peer.trim().to_ascii_lowercase();
    if peer.is_empty() {
        return None;
    }
    Some(format!("offer:{peer}"))
}

pub(super) fn split_propagation_stamp(transient_data: &[u8]) -> Option<(&[u8], &[u8])> {
    if transient_data.len() <= MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE {
        return None;
    }

    let split_at = transient_data.len() - PROPAGATION_STAMP_SIZE;
    Some((&transient_data[..split_at], &transient_data[split_at..]))
}

fn propagation_payload_matches_destination(payload: &[u8], destination: &[u8; 16]) -> bool {
    payload.len() >= 16 && &payload[..16] == destination
}

pub(super) fn propagation_stamp_workblock(material: &[u8]) -> Vec<u8> {
    let mut workblock = Vec::with_capacity(PROPAGATION_STAMP_WORKBLOCK_ROUNDS * 256);
    for round in 0..PROPAGATION_STAMP_WORKBLOCK_ROUNDS {
        let mut salt_data = Vec::with_capacity(material.len() + 8);
        salt_data.extend_from_slice(material);
        let packed =
            rmp_serde::to_vec(&(round as u32)).expect("msgpack encode propagation stamp round");
        salt_data.extend_from_slice(&packed);
        let salt_hash = Sha256::digest(&salt_data);
        let hk = hkdf::Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), material);
        let mut okm = [0u8; 256];
        hk.expand(&[], &mut okm).expect("hkdf expand propagation stamp workblock");
        workblock.extend_from_slice(&okm);
    }
    workblock
}

pub(super) fn propagation_stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    propagation_stamp_value(workblock, stamp) >= target_cost
}

pub(super) fn propagation_stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
    let mut material = Vec::with_capacity(workblock.len() + stamp.len());
    material.extend_from_slice(workblock);
    material.extend_from_slice(stamp);
    let hash = Sha256::digest(&material);
    let mut value = 0u32;
    for byte in hash {
        if byte == 0 {
            value += 8;
        } else {
            value += byte.leading_zeros();
            break;
        }
    }
    value
}

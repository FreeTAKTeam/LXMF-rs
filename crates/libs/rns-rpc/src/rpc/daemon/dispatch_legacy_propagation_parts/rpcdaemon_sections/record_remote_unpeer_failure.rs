impl RpcDaemon {

    fn record_remote_unpeer_failure(&self, error: String) {
        let timestamp = now_i64();
        let mut state = self.propagation_state.lock().expect("propagation mutex poisoned");
        state.sync_state = PR_FAILED;
        state.state_name = "failed".to_string();
        state.sync_progress = 0.0;
        state.last_sync_started = Some(timestamp);
        state.last_sync_completed = None;
        state.last_sync_error = Some(error);
        let snapshot = state.clone();
        *self.remote_unpeer_failure_state.lock().expect("remote unpeer failure mutex poisoned") =
            Some(snapshot.clone());
        drop(state);
        self.update_daemon_status_snapshot(|status| {
            status.propagation = snapshot;
        });
    }

    fn clear_matching_remote_unpeer_failure(&self, prior_failure: Option<PropagationState>) {
        let Some(prior_failure) = prior_failure else {
            return;
        };
        let mut state = self.propagation_state.lock().expect("propagation mutex poisoned");
        let mut recorded_failure =
            self.remote_unpeer_failure_state.lock().expect("remote unpeer failure mutex poisoned");
        if recorded_failure.as_ref() != Some(&prior_failure) || *state != prior_failure {
            return;
        }
        *recorded_failure = None;
        drop(recorded_failure);
        state.sync_state = PR_IDLE;
        state.state_name = propagation_sync_state_name(PR_IDLE).to_string();
        state.sync_progress = 0.0;
        state.last_sync_completed = Some(now_i64());
        state.last_sync_error = None;
        let snapshot = state.clone();
        drop(state);
        self.update_daemon_status_snapshot(|status| {
            status.propagation = snapshot;
        });
    }

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
        propagation["state"] = json!(super::dispatch_legacy_messages::PEER_SYNC_STATE_FAILED);
        propagation["state_name"] = json!("failed");
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
        let failure_kind = remote_peer_sync_failure_kind(error, postpone_reason);
        payload["failure_kind"] = json!(failure_kind);
        payload["propagation"]["failure_kind"] = json!(failure_kind);
        payload["state"] = json!(super::dispatch_legacy_messages::PEER_SYNC_STATE_FAILED);
        payload["state_name"] = json!("failed");
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
                peer.sync_backoff =
                    peer.sync_backoff.saturating_add(super::init::LXMF_PEER_SYNC_BACKOFF_STEP_SECS);
                peer.next_sync_attempt = timestamp.saturating_add(i64::from(peer.sync_backoff));
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

    fn record_failed_remote_transfer_for_active_source_peer(
        &self,
        source_peer: &str,
        remote: &str,
        error: &std::io::Error,
    ) -> Result<bool, std::io::Error> {
        if is_retryable_remote_peer_sync_error(error) {
            let source_peer_key = self
                .active_peer_ids()
                .into_iter()
                .find(|peer| peer.eq_ignore_ascii_case(source_peer));
            let Some(source_peer_key) = source_peer_key else {
                return Ok(false);
            };
            self.record_retryable_remote_peer_sync_error(
                source_peer_key.as_str(),
                remote,
                error.to_string().as_str(),
                None,
                None,
            )?;
            return Ok(true);
        }

        if !is_remote_transfer_attempt_error(error) {
            return Ok(false);
        }

        self.record_failed_remote_import_for_active_source_peer(source_peer, remote, error)
    }

    fn record_failed_remote_import_for_active_source_peer(
        &self,
        source_peer: &str,
        remote: &str,
        error: &std::io::Error,
    ) -> Result<bool, std::io::Error> {
        let source_peer_key =
            self.active_peer_ids().into_iter().find(|peer| peer.eq_ignore_ascii_case(source_peer));
        let Some(source_peer_key) = source_peer_key else {
            return Ok(false);
        };

        self.record_outbound_peer_activity(source_peer_key.as_str(), 0, false);
        self.record_payload_backed_peer_queue_snapshot(source_peer_key.as_str())?;
        self.publish_failed_remote_peer_sync_event(
            source_peer_key.as_str(),
            remote,
            error.to_string().as_str(),
            None,
            None,
            None,
        );
        Ok(true)
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
                "peer": cleanup.peer,
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

    fn prune_propagation_payloads_to_storage_limit(&self) -> Result<(), std::io::Error> {
        let Some(limit_mb) = self
            .propagation_state
            .lock()
            .expect("propagation mutex poisoned")
            .message_storage_limit_mb
        else {
            return Ok(());
        };
        let Some(limit_bytes) = limit_mb.checked_mul(1_000_000) else {
            return Ok(());
        };
        let prioritised_destinations = self
            .delivery_policy
            .lock()
            .expect("policy mutex poisoned")
            .prioritised_destinations
            .clone();
        let pruned = self
            .store
            .prune_propagation_entries_to_limit_bytes_with_priorities(
                limit_bytes,
                prioritised_destinations.as_slice(),
            )
            .map_err(std::io::Error::other)?;
        if pruned.is_empty() {
            return Ok(());
        }
        {
            let mut guard =
                self.propagation_payloads.lock().expect("propagation payload mutex poisoned");
            for transient_id in &pruned {
                guard.remove(transient_id.as_str());
            }
        }
        for transient_id in pruned {
            self.remove_peer_queue_snapshot_id(transient_id.as_str());
        }
        Ok(())
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
}

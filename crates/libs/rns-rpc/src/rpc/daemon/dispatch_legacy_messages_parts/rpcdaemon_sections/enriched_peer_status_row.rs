impl RpcDaemon {

    pub(super) fn enriched_peer_status_row(&self, peer: PeerRecord) -> JsonValue {
        let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes) =
            self.peer_message_stats(peer.peer.as_str()).unwrap_or((0, 0, 0, 0, 0, 0));
        let peering_key = peer_peering_key_value(&peer, self.identity_hash.as_str());
        let peering_key_status = peer_peering_key_status(&peer, peering_key);
        let acceptance_rate =
            peer_acceptance_rate_for_reporting(peer.acceptance_rate, outgoing, offered, peer.alive);
        let handled_ids =
            self.store.list_peer_handled_propagation_ids(peer.peer.as_str()).unwrap_or_default();
        let unhandled_ids =
            self.store.list_peer_unhandled_propagation_ids(peer.peer.as_str()).unwrap_or_default();
        let is_static_peer = self.is_static_peer(peer.peer.as_str());
        let (sync_schedule_state, sync_schedule_reason) = peer_sync_schedule(&peer);
        let sync_strategy = peer.sync_strategy;
        let mut row = serde_json::to_value(&peer).unwrap_or_else(|_| json!({}));
        row["type"] =
            JsonValue::String(if is_static_peer { "static" } else { "discovered" }.to_string());
        row["state"] = JsonValue::from(PEER_SYNC_STATE_IDLE);
        row["state_name"] = JsonValue::from("idle");
        row["sync_schedule_state"] = JsonValue::from(sync_schedule_state);
        row["sync_schedule_reason"] = sync_schedule_reason.map_or(JsonValue::Null, JsonValue::from);
        row["sync_strategy"] = JsonValue::from(sync_strategy);
        row["ler"] = JsonValue::from(0);
        row["str"] = row
            .get("sync_transfer_rate")
            .and_then(JsonValue::as_f64)
            .map(|value| JsonValue::from(value as u64))
            .unwrap_or_else(|| JsonValue::from(0));
        row["messages"] = json!({
            "offered": offered,
            "outgoing": outgoing,
            "incoming": incoming,
            "unhandled": unhandled,
            "offered_bytes": offered_bytes,
            "unhandled_bytes": unhandled_bytes,
            "handled_ids": handled_ids,
            "unhandled_ids": unhandled_ids,
        });
        row["offered"] = json!(offered);
        row["outgoing"] = json!(outgoing);
        row["incoming"] = json!(incoming);
        row["handled_ids"] = json!(handled_ids);
        row["unhandled_ids"] = json!(unhandled_ids);
        row["acceptance_rate"] = json!(acceptance_rate);
        row["peering_key"] = peering_key.map_or(JsonValue::Null, JsonValue::from);
        row["peering_key_status"] = json!(peering_key_status);
        row["last_heard"] = row.get("last_seen").cloned().unwrap_or(JsonValue::Null);
        let transfer_limit =
            peer.propagation_transfer_limit.map(JsonValue::from).unwrap_or(JsonValue::Null);
        let sync_limit =
            peer.propagation_sync_limit.map(JsonValue::from).unwrap_or(JsonValue::Null);
        row["propagation_transfer_limit"] = transfer_limit.clone();
        row["transfer_limit"] = transfer_limit;
        row["propagation_sync_limit"] = sync_limit.clone();
        row["sync_limit"] = sync_limit;
        row["target_stamp_cost"] =
            row.get("propagation_stamp_cost").cloned().unwrap_or(JsonValue::Null);
        row["stamp_cost_flexibility"] =
            row.get("propagation_stamp_cost_flexibility").cloned().unwrap_or(JsonValue::Null);
        row
    }

    pub(super) fn postponed_peer_sync_response(
        &self,
        request_id: u64,
        record: &PeerRecord,
        timestamp: i64,
        postpone_reason: &str,
        transfer_limit_bytes: Option<usize>,
        sync_limit_bytes: Option<usize>,
    ) -> RpcResponse {
        let (
            acceptance_rate,
            last_sync_attempt,
            next_sync_attempt,
            sync_backoff,
            sync_transfer_rate,
            alive,
        ) = {
            let mut guard = self.peers.lock().expect("peers mutex poisoned");
            if let Some(existing) = guard.get_mut(&record.peer) {
                existing.last_sync_attempt = timestamp;
                existing.sync_schedule_reason =
                    (postpone_reason != "backoff").then(|| postpone_reason.to_string());
                if postpone_reason == "backoff" && existing.last_sync_attempt > existing.last_seen {
                    existing.alive = false;
                }
                (
                    existing.acceptance_rate,
                    existing.last_sync_attempt,
                    existing.next_sync_attempt,
                    existing.sync_backoff,
                    existing.sync_transfer_rate,
                    existing.alive,
                )
            } else {
                (
                    record.acceptance_rate,
                    timestamp,
                    record.next_sync_attempt,
                    record.sync_backoff,
                    record.sync_transfer_rate,
                    record.alive,
                )
            }
        };
        let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes) =
            self.peer_message_stats(record.peer.as_str()).unwrap_or((0, 0, 0, 0, 0, 0));
        let acceptance_rate =
            peer_acceptance_rate_for_reporting(acceptance_rate, outgoing, offered, alive);
        let handled_ids =
            self.store.list_peer_handled_propagation_ids(record.peer.as_str()).unwrap_or_default();
        let unhandled_ids = self
            .store
            .list_peer_unhandled_propagation_ids(record.peer.as_str())
            .unwrap_or_default();
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
        let propagation_sync = json!({
            "synced": false,
            "postponed": true,
            "postpone_reason": postpone_reason,
            "handled": 0,
            "skipped": 0,
            "rejected": 0,
            "offered": 0,
            "bytes": 0,
            "offered_bytes": 0,
            "rejected_bytes": 0,
            "remaining": 0,
            "remaining_bytes": 0,
            "handled_ids": [],
            "skipped_ids": [],
            "rejected_ids": [],
            "transfer_limited": 0,
            "transfer_limited_bytes": 0,
            "transfer_limited_ids": [],
            "messages": [],
            "transfer_limit": transfer_limit_bytes,
            "sync_limit": sync_limit_bytes,
            "target_stamp_cost": record.propagation_stamp_cost,
            "stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
        });
        let peer_type_value = record.peer_type.clone();
        let peer_status_type =
            if self.is_static_peer(record.peer.as_str()) { "static" } else { "discovered" };
        let peering_key = peer_peering_key_value(record, self.identity_hash.as_str());
        let peering_key_status = peer_peering_key_status(record, peering_key);
        let sync_schedule_state = peer_sync_schedule_state(postpone_reason);
        let mut propagation_sync = propagation_sync;
        propagation_sync["peering_key"] = peering_key.map_or(JsonValue::Null, JsonValue::from);
        propagation_sync["peering_key_status"] = json!(peering_key_status);
        propagation_sync["state"] = json!(PEER_SYNC_STATE_IDLE);
        propagation_sync["state_name"] = json!("idle");
        propagation_sync["sync_schedule_state"] = json!(sync_schedule_state);
        propagation_sync["sync_schedule_reason"] = json!(postpone_reason);
        let mut event_payload = json!({
            "peer": &record.peer,
            "peer_type": peer_type_value,
            "type": peer_status_type,
            "timestamp": timestamp,
            "name": &record.name,
            "name_source": &record.name_source,
            "last_heard": record.last_seen,
            "first_seen": record.first_seen,
            "seen_count": record.seen_count,
            "state": PEER_SYNC_STATE_IDLE,
            "sync_strategy": record.sync_strategy,
            "ler": 0,
            "peering_timebase": record.peering_timebase,
            "network_distance": record.network_distance,
            "rx_bytes": record.rx_bytes,
            "tx_bytes": record.tx_bytes,
            "alive": alive,
            "acceptance_rate": acceptance_rate,
            "last_sync_attempt": last_sync_attempt,
            "next_sync_attempt": next_sync_attempt,
            "sync_backoff": sync_backoff,
            "sync_transfer_rate": sync_transfer_rate,
            "str": sync_transfer_rate as u64,
            "synced": false,
            "postponed": true,
            "postpone_reason": postpone_reason,
            "propagation_transfer_limit": record.propagation_transfer_limit,
            "propagation_sync_limit": record.propagation_sync_limit,
            "propagation_stamp_cost": record.propagation_stamp_cost,
            "propagation_stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
            "peering_key": peering_key,
            "peering_key_status": peering_key_status,
            "transfer_limit": transfer_limit_bytes,
            "sync_limit": sync_limit_bytes,
            "target_stamp_cost": record.propagation_stamp_cost,
            "stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
            "offered": offered,
            "outgoing": outgoing,
            "incoming": incoming,
            "messages": messages,
            "propagation": propagation_sync.clone(),
        });
        event_payload["state_name"] = json!("idle");
        event_payload["sync_schedule_state"] = json!(sync_schedule_state);
        event_payload["sync_schedule_reason"] = json!(postpone_reason);
        self.publish_event(RpcEvent { event_type: "peer_sync".into(), payload: event_payload });

        let mut result = json!({
                "peer": &record.peer,
                "peer_type": peer_type_value,
                "type": peer_status_type,
                "name": &record.name,
                "name_source": &record.name_source,
                "first_seen": record.first_seen,
                "seen_count": record.seen_count,
                "synced": false,
                "postponed": true,
                "postpone_reason": postpone_reason,
                "state": PEER_SYNC_STATE_IDLE,
                "sync_strategy": record.sync_strategy,
                "ler": 0,
                "peering_timebase": record.peering_timebase,
                "network_distance": record.network_distance,
                "rx_bytes": record.rx_bytes,
                "tx_bytes": record.tx_bytes,
                "alive": alive,
                "acceptance_rate": acceptance_rate,
                "last_heard": record.last_seen,
                "last_sync_attempt": last_sync_attempt,
                "next_sync_attempt": next_sync_attempt,
                "sync_backoff": sync_backoff,
                "sync_transfer_rate": sync_transfer_rate,
                "str": sync_transfer_rate as u64,
                "propagation_transfer_limit": record.propagation_transfer_limit,
                "propagation_sync_limit": record.propagation_sync_limit,
                "propagation_stamp_cost": record.propagation_stamp_cost,
                "propagation_stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                "peering_key": peering_key,
                "peering_key_status": peering_key_status,
                "transfer_limit": transfer_limit_bytes,
                "sync_limit": sync_limit_bytes,
                "target_stamp_cost": record.propagation_stamp_cost,
                "stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                "offered": offered,
                "outgoing": outgoing,
                "incoming": incoming,
                "messages": messages,
                "propagation": propagation_sync,
        });
        result["state_name"] = json!("idle");
        result["sync_schedule_state"] = json!(sync_schedule_state);
        result["sync_schedule_reason"] = json!(postpone_reason);
        RpcResponse { id: request_id, result: Some(result), error: None }
    }

    fn local_peer_offer_error_response(
        &self,
        request_id: u64,
        record: &PeerRecord,
        timestamp: i64,
        reason: &str,
        offer_response: u8,
        limit_bytes: (Option<usize>, Option<usize>),
    ) -> RpcResponse {
        let (transfer_limit_bytes, sync_limit_bytes) = limit_bytes;
        let peer = self
            .peers
            .lock()
            .expect("peers mutex poisoned")
            .get(record.peer.as_str())
            .cloned()
            .unwrap_or_else(|| record.clone());
        let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes) =
            self.peer_message_stats(peer.peer.as_str()).unwrap_or((0, 0, 0, 0, 0, 0));
        let acceptance_rate =
            peer_acceptance_rate_for_reporting(peer.acceptance_rate, outgoing, offered, peer.alive);
        let handled_ids =
            self.store.list_peer_handled_propagation_ids(peer.peer.as_str()).unwrap_or_default();
        let unhandled_ids =
            self.store.list_peer_unhandled_propagation_ids(peer.peer.as_str()).unwrap_or_default();
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
        let peering_key = peer_peering_key_value(&peer, self.identity_hash.as_str());
        let peering_key_status = peer_peering_key_status(&peer, peering_key);
        let peer_type_value = peer.peer_type.clone();
        let peer_status_type =
            if self.is_static_peer(peer.peer.as_str()) { "static" } else { "discovered" };
        let failure_kind = local_retryable_peer_offer_failure_kind(offer_response);
        let propagation_sync = json!({
            "synced": false,
            "postponed": false,
            "state": PEER_SYNC_STATE_FAILED,
            "state_name": "failed",
            "reason": reason,
            "offer_response": offer_response,
            "handled": 0,
            "transferred": 0,
            "skipped": 0,
            "rejected": 0,
            "offered": 0,
            "bytes": 0,
            "offered_bytes": 0,
            "rejected_bytes": 0,
            "remaining": unhandled,
            "remaining_bytes": unhandled_bytes,
            "handled_ids": [],
            "transferred_ids": [],
            "skipped_ids": [],
            "rejected_ids": [],
            "transfer_limited": 0,
            "transfer_limited_bytes": 0,
            "transfer_limited_ids": [],
            "messages": [],
            "peering_key": peering_key,
            "peering_key_status": peering_key_status,
            "transfer_limit": transfer_limit_bytes,
            "sync_limit": sync_limit_bytes,
            "target_stamp_cost": peer.propagation_stamp_cost,
            "stamp_cost_flexibility": peer.propagation_stamp_cost_flexibility,
        });
        let mut payload = json!({
            "peer": &peer.peer,
            "peer_type": peer_type_value,
            "type": peer_status_type,
            "timestamp": timestamp,
            "name": &peer.name,
            "name_source": &peer.name_source,
            "last_heard": peer.last_seen,
            "first_seen": peer.first_seen,
            "seen_count": peer.seen_count,
            "state": 0,
            "sync_strategy": peer.sync_strategy,
            "ler": 0,
            "peering_timebase": peer.peering_timebase,
            "network_distance": peer.network_distance,
            "rx_bytes": peer.rx_bytes,
            "tx_bytes": peer.tx_bytes,
            "alive": peer.alive,
            "acceptance_rate": acceptance_rate,
            "last_sync_attempt": peer.last_sync_attempt,
            "next_sync_attempt": peer.next_sync_attempt,
            "sync_backoff": peer.sync_backoff,
            "sync_transfer_rate": peer.sync_transfer_rate,
            "str": peer.sync_transfer_rate as u64,
            "synced": false,
            "reason": reason,
            "offer_response": offer_response,
            "propagation_transfer_limit": peer.propagation_transfer_limit,
            "propagation_sync_limit": peer.propagation_sync_limit,
            "propagation_stamp_cost": peer.propagation_stamp_cost,
            "propagation_stamp_cost_flexibility": peer.propagation_stamp_cost_flexibility,
            "peering_key": peering_key,
            "peering_key_status": peering_key_status,
            "transfer_limit": transfer_limit_bytes,
            "sync_limit": sync_limit_bytes,
            "target_stamp_cost": peer.propagation_stamp_cost,
            "stamp_cost_flexibility": peer.propagation_stamp_cost_flexibility,
            "offered": offered,
            "outgoing": outgoing,
            "incoming": incoming,
            "messages": messages,
            "propagation": propagation_sync,
        });
        payload["state"] = json!(PEER_SYNC_STATE_FAILED);
        payload["state_name"] = json!("failed");
        payload["failure_kind"] = json!(failure_kind);
        payload["propagation"]["failure_kind"] = json!(failure_kind);
        self.publish_event(RpcEvent { event_type: "peer_sync".into(), payload: payload.clone() });

        RpcResponse { id: request_id, result: Some(payload), error: None }
    }
}

fn local_retryable_peer_offer_failure_kind(offer_error: u8) -> &'static str {
    match offer_error {
        LXMF_PEER_ERROR_NO_IDENTITY => "no_identity",
        LXMF_PEER_ERROR_INVALID_KEY => "invalid_key",
        LXMF_PEER_ERROR_INVALID_DATA => "invalid_data",
        LXMF_PEER_ERROR_INVALID_STAMP => "invalid_stamp",
        LXMF_PEER_ERROR_NOT_FOUND => "not_found",
        LXMF_PEER_ERROR_TIMEOUT => "timeout",
        _ => "failed",
    }
}

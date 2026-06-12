impl RpcDaemon {
    fn handle_rpc_legacy_remote_sync(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
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
            _ => unreachable!("legacy remote sync route: {}", request.method),
        }
    }
}

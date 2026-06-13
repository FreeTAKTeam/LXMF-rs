impl RpcDaemon {
    fn handle_rpc_legacy_remote_unpeer(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
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
                let prior_unpeer_failure = self
                    .remote_unpeer_failure_state
                    .lock()
                    .expect("remote unpeer failure mutex poisoned")
                    .clone();
                let bridge = match self
                    .remote_control_bridge
                    .lock()
                    .expect("remote control bridge mutex poisoned")
                    .clone()
                {
                    Some(bridge) => bridge,
                    None => {
                        self.record_remote_unpeer_failure(
                            "remote control bridge unavailable".to_string(),
                        );
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
                        let error = err.to_string();
                        self.record_remote_unpeer_failure(error.clone());
                        if is_remote_access_denied_error(&err) {
                            self.break_remote_peer_sync_peering_on_denied_access(
                                snapshot_peer.as_str(),
                                remote_id.as_str(),
                                error.as_str(),
                            )?;
                        } else {
                            let timestamp = now_i64();
                            if let Ok(mut peers) = self.peers.lock() {
                                if let Some(peer) = peers.get_mut(snapshot_peer.as_str()) {
                                    peer.last_sync_attempt = timestamp;
                                    peer.sync_backoff = peer
                                        .sync_backoff
                                        .saturating_add(super::init::LXMF_PEER_SYNC_BACKOFF_STEP_SECS);
                                    peer.next_sync_attempt =
                                        timestamp.saturating_add(i64::from(peer.sync_backoff));
                                }
                            }
                            let _ = self
                                .record_payload_backed_peer_queue_snapshot(snapshot_peer.as_str());
                            self.publish_failed_remote_peer_sync_event(
                                snapshot_peer.as_str(),
                                remote_id.as_str(),
                                error.as_str(),
                                None,
                                None,
                                None,
                            );
                        }
                        return Err(err);
                    }
                };
                self.clear_matching_remote_unpeer_failure(prior_unpeer_failure);
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
            _ => unreachable!("legacy remote unpeer route: {}", request.method),
        }
    }
}

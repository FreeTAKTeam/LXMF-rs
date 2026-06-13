impl RpcDaemon {
    fn handle_rpc_legacy_remote_download_fetch(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
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
                        if let Some(err) =
                            remote_transfer_incomplete_error(&result, "remote download postponed")
                        {
                            self.update_propagation_sync_state(|state| {
                                state.sync_state = PR_FAILED;
                                state.state_name = "failed".to_string();
                                state.sync_progress = 0.0;
                                state.last_sync_error = Some(err.to_string());
                            });
                            self.record_failed_remote_transfer_for_active_source_peer(
                                remote_id.as_str(),
                                remote_id.as_str(),
                                &err,
                            )?;
                            for peer in self.active_peer_ids() {
                                self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                            }
                            result
                        } else {
                            let imported = match self.import_remote_propagation_payloads(&result) {
                                Ok(imported) => imported,
                                Err(err) => {
                                    self.update_propagation_sync_state(|state| {
                                        state.sync_state = PR_FAILED;
                                        state.state_name = "failed".to_string();
                                        state.sync_progress = 0.0;
                                        state.last_sync_error = Some(err.to_string());
                                    });
                                    self.record_failed_remote_import_for_active_source_peer(
                                        remote_id.as_str(),
                                        remote_id.as_str(),
                                        &err,
                                    )?;
                                    for peer in self.active_peer_ids() {
                                        self.record_payload_backed_peer_queue_snapshot(
                                            peer.as_str(),
                                        )?;
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
                                result.insert(
                                    "imported_ids".to_string(),
                                    json!(imported.imported_ids),
                                );
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
                    }
                    Err(err) => {
                        let sync_state = remote_propagation_failure_state(&err);
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = sync_state;
                            state.state_name = propagation_sync_state_name(sync_state).to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_error = Some(err.to_string());
                        });
                        if is_remote_access_denied_error(&err) {
                            self.break_remote_peer_sync_peering_on_denied_access(
                                remote_id.as_str(),
                                remote_id.as_str(),
                                err.to_string().as_str(),
                            )?;
                        } else {
                            self.record_failed_remote_transfer_for_active_source_peer(
                                remote_id.as_str(),
                                remote_id.as_str(),
                                &err,
                            )?;
                            for peer in self.active_peer_ids() {
                                self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                            }
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
                        let sync_state = remote_propagation_failure_state(&err);
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = sync_state;
                            state.state_name = propagation_sync_state_name(sync_state).to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_error = Some(err.to_string());
                        });
                        if is_remote_access_denied_error(&err) {
                            self.break_remote_peer_sync_peering_on_denied_access(
                                remote_id.as_str(),
                                remote_id.as_str(),
                                err.to_string().as_str(),
                            )?;
                        } else {
                            self.record_failed_remote_transfer_for_active_source_peer(
                                remote_id.as_str(),
                                remote_id.as_str(),
                                &err,
                            )?;
                            for peer in self.active_peer_ids() {
                                self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                            }
                        }
                        return Err(err);
                    }
                };
                if let Some(err) =
                    remote_transfer_incomplete_error(&result, "remote fetch postponed")
                {
                    self.update_propagation_sync_state(|state| {
                        state.sync_state = PR_FAILED;
                        state.state_name = "failed".to_string();
                        state.sync_progress = 0.0;
                        state.last_sync_error = Some(err.to_string());
                    });
                    self.record_failed_remote_transfer_for_active_source_peer(
                        remote_id.as_str(),
                        remote_id.as_str(),
                        &err,
                    )?;
                    for peer in self.active_peer_ids() {
                        self.record_payload_backed_peer_queue_snapshot(peer.as_str())?;
                    }
                    let propagation =
                        self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                    return Ok(RpcResponse {
                        id: request.id,
                        result: Some(json!({
                            "remote": remote_id,
                            "propagation": propagation,
                            "result": result,
                        })),
                        error: None,
                    });
                }
                let imported = match self.import_remote_propagation_payloads(&result) {
                    Ok(imported) => imported,
                    Err(err) => {
                        self.update_propagation_sync_state(|state| {
                            state.sync_state = PR_FAILED;
                            state.state_name = "failed".to_string();
                            state.sync_progress = 0.0;
                            state.last_sync_error = Some(err.to_string());
                        });
                        self.record_failed_remote_import_for_active_source_peer(
                            remote_id.as_str(),
                            remote_id.as_str(),
                            &err,
                        )?;
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
            _ => unreachable!("legacy remote transfer route: {}", request.method),
        }
    }
}

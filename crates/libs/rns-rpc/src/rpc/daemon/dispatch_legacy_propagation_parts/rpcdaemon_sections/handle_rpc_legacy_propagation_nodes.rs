impl RpcDaemon {
    fn handle_rpc_legacy_propagation_nodes(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
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
                let has_payload = normalized_payload.is_some();
                let decoded_payload = normalized_payload
                    .as_ref()
                    .and_then(|(_transient_id, payload_hex)| hex::decode(payload_hex).ok());
                if let Some(payload) = decoded_payload.as_ref() {
                    if self.propagation_payload_destination_is_ignored(payload.as_slice()) {
                        self.emit_ignored_propagation_drop_event(
                            payload.as_slice(),
                            Some(transient_id.as_str()),
                            "propagation_ingest",
                            None,
                        );
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "ignored propagation destination",
                        ));
                    }
                }
                let payload_bytes = decoded_payload.as_ref().map(Vec::len).unwrap_or(0);
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
                    self.store
                        .mark_local_propagation_processed(transient_id.as_str())
                        .map_err(std::io::Error::other)?;
                    self.prune_propagation_payloads_to_storage_limit()?;
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
                        "propagation": state,
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
                let state = {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    guard.client_propagation_messages_served =
                        guard.client_propagation_messages_served.saturating_add(1);
                    let state = guard.clone();
                    drop(guard);
                    self.update_daemon_status_snapshot(|snapshot| {
                        snapshot.propagation = state.clone();
                    });
                    state
                };

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "transient_id": normalized_transient_id,
                        "payload_hex": payload,
                        "payload_bytes": payload_bytes,
                        "transferred_bytes": payload_bytes,
                        "propagation": state,
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
            "get_outbound_propagation_cost" => {
                let parsed = request
                    .params
                    .map(serde_json::from_value::<SetOutboundPropagationNodeParams>)
                    .transpose()
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let (peer, target_cost, source) =
                    self.outbound_propagation_cost_lookup(parsed.as_ref().and_then(|value| value.peer.as_deref()));
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "peer": peer,
                        "target_cost": target_cost,
                        "source": source,
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
            _ => unreachable!("legacy propagation node route: {}", request.method),
        }
    }
}

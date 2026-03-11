impl RpcDaemon {
    fn handle_rpc_legacy_propagation(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
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
            "propagation_enable" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationEnableParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let state = {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    guard.enabled = parsed.enabled;
                    if parsed.store_root.is_some() {
                        guard.store_root = parsed.store_root;
                    }
                    if let Some(cost) = parsed.target_cost {
                        guard.target_cost = cost;
                    }
                    if let Some(limit) = parsed.message_storage_limit_mb {
                        guard.message_storage_limit_mb = Some(limit);
                    }
                    if let Some(autopeer) = parsed.autopeer {
                        guard.autopeer = autopeer;
                    }
                    if let Some(autopeer_maxdepth) = parsed.autopeer_maxdepth {
                        guard.autopeer_maxdepth = autopeer_maxdepth;
                    }
                    if let Some(static_peers) = parsed.static_peers {
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
                let transient_id = parsed.transient_id.unwrap_or_else(|| {
                    let mut hasher = Sha256::new();
                    hasher.update(payload_hex.as_bytes());
                    encode_hex(hasher.finalize())
                });

                if !payload_hex.is_empty() {
                    self.propagation_payloads
                        .lock()
                        .expect("propagation payload mutex poisoned")
                        .insert(transient_id.clone(), payload_hex);
                }

                let state = {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    let ingested_count = usize::from(!transient_id.is_empty());
                    guard.last_ingest_count = ingested_count;
                    guard.total_ingested += ingested_count;
                    guard.client_propagation_messages_received = guard
                        .client_propagation_messages_received
                        .saturating_add(ingested_count);
                    guard.clone()
                };
                self.update_daemon_status_snapshot(|snapshot| {
                    snapshot.propagation = state.clone();
                });

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "ingested_count": state.last_ingest_count,
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

                let payload = self
                    .propagation_payloads
                    .lock()
                    .expect("propagation payload mutex poisoned")
                    .get(&parsed.transient_id)
                    .cloned()
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "transient_id not found")
                    })?;
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
                        "transient_id": parsed.transient_id,
                        "payload_hex": payload,
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
                let peer = parsed
                    .and_then(|value| value.peer)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                {
                    let mut guard = self
                        .outbound_propagation_node
                        .lock()
                        .expect("propagation node mutex poisoned");
                    *guard = peer.clone();
                }
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
                let bridge = self
                    .remote_control_bridge
                    .lock()
                    .expect("remote control bridge mutex poisoned")
                    .clone()
                    .ok_or_else(|| std::io::Error::other("remote control bridge unavailable"))?;
                let timeout_secs = parsed.timeout_secs.unwrap_or(5.0).max(0.1);
                let result = bridge.propagation_remote_status(
                    parsed.remote.as_str(),
                    parsed.identity_private_key_hex.as_deref(),
                    timeout_secs,
                )?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "remote": parsed.remote,
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
                let bridge = self
                    .remote_control_bridge
                    .lock()
                    .expect("remote control bridge mutex poisoned")
                    .clone()
                    .ok_or_else(|| std::io::Error::other("remote control bridge unavailable"))?;
                let timeout_secs = parsed.timeout_secs.unwrap_or(5.0).max(0.1);
                let result = bridge.propagation_remote_sync(
                    parsed.remote.as_str(),
                    parsed.peer.as_str(),
                    parsed.identity_private_key_hex.as_deref(),
                    timeout_secs,
                )?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "remote": parsed.remote,
                        "peer": parsed.peer,
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
                let bridge = self
                    .remote_control_bridge
                    .lock()
                    .expect("remote control bridge mutex poisoned")
                    .clone()
                    .ok_or_else(|| std::io::Error::other("remote control bridge unavailable"))?;
                let timeout_secs = parsed.timeout_secs.unwrap_or(5.0).max(0.1);
                let result = bridge.propagation_remote_unpeer(
                    parsed.remote.as_str(),
                    parsed.peer.as_str(),
                    parsed.identity_private_key_hex.as_deref(),
                    timeout_secs,
                )?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "remote": parsed.remote,
                        "peer": parsed.peer,
                        "result": result,
                    })),
                    error: None,
                })
            }
            _ => unreachable!("legacy propagation route: {}", request.method),
        }
    }

}

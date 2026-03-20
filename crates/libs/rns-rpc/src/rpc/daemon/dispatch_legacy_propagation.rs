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
                let target_cost = self
                    .propagation_state
                    .lock()
                    .expect("propagation mutex poisoned")
                    .target_cost;
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
                let transient_id = parsed.transient_id.unwrap_or_else(|| {
                    normalized_payload
                        .as_ref()
                        .map(|(transient_id, _payload_hex)| transient_id.clone())
                        .unwrap_or_else(|| {
                            let mut hasher = Sha256::new();
                            hasher.update(payload_hex.as_bytes());
                            encode_hex(hasher.finalize())
                        })
                });
                let ingested_count = usize::from(!payload_hex.is_empty() && !transient_id.is_empty());

                if let Some(payload_hex) =
                    normalized_payload.map(|(_transient_id, payload_hex)| payload_hex)
                {
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

const PROPAGATION_STAMP_SIZE: usize = 32;
const PROPAGATION_STAMP_WORKBLOCK_ROUNDS: usize = 1000;
// Python rejects propagation-stamped payloads that cannot contain a minimally
// structured LXMF message before validating the trailing stamp.
const MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE: usize = 112 + PROPAGATION_STAMP_SIZE;

fn normalize_propagation_payload_hex(
    payload_hex: &str,
    target_cost: u32,
) -> Result<(String, String), std::io::Error> {
    let transient_data = decode_propagation_payload_hex(payload_hex)?;
    let (transient_id, payload) = normalize_propagation_payload_bytes(&transient_data, target_cost)?;
    Ok((hex::encode(transient_id), hex::encode(payload)))
}

fn decode_propagation_payload_hex(payload_hex: &str) -> Result<Vec<u8>, std::io::Error> {
    hex::decode(payload_hex.trim()).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid propagation payload hex: {err}"),
        )
    })
}

fn normalize_propagation_payload_bytes(
    transient_data: &[u8],
    target_cost: u32,
) -> Result<([u8; 32], &[u8]), std::io::Error> {
    let lxm_data = propagation_payload_hash_input(transient_data, target_cost)?;

    let transient_hash = Sha256::digest(lxm_data);
    let mut transient_id = [0u8; 32];
    transient_id.copy_from_slice(transient_hash.as_slice());
    Ok((transient_id, lxm_data))
}

fn propagation_payload_hash_input(
    transient_data: &[u8],
    target_cost: u32,
) -> Result<&[u8], std::io::Error> {
    if target_cost == 0 {
        return Ok(split_propagation_stamp(transient_data)
            .map(|(lxm_data, _stamp)| lxm_data)
            .unwrap_or(transient_data));
    }

    let (lxm_data, stamp) = split_propagation_stamp(transient_data).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "invalid propagation stamp",
        )
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

fn split_propagation_stamp(transient_data: &[u8]) -> Option<(&[u8], &[u8])> {
    if transient_data.len() <= MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE {
        return None;
    }

    let split_at = transient_data.len() - PROPAGATION_STAMP_SIZE;
    Some((&transient_data[..split_at], &transient_data[split_at..]))
}

fn propagation_stamp_workblock(material: &[u8]) -> Vec<u8> {
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

fn propagation_stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    propagation_stamp_value(workblock, stamp) >= target_cost
}

fn propagation_stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
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

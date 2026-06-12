impl RpcDaemon {
    fn handle_rpc_legacy_propagation_policy(
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
                    if let Some(auth_required) = parsed.auth_required {
                        guard.auth_required = auth_required;
                    }
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
                    if let Some(retain_synced_on_node) = parsed.retain_synced_on_node {
                        guard.retain_synced_on_node = retain_synced_on_node;
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
            _ => unreachable!("legacy propagation policy route: {}", request.method),
        }
    }
}

use super::*;

impl RpcDaemon {
    pub(super) fn handle_rpc_legacy_misc(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "paper_ingest_uri" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PaperIngestUriParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                if !parsed.uri.starts_with("lxm://") {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "paper URI must start with lxm://",
                    ));
                }

                let transient_id = {
                    let mut hasher = Sha256::new();
                    hasher.update(parsed.uri.as_bytes());
                    encode_hex(hasher.finalize())
                };

                let duplicate = {
                    let mut guard =
                        self.paper_ingest_seen.lock().expect("paper ingest mutex poisoned");
                    if guard.contains(&transient_id) {
                        true
                    } else {
                        guard.insert(transient_id.clone());
                        false
                    }
                };

                let body = parsed.uri.trim_start_matches("lxm://");
                let destination = first_n_chars(body, 32).unwrap_or_default();

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "destination": destination,
                        "transient_id": transient_id,
                        "duplicate": duplicate,
                        "bytes_len": parsed.uri.len(),
                    })),
                    error: None,
                })
            }
            "stamp_policy_get" => {
                let policy = self.stamp_policy.lock().expect("stamp mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "stamp_policy": policy })),
                    error: None,
                })
            }
            "stamp_policy_set" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: StampPolicySetParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let policy = {
                    let mut guard = self.stamp_policy.lock().expect("stamp mutex poisoned");
                    if let Some(value) = parsed.target_cost {
                        guard.target_cost = value;
                    }
                    if let Some(value) = parsed.flexibility {
                        guard.flexibility = value;
                    }
                    if let Some(value) = parsed.enforce {
                        guard.enforce = value;
                    }
                    guard.clone()
                };
                self.update_daemon_status_snapshot(|snapshot| {
                    snapshot.stamp_policy = policy.clone();
                });

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "stamp_policy": policy })),
                    error: None,
                })
            }
            "ticket_generate" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: TicketGenerateParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let ttl_secs = parsed.ttl_secs.unwrap_or(Self::DEFAULT_TICKET_EXPIRY_SECS);
                let record = self.generate_ticket(parsed.destination.as_str(), Some(ttl_secs))?;
                let Some(record) = record else {
                    return Ok(RpcResponse {
                        id: request.id,
                        result: Some(json!({
                            "ticket": null,
                            "destination": parsed.destination,
                            "expires_at": null,
                            "ttl_secs": ttl_secs,
                            "included": false,
                            "reason": "ticket_interval",
                        })),
                        error: None,
                    });
                };

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "ticket": record.ticket,
                        "destination": record.destination,
                        "expires_at": record.expires_at,
                        "ttl_secs": ttl_secs,
                        "included": true,
                    })),
                    error: None,
                })
            }
            "rnode_management" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: RNodeManagementParams = serde_json::from_value(params.clone())
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let iface = parsed.iface.trim();
                if iface.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "iface is required",
                    ));
                }
                let command = parsed.command.trim();
                if command.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "command is required",
                    ));
                }
                let Some(bridge) = self
                    .rnode_management_bridge
                    .lock()
                    .expect("rnode_management_bridge mutex poisoned")
                    .clone()
                else {
                    return Ok(RpcResponse {
                        id: request.id,
                        result: None,
                        error: Some(RpcError::new(
                            "RNODE_MANAGEMENT_UNAVAILABLE",
                            "RNode management bridge is not configured",
                        )),
                    });
                };
                match bridge.dispatch_rnode_management(iface, command, &params) {
                    Ok(result) => {
                        Ok(RpcResponse { id: request.id, result: Some(result), error: None })
                    }
                    Err(err) => Ok(RpcResponse {
                        id: request.id,
                        result: None,
                        error: Some(RpcError::new("RNODE_MANAGEMENT_FAILED", err.to_string())),
                    }),
                }
            }
            "weave_remote_display_control" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: WeaveRemoteDisplayControlParams =
                    serde_json::from_value(params.clone()).map_err(|err| {
                        std::io::Error::new(std::io::ErrorKind::InvalidInput, err)
                    })?;
                let iface = parsed.iface.trim();
                if iface.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "iface is required",
                    ));
                }
                let remote_switch_id_hex = parsed
                    .remote_switch_id_hex
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let Some(bridge) = self
                    .weave_display_control_bridge
                    .lock()
                    .expect("weave_display_control_bridge mutex poisoned")
                    .clone()
                else {
                    return Ok(RpcResponse {
                        id: request.id,
                        result: None,
                        error: Some(RpcError::new(
                            "WEAVE_DISPLAY_CONTROL_UNAVAILABLE",
                            "Weave display control bridge is not configured",
                        )),
                    });
                };
                match bridge.set_weave_remote_display(iface, parsed.enable, remote_switch_id_hex) {
                    Ok(result) => {
                        Ok(RpcResponse { id: request.id, result: Some(result), error: None })
                    }
                    Err(err) => Ok(RpcResponse {
                        id: request.id,
                        result: None,
                        error: Some(RpcError::new("WEAVE_DISPLAY_CONTROL_FAILED", err.to_string())),
                    }),
                }
            }
            "announce_now" => {
                let timestamp = now_i64();
                if let Some(bridge) = &self.announce_bridge {
                    let _ = bridge.announce_now();
                }
                let event = RpcEvent {
                    event_type: "announce_sent".into(),
                    payload: json!({ "timestamp": timestamp }),
                };
                self.publish_event(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "announce_id": request.id })),
                    error: None,
                })
            }
            "announce_received" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: AnnounceReceivedParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let timestamp = parsed.timestamp.unwrap_or_else(now_i64);
                let peer = parsed.peer.clone();
                let aspect = parsed.aspect.clone();
                let (
                    parsed_propagation_stamp_cost,
                    parsed_stamp_cost_flexibility,
                    parsed_peering_cost,
                ) = parse_announce_costs_from_app_data_hex(parsed.app_data_hex.as_deref());
                let parsed_delivery_stamp_cost = is_lxmf_delivery_aspect(aspect.as_deref())
                    .then(|| {
                        parse_delivery_stamp_cost_from_app_data_hex(parsed.app_data_hex.as_deref())
                    })
                    .flatten();
                let stamp_cost = parsed
                    .stamp_cost
                    .or(parsed_delivery_stamp_cost)
                    .or(parsed_propagation_stamp_cost);
                let stamp_cost_flexibility =
                    parsed.stamp_cost_flexibility.or(parsed_stamp_cost_flexibility);
                let peering_cost = parsed.peering_cost.or(parsed_peering_cost);
                let (name, name_source) = if parsed.name.is_none() && parsed.name_source.is_none() {
                    parse_peer_name_from_app_data_hex(parsed.app_data_hex.as_deref())
                        .map(|(name, source)| (Some(name), Some(source.to_string())))
                        .unwrap_or((parsed.name, parsed.name_source))
                } else {
                    (parsed.name, parsed.name_source)
                };
                self.accept_announce_with_metadata_for_path_response(
                    parsed.peer,
                    timestamp,
                    name,
                    name_source,
                    parsed.app_data_hex,
                    parsed.capabilities,
                    parsed.rssi,
                    parsed.snr,
                    parsed.q,
                    stamp_cost,
                    Some(stamp_cost_flexibility),
                    Some(peering_cost),
                    aspect,
                    parsed.hops,
                    parsed.interface,
                    parsed.source_private_key,
                    parsed.source_identity,
                    parsed.source_node,
                    parsed.is_path_response,
                )?;
                let record =
                    self.peers.lock().expect("peers mutex poisoned").get(peer.as_str()).cloned();
                let peer = record.map(|record| self.enriched_peer_status_row(record));
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "peer": peer })),
                    error: None,
                })
            }
            _ => unreachable!("legacy misc route: {}", request.method),
        }
    }
}

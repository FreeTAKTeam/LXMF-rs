impl RpcDaemon {
    fn handle_rpc_legacy_messages(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "list_messages" => {
                let items = self.store.list_messages(100, None).map_err(std::io::Error::other)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "messages": items,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "sdk_poll_events_v2" => self.handle_sdk_poll_events_v2(request),
            "list_announces" => {
                let parsed = request
                    .params
                    .map(serde_json::from_value::<ListAnnouncesParams>)
                    .transpose()
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
                    .unwrap_or_default();
                let limit = parsed.limit.unwrap_or(200).clamp(1, 5000);
                let (before_ts, before_id) = match parsed.before_ts {
                    Some(timestamp) => (Some(timestamp), None),
                    None => parse_announce_cursor(parsed.cursor.as_deref()).unwrap_or((None, None)),
                };
                let items = self
                    .store
                    .list_announces(limit, before_ts, before_id.as_deref())
                    .map_err(std::io::Error::other)?;
                let next_cursor = if items.len() >= limit {
                    items.last().map(|record| format!("{}:{}", record.timestamp, record.id))
                } else {
                    None
                };
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "announces": items,
                        "next_cursor": next_cursor,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "list_peers" => {
                let mut peers = self
                    .peers
                    .lock()
                    .expect("peers mutex poisoned")
                    .values()
                    .cloned()
                    .collect::<Vec<_>>();
                peers.sort_by(|a, b| {
                    b.last_seen.cmp(&a.last_seen).then_with(|| a.peer.cmp(&b.peer))
                });
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "peers": peers,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "list_interfaces" => {
                let interfaces = self.interfaces.lock().expect("interfaces mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "interfaces": interfaces,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "set_interfaces" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: SetInterfacesParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                for iface in &parsed.interfaces {
                    if iface.kind.trim().is_empty() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "interface type is required",
                        ));
                    }
                    if iface.kind == "tcp_client" && (iface.host.is_none() || iface.port.is_none()) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "tcp_client requires host and port",
                        ));
                    }
                    if iface.kind == "tcp_server" && iface.port.is_none() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "tcp_server requires port",
                        ));
                    }
                }
                let blocked = parsed
                    .interfaces
                    .iter()
                    .enumerate()
                    .filter_map(|(index, iface)| {
                        (!Self::is_legacy_hot_apply_kind(iface.kind.as_str()))
                            .then(|| Self::interface_identifier(iface, index))
                    })
                    .collect::<Vec<_>>();
                if !blocked.is_empty() {
                    return Ok(Self::restart_required_response(
                        request.id,
                        "set_interfaces",
                        blocked,
                    ));
                }

                {
                    let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
                    *guard = parsed.interfaces.clone();
                }
                let applied_interfaces = parsed
                    .interfaces
                    .iter()
                    .enumerate()
                    .map(|(index, iface)| Self::interface_identifier(iface, index))
                    .collect::<Vec<_>>();

                let event = RpcEvent {
                    event_type: "interfaces_updated".into(),
                    payload: json!({ "interfaces": parsed.interfaces.clone() }),
                };
                self.publish_event(event);

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "updated": true,
                        "applied_interfaces": applied_interfaces,
                        "rejected_interfaces": Vec::<String>::new(),
                    })),
                    error: None,
                })
            }
            "reload_config" => {
                if let Some(params) = request.params.clone() {
                    let parsed: ReloadConfigParams = serde_json::from_value(params).map_err(|err| {
                        std::io::Error::new(std::io::ErrorKind::InvalidInput, err)
                    })?;
                    for iface in &parsed.interfaces {
                        if iface.kind.trim().is_empty() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "interface type is required",
                            ));
                        }
                        if iface.kind == "tcp_client"
                            && (iface.host.is_none() || iface.port.is_none())
                        {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "tcp_client requires host and port",
                            ));
                        }
                        if iface.kind == "tcp_server" && iface.port.is_none() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "tcp_server requires port",
                            ));
                        }
                    }

                    let current = self.interfaces.lock().expect("interfaces mutex poisoned").clone();
                    if !Self::is_reload_hot_apply_compatible(&current, &parsed.interfaces) {
                        let mut affected = parsed
                            .interfaces
                            .iter()
                            .enumerate()
                            .filter_map(|(index, iface)| {
                                (!Self::is_legacy_hot_apply_kind(iface.kind.as_str()))
                                    .then(|| Self::interface_identifier(iface, index))
                            })
                            .collect::<Vec<_>>();
                        if affected.is_empty() {
                            affected = parsed
                                .interfaces
                                .iter()
                                .enumerate()
                                .map(|(index, iface)| Self::interface_identifier(iface, index))
                                .collect::<Vec<_>>();
                        }
                        if affected.is_empty() {
                            affected = current
                                .iter()
                                .enumerate()
                                .map(|(index, iface)| Self::interface_identifier(iface, index))
                                .collect::<Vec<_>>();
                        }
                        if affected.is_empty() {
                            affected.push("interfaces".to_string());
                        }
                        return Ok(Self::restart_required_response(
                            request.id,
                            "reload_config",
                            affected,
                        ));
                    }

                    {
                        let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
                        *guard = parsed.interfaces.clone();
                    }
                    let update_event = RpcEvent {
                        event_type: "interfaces_updated".into(),
                        payload: json!({ "interfaces": parsed.interfaces }),
                    };
                    self.publish_event(update_event);
                }
                let timestamp = now_i64();
                let event = RpcEvent {
                    event_type: "config_reloaded".into(),
                    payload: json!({ "timestamp": timestamp }),
                };
                self.publish_event(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "reloaded": true,
                        "timestamp": timestamp,
                        "hot_applied_legacy_tcp_only": request.params.is_some(),
                    })),
                    error: None,
                })
            }
            "peer_sync" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PeerOpParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let timestamp = now_i64();
                let record = self.upsert_peer(parsed.peer, timestamp, None, None);
                    let event = RpcEvent {
                        event_type: "peer_sync".into(),
                        payload: json!({
                            "peer": &record.peer,
                            "timestamp": timestamp,
                            "name": &record.name,
                            "name_source": &record.name_source,
                            "first_seen": record.first_seen,
                            "seen_count": record.seen_count,
                        }),
                    };
                self.publish_event(event);

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "peer": record.peer, "synced": true })),
                    error: None,
                })
            }
            "peer_unpeer" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PeerOpParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let removed = {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    guard.remove(&parsed.peer).is_some()
                };
                let event = RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: json!({ "peer": parsed.peer, "removed": removed }),
                };
                self.publish_event(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "removed": removed })),
                    error: None,
                })
            }
            "send_message" | "send_message_v2" | "sdk_send_v2" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed = parse_outbound_send_request(request.method.as_str(), params)?;

                self.store_outbound(
                    request.id,
                    parsed.id,
                    parsed.source,
                    parsed.destination,
                    parsed.title,
                    parsed.content,
                    parsed.fields,
                    parsed.method,
                    parsed.stamp_cost,
                    parsed.options,
                    parsed.include_ticket,
                )
            }
            "receive_message" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: ReceiveMessageParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let timestamp = now_i64();
                let record = MessageRecord {
                    id: parsed.id.clone(),
                    source: parsed.source,
                    destination: parsed.destination,
                    title: parsed.title,
                    content: parsed.content,
                    timestamp,
                    direction: "in".into(),
                    fields: parsed.fields,
                    receipt_status: None,
                };
                self.store_inbound_record(record)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "message_id": parsed.id })),
                    error: None,
                })
            }
            "record_receipt" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: RecordReceiptParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let message_id = parsed.message_id;
                let requested_status = parsed.status;
                let (status, updated) = {
                    let _status_guard = self
                        .delivery_status_lock
                        .lock()
                        .expect("delivery_status_lock mutex poisoned");
                    let existing_message =
                        self.store.get_message(&message_id).map_err(std::io::Error::other)?;
                    let existing_status = existing_message
                        .as_ref()
                        .and_then(|message| message.receipt_status.clone());
                    if existing_message.is_none() {
                        (requested_status, false)
                    } else if existing_status
                        .as_deref()
                        .is_some_and(Self::is_terminal_receipt_status)
                    {
                        (existing_status.unwrap_or(requested_status), false)
                    } else {
                        self.store
                            .update_receipt_status(&message_id, &requested_status)
                            .map_err(std::io::Error::other)?;
                        (requested_status, true)
                    }
                };
                if updated {
                    self.append_delivery_trace(&message_id, status.clone());
                }
                let reason_code = delivery_reason_code(&status);
                let event = RpcEvent {
                    event_type: "receipt".into(),
                    payload: json!({
                        "message_id": message_id,
                        "status": status,
                        "updated": updated,
                        "reason_code": reason_code,
                    }),
                };
                self.publish_event(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "message_id": message_id,
                        "status": status,
                        "updated": updated,
                        "reason_code": reason_code,
                    })),
                    error: None,
                })
            }
            "sdk_cancel_message_v2" => self.handle_sdk_cancel_message_v2(request),
            "message_delivery_trace" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: MessageDeliveryTraceParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let traces = self
                    .delivery_traces
                    .lock()
                    .expect("delivery traces mutex poisoned")
                    .get(parsed.message_id.as_str())
                    .cloned()
                    .unwrap_or_default();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "message_id": parsed.message_id,
                        "transitions": traces,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            _ => unreachable!("legacy message route: {}", request.method),
        }
    }

    fn restart_required_response(
        id: u64,
        operation: &str,
        affected_interfaces: Vec<String>,
    ) -> RpcResponse {
        let mut error = RpcError::new(
            "CONFIG_RESTART_REQUIRED",
            "requested interface mutation requires daemon restart",
        );
        error.machine_code = Some("UNSUPPORTED_MUTATION_KIND_REQUIRES_RESTART".to_string());
        error.category = Some("Config".to_string());
        error.retryable = Some(false);
        error.is_user_actionable = Some(true);

        let mut details = serde_json::Map::new();
        details.insert(
            "operation".to_string(),
            JsonValue::String(operation.to_string()),
        );
        details.insert(
            "affected_interfaces".to_string(),
            JsonValue::Array(
                affected_interfaces
                    .iter()
                    .map(|item| JsonValue::String(item.clone()))
                    .collect::<Vec<_>>(),
            ),
        );
        details.insert(
            "legacy_hot_apply_supported_kinds".to_string(),
            json!(["tcp_client", "tcp_server"]),
        );
        error.details = Some(Box::new(details));

        RpcResponse { id, result: None, error: Some(error) }
    }

    fn is_legacy_hot_apply_kind(kind: &str) -> bool {
        matches!(kind, "tcp_client" | "tcp_server")
    }

    fn interface_identifier(iface: &InterfaceRecord, index: usize) -> String {
        iface
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("{}[{index}]", iface.kind))
    }

    fn is_reload_hot_apply_compatible(current: &[InterfaceRecord], next: &[InterfaceRecord]) -> bool {
        if current.len() != next.len() {
            return false;
        }
        current.iter().zip(next.iter()).all(|(before, after)| {
            before.kind == after.kind && Self::is_legacy_hot_apply_kind(before.kind.as_str())
        })
    }
}

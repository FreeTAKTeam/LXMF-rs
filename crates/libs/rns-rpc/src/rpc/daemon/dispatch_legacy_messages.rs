use super::*;

impl RpcDaemon {
    pub(super) fn handle_rpc_legacy_messages(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
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
                    .filter(|record| !record.peer.trim().is_empty())
                    .cloned()
                    .collect::<Vec<_>>();
                peers.sort_by(|a, b| {
                    b.last_seen.cmp(&a.last_seen).then_with(|| a.peer.cmp(&b.peer))
                });
                let static_peers = self
                    .propagation_state
                    .lock()
                    .expect("propagation mutex poisoned")
                    .static_peers
                    .clone();
                let peers = peers
                    .into_iter()
                    .map(|peer| {
                        let (outgoing, incoming, offered, unhandled) =
                            self.peer_message_stats(peer.peer.as_str()).unwrap_or((0, 0, 0, 0));
                        let peering_key =
                            peer_peering_key_value(&peer, self.identity_hash.as_str());
                        let is_static_peer = peer.peer_type.as_deref() == Some("static")
                            || static_peers.iter().any(|static_peer| {
                                static_peer.eq_ignore_ascii_case(peer.peer.as_str())
                            });
                        let mut row = serde_json::to_value(peer).unwrap_or_else(|_| json!({}));
                        row["type"] = JsonValue::String(
                            if is_static_peer { "static" } else { "discovered" }.to_string(),
                        );
                        row["state"] = JsonValue::from(0);
                        row["sync_strategy"] = JsonValue::from(2);
                        row["ler"] = JsonValue::from(0);
                        row["str"] = JsonValue::from(0);
                        row["messages"] = json!({
                            "offered": offered,
                            "outgoing": outgoing,
                            "incoming": incoming,
                            "unhandled": unhandled,
                        });
                        row["peering_key"] = peering_key.map_or(JsonValue::Null, JsonValue::from);
                        row["last_heard"] =
                            row.get("last_seen").cloned().unwrap_or(JsonValue::Null);
                        row["transfer_limit"] = row
                            .get("propagation_transfer_limit")
                            .cloned()
                            .unwrap_or(JsonValue::Null);
                        row["sync_limit"] =
                            row.get("propagation_sync_limit").cloned().unwrap_or(JsonValue::Null);
                        row["target_stamp_cost"] =
                            row.get("propagation_stamp_cost").cloned().unwrap_or(JsonValue::Null);
                        row["stamp_cost_flexibility"] = row
                            .get("propagation_stamp_cost_flexibility")
                            .cloned()
                            .unwrap_or(JsonValue::Null);
                        row
                    })
                    .collect::<Vec<_>>();
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
                    if iface.kind == "tcp_client" && (iface.host.is_none() || iface.port.is_none())
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
                let blocked = parsed
                    .interfaces
                    .iter()
                    .enumerate()
                    .filter(|(_, iface)| !Self::is_legacy_hot_apply_kind(iface.kind.as_str()))
                    .map(|(index, iface)| Self::interface_identifier(iface, index))
                    .collect::<Vec<_>>();
                if !blocked.is_empty() {
                    return Ok(Self::restart_required_response(
                        request.id,
                        "set_interfaces",
                        blocked,
                    ));
                }
                Self::validate_legacy_hot_apply_uniqueness(&parsed.interfaces)?;
                let parsed_interfaces = parsed.interfaces;

                let applied_interfaces = if let Some(bridge) = self
                    .interface_mutation_bridge
                    .lock()
                    .expect("interface mutation bridge mutex poisoned")
                    .clone()
                {
                    bridge.apply_interfaces(parsed_interfaces)?
                } else {
                    parsed_interfaces
                };
                {
                    let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
                    *guard = applied_interfaces.clone();
                }
                self.update_daemon_status_snapshot(|snapshot| {
                    snapshot.interfaces = applied_interfaces.clone();
                });
                let applied_interface_ids = applied_interfaces
                    .iter()
                    .enumerate()
                    .map(|(index, iface)| Self::interface_identifier(iface, index))
                    .collect::<Vec<_>>();

                let event = RpcEvent {
                    event_type: "interfaces_updated".into(),
                    payload: json!({ "interfaces": applied_interfaces }),
                };
                self.publish_event(event);

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "updated": true,
                        "applied_interfaces": applied_interface_ids,
                        "rejected_interfaces": Vec::<String>::new(),
                    })),
                    error: None,
                })
            }
            "reload_config" => {
                if let Some(params) = request.params.clone() {
                    let parsed: ReloadConfigParams =
                        serde_json::from_value(params).map_err(|err| {
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

                    let current =
                        self.interfaces.lock().expect("interfaces mutex poisoned").clone();
                    if !Self::is_reload_hot_apply_compatible(&current, &parsed.interfaces) {
                        let mut affected = parsed
                            .interfaces
                            .iter()
                            .enumerate()
                            .filter(|(_, iface)| {
                                !Self::is_legacy_hot_apply_kind(iface.kind.as_str())
                            })
                            .map(|(index, iface)| Self::interface_identifier(iface, index))
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
                    Self::validate_legacy_hot_apply_uniqueness(&parsed.interfaces)?;
                    let parsed_interfaces = parsed.interfaces;

                    let applied_interfaces = if let Some(bridge) = self
                        .interface_mutation_bridge
                        .lock()
                        .expect("interface mutation bridge mutex poisoned")
                        .clone()
                    {
                        bridge.apply_interfaces(parsed_interfaces)?
                    } else {
                        parsed_interfaces
                    };
                    {
                        let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
                        *guard = applied_interfaces.clone();
                    }
                    self.update_daemon_status_snapshot(|snapshot| {
                        snapshot.interfaces = applied_interfaces.clone();
                    });
                    let update_event = RpcEvent {
                        event_type: "interfaces_updated".into(),
                        payload: json!({ "interfaces": applied_interfaces }),
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
                let peer_id = parsed.peer.trim();
                if peer_id.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "peer is required",
                    ));
                }

                let timestamp = now_i64();
                let existing_peer_type = self
                    .peers
                    .lock()
                    .expect("peers mutex poisoned")
                    .get(peer_id)
                    .and_then(|record| record.peer_type.clone());
                let peer_type = if self.is_static_peer(peer_id) {
                    Some("static".to_string())
                } else {
                    existing_peer_type.or(Some("manual".to_string()))
                };
                let record = self.upsert_peer(
                    peer_id.to_string(),
                    timestamp,
                    Vec::new(),
                    None,
                    None,
                    peer_type,
                )?;
                {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    if let Some(existing) = guard.get_mut(&record.peer) {
                        existing.last_sync_attempt = timestamp;
                        existing.alive = true;
                        existing.sync_backoff = 0;
                        existing.next_sync_attempt = 0;
                    }
                }
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
                let peer_id = parsed.peer.trim();
                if peer_id.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "peer is required",
                    ));
                }

                let removed = {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    let removed = guard.remove(peer_id).is_some();
                    let peer_count = Self::active_peer_count_from_guard(&guard);
                    drop(guard);
                    self.update_daemon_status_snapshot(|snapshot| {
                        snapshot.peer_count = peer_count;
                    });
                    removed
                };
                let event = RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: json!({ "peer": peer_id, "removed": removed }),
                };
                self.publish_event(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "removed": removed })),
                    error: None,
                })
            }
            "sdk_send_batch_v2" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed = parse_outbound_send_batch_request(params)?;
                self.store_outbound_batch(request.id, parsed)
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
                self.store_inbound_record(record, None)?;
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
                let (status, updated, delivered_ticket_destination) = {
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
                        (requested_status, false, None)
                    } else if existing_status.as_deref().is_some_and(|status| {
                        Self::is_terminal_receipt_status(status)
                            || Self::is_receipt_status_regression(status, &requested_status)
                    }) {
                        (existing_status.unwrap_or(requested_status), false, None)
                    } else {
                        let delivered_ticket_destination = existing_message
                            .as_ref()
                            .filter(|message| {
                                requested_status.eq_ignore_ascii_case("delivered")
                                    && Self::message_requested_ticket(message)
                            })
                            .map(|message| message.destination.clone());
                        self.store
                            .update_receipt_status(&message_id, &requested_status)
                            .map_err(std::io::Error::other)?;
                        (requested_status, true, delivered_ticket_destination)
                    }
                };
                if updated {
                    self.append_delivery_trace(&message_id, status.clone());
                }
                if let Some(destination) = delivered_ticket_destination {
                    self.mark_ticket_delivered(destination.as_str());
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
            "get_outbound_progress"
            | "get_outbound_lxm_stamp_cost"
            | "get_outbound_lxm_propagation_stamp_cost" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: OutboundLxmQueryParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let lookup = parsed
                    .message_id
                    .or(parsed.lxm_hash)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "message_id or lxm_hash is required",
                        )
                    })?;
                let message = self.outbound_message_for_query(lookup.as_str())?;
                let result = match request.method.as_str() {
                    "get_outbound_progress" => {
                        json!({
                            "message_id": message.as_ref().map(|message| message.id.clone()),
                            "progress": message.as_ref().and_then(Self::outbound_progress_for_message),
                            "meta": self.response_meta(),
                        })
                    }
                    "get_outbound_lxm_stamp_cost" => {
                        json!({
                            "message_id": message.as_ref().map(|message| message.id.clone()),
                            "stamp_cost": message.as_ref().and_then(Self::outbound_stamp_cost_for_message),
                            "meta": self.response_meta(),
                        })
                    }
                    "get_outbound_lxm_propagation_stamp_cost" => {
                        json!({
                            "message_id": message.as_ref().map(|message| message.id.clone()),
                            "propagation_stamp_cost": message
                                .as_ref()
                                .and_then(Self::outbound_propagation_stamp_cost_for_message),
                            "meta": self.response_meta(),
                        })
                    }
                    _ => unreachable!("outbound LXM query method: {}", request.method),
                };
                Ok(RpcResponse { id: request.id, result: Some(result), error: None })
            }
            _ => unreachable!("legacy message route: {}", request.method),
        }
    }

    fn outbound_message_for_query(
        &self,
        lookup: &str,
    ) -> Result<Option<MessageRecord>, std::io::Error> {
        if let Some(message) = self.store.get_message(lookup).map_err(std::io::Error::other)? {
            return Ok(Some(message));
        }

        let messages = self.store.list_messages(500, None).map_err(std::io::Error::other)?;
        Ok(messages.into_iter().find(|message| {
            message.id == lookup || Self::message_lxmf_field_matches(message, lookup)
        }))
    }

    fn message_lxmf_field_matches(message: &MessageRecord, lookup: &str) -> bool {
        Self::message_lxmf(message).is_some_and(|lxmf| {
            ["message_id", "lxm_hash", "hash", "transient_id", "propagation_transient_id"]
                .iter()
                .any(|key| lxmf.get(*key).and_then(JsonValue::as_str) == Some(lookup))
        })
    }

    fn outbound_progress_for_message(message: &MessageRecord) -> Option<f64> {
        if message.direction != "out" {
            return None;
        }
        if let Some(status) = message.receipt_status.as_deref() {
            let normalized = status.trim().to_ascii_lowercase();
            if normalized.starts_with("sent") || normalized == "delivered" {
                return Some(1.0);
            }
            if normalized.starts_with("failed")
                || matches!(normalized.as_str(), "cancelled" | "expired" | "rejected")
            {
                return None;
            }
        }
        let lxmf = Self::message_lxmf(message);
        if let Some(progress) =
            lxmf.and_then(|lxmf| lxmf.get("progress")).and_then(JsonValue::as_f64)
        {
            return Some(progress.clamp(0.0, 1.0));
        }
        match lxmf
            .and_then(|lxmf| lxmf.get("stamp_state"))
            .and_then(JsonValue::as_str)
            .map(|state| state.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("failed" | "cancelled") => None,
            Some("generating") => Some(0.0),
            _ => match lxmf
                .and_then(|lxmf| lxmf.get("propagation_stamp_state"))
                .and_then(JsonValue::as_str)
                .map(|state| state.trim().to_ascii_lowercase())
                .as_deref()
            {
                Some("failed" | "cancelled") => None,
                Some("generating") => Some(0.0),
                _ if message.receipt_status.as_deref().is_some_and(|status| {
                    matches!(status.trim().to_ascii_lowercase().as_str(), "queued" | "sending")
                }) =>
                {
                    Some(0.01)
                }
                _ => Some(0.0),
            },
        }
    }

    fn outbound_stamp_cost_for_message(message: &MessageRecord) -> Option<u32> {
        if message.direction != "out" {
            return None;
        }
        if message.receipt_status.as_deref().is_some_and(Self::outbound_query_terminal_status) {
            return None;
        }
        let lxmf = Self::message_lxmf(message)?;
        if Self::has_outbound_ticket_marker(lxmf.get("outbound_ticket"))
            || Self::has_outbound_ticket_marker(lxmf.get("stamp_ticket_source"))
            || lxmf.get("stamp_kind").and_then(JsonValue::as_str) == Some("ticket")
        {
            return None;
        }
        Self::json_u32(lxmf.get("stamp_cost"))
            .or_else(|| Self::json_u32(lxmf.get("stamp_target_cost")))
    }

    fn outbound_propagation_stamp_cost_for_message(message: &MessageRecord) -> Option<u32> {
        if message.direction != "out" {
            return None;
        }
        if message.receipt_status.as_deref().is_some_and(Self::outbound_query_terminal_status) {
            return None;
        }
        let lxmf = Self::message_lxmf(message)?;
        Self::json_u32(lxmf.get("propagation_target_cost"))
            .or_else(|| Self::json_u32(lxmf.get("propagation_stamp_target_cost")))
    }

    fn has_outbound_ticket_marker(value: Option<&JsonValue>) -> bool {
        match value {
            Some(JsonValue::String(ticket)) => !ticket.trim().is_empty(),
            Some(JsonValue::Null) | None => false,
            Some(_) => true,
        }
    }

    fn outbound_query_terminal_status(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        normalized.starts_with("sent")
            || normalized.starts_with("failed")
            || matches!(normalized.as_str(), "delivered" | "cancelled" | "expired" | "rejected")
    }

    fn message_lxmf(message: &MessageRecord) -> Option<&serde_json::Map<String, JsonValue>> {
        let JsonValue::Object(fields) = message.fields.as_ref()? else {
            return None;
        };
        let JsonValue::Object(lxmf) = fields.get("_lxmf")? else {
            return None;
        };
        Some(lxmf)
    }

    fn json_u32(value: Option<&JsonValue>) -> Option<u32> {
        match value? {
            JsonValue::Number(number) => {
                number.as_u64().and_then(|value| u32::try_from(value).ok()).or_else(|| {
                    let value = number.as_f64()?;
                    (value.is_finite()
                        && value.fract() == 0.0
                        && value >= 0.0
                        && value <= f64::from(u32::MAX))
                    .then_some(value as u32)
                })
            }
            JsonValue::String(value) => Self::string_u32(value),
            _ => None,
        }
    }

    fn string_u32(value: &str) -> Option<u32> {
        let value = value.trim();
        value.parse::<u32>().ok().or_else(|| {
            let value = value.parse::<f64>().ok()?;
            (value.is_finite()
                && value.fract() == 0.0
                && value >= 0.0
                && value <= f64::from(u32::MAX))
            .then_some(value as u32)
        })
    }

    fn message_requested_ticket(message: &MessageRecord) -> bool {
        Self::message_lxmf(message)
            .and_then(|lxmf| lxmf.get("include_ticket"))
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
    }

    pub(super) fn restart_required_response(
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
        details.insert("operation".to_string(), JsonValue::String(operation.to_string()));
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

    pub(super) fn is_legacy_hot_apply_kind(kind: &str) -> bool {
        matches!(kind, "tcp_client" | "tcp_server")
    }

    pub(super) fn interface_identifier(iface: &InterfaceRecord, index: usize) -> String {
        iface
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("{}[{index}]", iface.kind))
    }

    pub(super) fn is_reload_hot_apply_compatible(
        current: &[InterfaceRecord],
        next: &[InterfaceRecord],
    ) -> bool {
        if current.len() != next.len() {
            return false;
        }
        current.iter().zip(next.iter()).all(|(before, after)| {
            before.kind == after.kind && Self::is_legacy_hot_apply_kind(before.kind.as_str())
        })
    }

    pub(super) fn validate_legacy_hot_apply_uniqueness(
        interfaces: &[InterfaceRecord],
    ) -> Result<(), std::io::Error> {
        let mut seen = std::collections::HashSet::new();
        for (index, iface) in interfaces.iter().enumerate() {
            if iface.kind != "tcp_client" {
                continue;
            }
            let Some(key) = Self::legacy_tcp_interface_key(iface) else {
                continue;
            };
            if !seen.insert(key.clone()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "duplicate legacy tcp interface key '{}' at {}",
                        key,
                        Self::interface_identifier(iface, index)
                    ),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn legacy_tcp_interface_key(iface: &InterfaceRecord) -> Option<String> {
        if iface.kind != "tcp_client" {
            return None;
        }
        if let Some(name) = iface.name.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            return Some(name.to_string());
        }
        let host = iface.host.as_deref()?.trim();
        let port = iface.port?;
        Some(format!("{host}:{port}"))
    }
}

fn peer_peering_key_value(peer: &PeerRecord, local_identity_hash: &str) -> Option<u32> {
    let peering_cost = peer.peering_cost?;
    let remote_hash = decode_truncated_hash(peer.peer.as_str())?;
    let local_hash = decode_truncated_hash(local_identity_hash)?;
    let mut material = Vec::with_capacity(remote_hash.len() + local_hash.len());
    material.extend_from_slice(remote_hash.as_slice());
    material.extend_from_slice(local_hash.as_slice());
    generate_peering_key_value(material.as_slice(), peering_cost)
}

fn decode_truncated_hash(value: &str) -> Option<Vec<u8>> {
    let bytes = hex::decode(value.trim()).ok()?;
    (bytes.len() == 16).then_some(bytes)
}

fn generate_peering_key_value(material: &[u8], target_cost: u32) -> Option<u32> {
    use hkdf::Hkdf;

    const PEERING_WORKBLOCK_EXPAND_ROUNDS: usize = 25;

    let mut workblock = Vec::with_capacity(PEERING_WORKBLOCK_EXPAND_ROUNDS * 256);
    for n in 0..PEERING_WORKBLOCK_EXPAND_ROUNDS {
        let mut salt_data = Vec::with_capacity(material.len() + 8);
        salt_data.extend_from_slice(material);
        let packed = rmp_serde::to_vec(&n).ok()?;
        salt_data.extend_from_slice(&packed);
        let salt_hash = Sha256::digest(&salt_data);
        let hk = Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), material);
        let mut okm = [0u8; 256];
        hk.expand(&[], &mut okm).ok()?;
        workblock.extend_from_slice(&okm);
    }

    let mut workblock_hasher = Sha256::new();
    workblock_hasher.update(&workblock);
    let mut nonce = 0u64;
    loop {
        let stamp = nonce.to_le_bytes();
        let value = stamp_value_with_prefix(&workblock_hasher, &stamp);
        if value >= target_cost {
            return Some(value);
        }
        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            return None;
        }
    }
}

fn stamp_value_with_prefix(workblock_hasher: &Sha256, stamp: &[u8]) -> u32 {
    let mut hasher = workblock_hasher.clone();
    hasher.update(stamp);
    stamp_value_from_hash(hasher.finalize().as_slice())
}

fn stamp_value_from_hash(hash: &[u8]) -> u32 {
    let mut value = 0u32;
    for byte in hash {
        if *byte == 0 {
            value += 8;
        } else {
            value += byte.leading_zeros();
            break;
        }
    }
    value
}

impl RpcDaemon {
    fn handle_rpc_legacy_message_delivery(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
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

                let cleanup = self.unpeer_local_state(peer_id)?;
                let offered = cleanup.messages["offered"].as_u64().unwrap_or(0);
                let outgoing = cleanup.messages["outgoing"].as_u64().unwrap_or(0);
                let incoming = cleanup.messages["incoming"].as_u64().unwrap_or(0);
                let event = RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: json!({
                        "peer": cleanup.peer.as_str(),
                        "removed": cleanup.removed,
                        "propagation_cleared": cleanup.propagation_cleared,
                        "propagation_cleared_bytes": cleanup.propagation_cleared_bytes,
                        "offered": offered,
                        "outgoing": outgoing,
                        "incoming": incoming,
                        "messages": cleanup.messages,
                    }),
                };
                self.publish_event(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "peer": cleanup.peer.as_str(),
                        "removed": cleanup.removed,
                        "propagation_cleared": cleanup.propagation_cleared,
                        "propagation_cleared_bytes": cleanup.propagation_cleared_bytes,
                        "offered": offered,
                        "outgoing": outgoing,
                        "incoming": incoming,
                        "messages": cleanup.messages,
                    })),
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
                let RecordReceiptParams {
                    message_id,
                    status: requested_status,
                    packet_hash,
                    resource_hash,
                    peer,
                    method,
                    delivery_kind,
                    bytes,
                    link_id,
                } = parsed;
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
                let mut payload = JsonMap::new();
                payload.insert("message_id".into(), json!(message_id));
                payload.insert("status".into(), json!(status));
                payload.insert("updated".into(), json!(updated));
                payload.insert("reason_code".into(), json!(reason_code));
                if let Some(packet_hash) = packet_hash {
                    payload.insert("packet_hash".into(), json!(packet_hash));
                }
                if let Some(resource_hash) = resource_hash {
                    payload.insert("resource_hash".into(), json!(resource_hash));
                }
                if let Some(peer) = peer {
                    payload.insert("peer".into(), json!(peer));
                }
                if let Some(method) = method {
                    payload.insert("method".into(), json!(method));
                }
                if let Some(delivery_kind) = delivery_kind {
                    payload.insert("delivery_kind".into(), json!(delivery_kind));
                }
                if let Some(bytes) = bytes {
                    payload.insert("bytes".into(), json!(bytes));
                }
                if let Some(link_id) = link_id {
                    payload.insert("link_id".into(), json!(link_id));
                }
                let payload = JsonValue::Object(payload);
                let event = RpcEvent { event_type: "receipt".into(), payload: payload.clone() };
                self.publish_event(event);
                Ok(RpcResponse { id: request.id, result: Some(payload), error: None })
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
            _ => unreachable!("legacy message delivery route: {}", request.method),
        }
    }
}

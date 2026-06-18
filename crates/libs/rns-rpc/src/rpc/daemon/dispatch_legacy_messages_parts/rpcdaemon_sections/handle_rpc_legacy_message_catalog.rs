impl RpcDaemon {
    fn handle_rpc_legacy_message_catalog(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "list_messages" => {
                let parsed = request
                    .params
                    .map(serde_json::from_value::<ListMessagesParams>)
                    .transpose()
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
                    .unwrap_or_default();
                let limit = parsed.limit.unwrap_or(100).clamp(1, 5000);
                let (before_ts, before_id) = match parsed.before_ts {
                    Some(timestamp) => (Some(timestamp), None),
                    None => {
                        parse_timestamp_id_cursor(parsed.cursor.as_deref()).unwrap_or((None, None))
                    }
                };
                let include_receipts = parsed.include_receipts.unwrap_or(true);
                let peer_id =
                    parsed.peer_id.as_deref().map(str::trim).filter(|value| !value.is_empty());
                let conversation_id = parsed
                    .conversation_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if let (Some(peer_id), Some(conversation_id)) = (peer_id, conversation_id) {
                    if !peer_id.eq_ignore_ascii_case(conversation_id) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "peer_id and conversation_id must match when both are set",
                        ));
                    }
                }
                let peer_filter = peer_id.or(conversation_id);
                let page_limit = limit.saturating_add(1);
                let mut items = if let Some(peer) = peer_filter {
                    self.store
                        .list_messages_page_for_peer(
                            page_limit,
                            before_ts,
                            before_id.as_deref(),
                            peer,
                        )
                        .map_err(std::io::Error::other)?
                } else {
                    self.store
                        .list_messages_page(page_limit, before_ts, before_id.as_deref())
                        .map_err(std::io::Error::other)?
                };
                let has_more = items.len() > limit;
                if has_more {
                    items.truncate(limit);
                }
                if !include_receipts {
                    for item in &mut items {
                        item.receipt_status = None;
                    }
                }
                let next_cursor = if has_more {
                    items.last().map(|record| format!("{}:{}", record.timestamp, record.id))
                } else {
                    None
                };
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "messages": items,
                        "next_cursor": next_cursor,
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
                let page_limit = limit.saturating_add(1);
                let mut items = self
                    .store
                    .list_announces(page_limit, before_ts, before_id.as_deref())
                    .map_err(std::io::Error::other)?;
                let has_more = items.len() > limit;
                if has_more {
                    items.truncate(limit);
                }
                let next_cursor = if has_more {
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
                let peers = self
                    .peers
                    .lock()
                    .expect("peers mutex poisoned")
                    .values()
                    .filter(|record| !record.peer.trim().is_empty())
                    .cloned()
                    .collect::<Vec<_>>();
                for peer in &peers {
                    self.restore_peer_record_queue_marks(peer)?;
                }
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
                let peers = peers
                    .into_iter()
                    .map(|peer| self.enriched_peer_status_row(peer))
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
            _ => unreachable!("legacy message catalog route: {}", request.method),
        }
    }
}

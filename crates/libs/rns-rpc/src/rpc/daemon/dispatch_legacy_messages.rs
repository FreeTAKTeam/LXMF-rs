use super::init::LXMF_PEER_SYNC_BACKOFF_STEP_SECS;
use super::*;

impl RpcDaemon {
    pub(super) fn enriched_peer_status_row(&self, peer: PeerRecord) -> JsonValue {
        let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes) =
            self.peer_message_stats(peer.peer.as_str()).unwrap_or((0, 0, 0, 0, 0, 0));
        let peering_key = peer_peering_key_value(&peer, self.identity_hash.as_str());
        let peering_key_status = peer_peering_key_status(&peer, peering_key);
        let acceptance_rate =
            peer_acceptance_rate_for_reporting(peer.acceptance_rate, outgoing, offered, peer.alive);
        let handled_ids =
            self.store.list_peer_handled_propagation_ids(peer.peer.as_str()).unwrap_or_default();
        let unhandled_ids =
            self.store.list_peer_unhandled_propagation_ids(peer.peer.as_str()).unwrap_or_default();
        let is_static_peer = self.is_static_peer(peer.peer.as_str());
        let mut row = serde_json::to_value(peer).unwrap_or_else(|_| json!({}));
        row["type"] =
            JsonValue::String(if is_static_peer { "static" } else { "discovered" }.to_string());
        row["state"] = JsonValue::from(0);
        row["sync_strategy"] = JsonValue::from(2);
        row["ler"] = JsonValue::from(0);
        row["str"] = row
            .get("sync_transfer_rate")
            .and_then(JsonValue::as_f64)
            .map(|value| JsonValue::from(value as u64))
            .unwrap_or_else(|| JsonValue::from(0));
        row["messages"] = json!({
            "offered": offered,
            "outgoing": outgoing,
            "incoming": incoming,
            "unhandled": unhandled,
            "offered_bytes": offered_bytes,
            "unhandled_bytes": unhandled_bytes,
            "handled_ids": handled_ids,
            "unhandled_ids": unhandled_ids,
        });
        row["offered"] = json!(offered);
        row["outgoing"] = json!(outgoing);
        row["incoming"] = json!(incoming);
        row["handled_ids"] = json!(handled_ids);
        row["unhandled_ids"] = json!(unhandled_ids);
        row["acceptance_rate"] = json!(acceptance_rate);
        row["peering_key"] = peering_key.map_or(JsonValue::Null, JsonValue::from);
        row["peering_key_status"] = json!(peering_key_status);
        row["last_heard"] = row.get("last_seen").cloned().unwrap_or(JsonValue::Null);
        row["transfer_limit"] =
            row.get("propagation_transfer_limit").cloned().unwrap_or(JsonValue::Null);
        row["sync_limit"] = row.get("propagation_sync_limit").cloned().unwrap_or(JsonValue::Null);
        row["target_stamp_cost"] =
            row.get("propagation_stamp_cost").cloned().unwrap_or(JsonValue::Null);
        row["stamp_cost_flexibility"] =
            row.get("propagation_stamp_cost_flexibility").cloned().unwrap_or(JsonValue::Null);
        row
    }

    pub(super) fn postponed_peer_sync_response(
        &self,
        request_id: u64,
        record: &PeerRecord,
        timestamp: i64,
        postpone_reason: &str,
        transfer_limit_bytes: Option<usize>,
        sync_limit_bytes: Option<usize>,
    ) -> RpcResponse {
        let (
            acceptance_rate,
            last_sync_attempt,
            next_sync_attempt,
            sync_backoff,
            sync_transfer_rate,
            alive,
        ) = {
            let mut guard = self.peers.lock().expect("peers mutex poisoned");
            if let Some(existing) = guard.get_mut(&record.peer) {
                existing.last_sync_attempt = timestamp;
                if postpone_reason == "backoff" {
                    existing.alive = existing.last_sync_attempt < existing.last_seen;
                }
                (
                    existing.acceptance_rate,
                    existing.last_sync_attempt,
                    existing.next_sync_attempt,
                    existing.sync_backoff,
                    existing.sync_transfer_rate,
                    existing.alive,
                )
            } else {
                (
                    record.acceptance_rate,
                    timestamp,
                    record.next_sync_attempt,
                    record.sync_backoff,
                    record.sync_transfer_rate,
                    record.alive,
                )
            }
        };
        let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes) =
            self.peer_message_stats(record.peer.as_str()).unwrap_or((0, 0, 0, 0, 0, 0));
        let acceptance_rate =
            peer_acceptance_rate_for_reporting(acceptance_rate, outgoing, offered, alive);
        let handled_ids =
            self.store.list_peer_handled_propagation_ids(record.peer.as_str()).unwrap_or_default();
        let unhandled_ids = self
            .store
            .list_peer_unhandled_propagation_ids(record.peer.as_str())
            .unwrap_or_default();
        let messages = json!({
            "offered": offered,
            "outgoing": outgoing,
            "incoming": incoming,
            "unhandled": unhandled,
            "offered_bytes": offered_bytes,
            "unhandled_bytes": unhandled_bytes,
            "handled_ids": handled_ids,
            "unhandled_ids": unhandled_ids,
        });
        let propagation_sync = json!({
            "synced": false,
            "postponed": true,
            "postpone_reason": postpone_reason,
            "handled": 0,
            "skipped": 0,
            "rejected": 0,
            "offered": 0,
            "bytes": 0,
            "offered_bytes": 0,
            "rejected_bytes": 0,
            "remaining": 0,
            "remaining_bytes": 0,
            "handled_ids": [],
            "skipped_ids": [],
            "rejected_ids": [],
            "transfer_limited": 0,
            "transfer_limited_bytes": 0,
            "transfer_limited_ids": [],
            "messages": [],
            "transfer_limit": transfer_limit_bytes,
            "sync_limit": sync_limit_bytes,
            "target_stamp_cost": record.propagation_stamp_cost,
            "stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
        });
        let peer_type_value = record.peer_type.clone();
        let peer_status_type =
            if self.is_static_peer(record.peer.as_str()) { "static" } else { "discovered" };
        let peering_key = peer_peering_key_value(record, self.identity_hash.as_str());
        let peering_key_status = peer_peering_key_status(record, peering_key);
        let mut propagation_sync = propagation_sync;
        propagation_sync["peering_key"] = peering_key.map_or(JsonValue::Null, JsonValue::from);
        propagation_sync["peering_key_status"] = json!(peering_key_status);
        let event = RpcEvent {
            event_type: "peer_sync".into(),
            payload: json!({
                "peer": &record.peer,
                "peer_type": peer_type_value,
                "type": peer_status_type,
                "timestamp": timestamp,
                "name": &record.name,
                "name_source": &record.name_source,
                "last_heard": record.last_seen,
                "first_seen": record.first_seen,
                "seen_count": record.seen_count,
                "state": 0,
                "sync_strategy": 2,
                "ler": 0,
                "peering_timebase": record.peering_timebase,
                "network_distance": record.network_distance,
                "rx_bytes": record.rx_bytes,
                "tx_bytes": record.tx_bytes,
                "alive": alive,
                "acceptance_rate": acceptance_rate,
                "last_sync_attempt": last_sync_attempt,
                "next_sync_attempt": next_sync_attempt,
                "sync_backoff": sync_backoff,
                "sync_transfer_rate": sync_transfer_rate,
                "str": sync_transfer_rate as u64,
                "synced": false,
                "postponed": true,
                "postpone_reason": postpone_reason,
                "propagation_transfer_limit": record.propagation_transfer_limit,
                "propagation_sync_limit": record.propagation_sync_limit,
                "propagation_stamp_cost": record.propagation_stamp_cost,
                "propagation_stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                "peering_key": peering_key,
                "peering_key_status": peering_key_status,
                "transfer_limit": transfer_limit_bytes,
                "sync_limit": sync_limit_bytes,
                "target_stamp_cost": record.propagation_stamp_cost,
                "stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                "offered": offered,
                "outgoing": outgoing,
                "incoming": incoming,
                "messages": messages,
                "propagation": propagation_sync.clone(),
            }),
        };
        self.publish_event(event);

        RpcResponse {
            id: request_id,
            result: Some(json!({
                "peer": &record.peer,
                "peer_type": peer_type_value,
                "type": peer_status_type,
                "name": &record.name,
                "name_source": &record.name_source,
                "first_seen": record.first_seen,
                "seen_count": record.seen_count,
                "synced": false,
                "postponed": true,
                "postpone_reason": postpone_reason,
                "state": 0,
                "sync_strategy": 2,
                "ler": 0,
                "peering_timebase": record.peering_timebase,
                "network_distance": record.network_distance,
                "rx_bytes": record.rx_bytes,
                "tx_bytes": record.tx_bytes,
                "alive": alive,
                "acceptance_rate": acceptance_rate,
                "last_heard": record.last_seen,
                "last_sync_attempt": last_sync_attempt,
                "next_sync_attempt": next_sync_attempt,
                "sync_backoff": sync_backoff,
                "sync_transfer_rate": sync_transfer_rate,
                "str": sync_transfer_rate as u64,
                "propagation_transfer_limit": record.propagation_transfer_limit,
                "propagation_sync_limit": record.propagation_sync_limit,
                "propagation_stamp_cost": record.propagation_stamp_cost,
                "propagation_stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                "peering_key": peering_key,
                "peering_key_status": peering_key_status,
                "transfer_limit": transfer_limit_bytes,
                "sync_limit": sync_limit_bytes,
                "target_stamp_cost": record.propagation_stamp_cost,
                "stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                "offered": offered,
                "outgoing": outgoing,
                "incoming": incoming,
                "messages": messages,
                "propagation": propagation_sync,
            })),
            error: None,
        }
    }

    pub(super) fn handle_rpc_legacy_messages(
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
                let page_limit = limit.saturating_add(1);
                let mut items = self
                    .store
                    .list_messages_page(page_limit, before_ts, before_id.as_deref())
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
                let wanted_ids = canonical_peer_sync_wanted_ids(parsed.wanted_ids.as_ref())?;
                let requested_transfer_limit_bytes =
                    parsed.transfer_limit_kb.map(|limit| (limit.max(0.0) * 1000.0) as usize);

                let timestamp = now_i64();
                let prioritised_destinations = self
                    .delivery_policy
                    .lock()
                    .expect("policy mutex poisoned")
                    .prioritised_destinations
                    .clone();
                let existing_peer =
                    self.peers.lock().expect("peers mutex poisoned").get(peer_id).cloned();
                if existing_peer.is_none()
                    && wanted_ids.as_ref().is_some_and(PeerSyncWantedIds::requires_offer_validation)
                {
                    let mut prospective_propagation = self
                        .store
                        .list_peer_prospective_unhandled_propagation(peer_id)
                        .map_err(std::io::Error::other)?;
                    prospective_propagation.sort_by(|left, right| {
                        let left_weight = propagation_peer_sync_weight(
                            left,
                            timestamp,
                            prioritised_destinations.as_slice(),
                        );
                        let right_weight = propagation_peer_sync_weight(
                            right,
                            timestamp,
                            prioritised_destinations.as_slice(),
                        );
                        left_weight
                            .partial_cmp(&right_weight)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| left.transient_id.cmp(&right.transient_id))
                    });
                    validate_peer_sync_wanted_ids_in_offer(
                        wanted_ids.as_ref(),
                        prospective_propagation.as_slice(),
                        requested_transfer_limit_bytes,
                        requested_transfer_limit_bytes,
                    )?;
                }
                if let Some(record) = existing_peer.as_ref() {
                    let record_transfer_limit_bytes =
                        record.propagation_transfer_limit.map(|limit| limit as usize);
                    let transfer_limit_bytes =
                        match (record_transfer_limit_bytes, requested_transfer_limit_bytes) {
                            (Some(record_limit), Some(requested_limit)) => {
                                Some(record_limit.min(requested_limit))
                            }
                            (Some(record_limit), None) => Some(record_limit),
                            (None, Some(requested_limit)) => Some(requested_limit),
                            (None, None) => None,
                        };
                    let sync_limit_bytes = record
                        .propagation_sync_limit
                        .map(|limit| limit as usize)
                        .or(transfer_limit_bytes);
                    if record.next_sync_attempt > 0 && timestamp < record.next_sync_attempt {
                        return Ok(self.postponed_peer_sync_response(
                            request.id,
                            record,
                            timestamp,
                            "backoff",
                            transfer_limit_bytes,
                            sync_limit_bytes,
                        ));
                    }
                }
                let existing_peer_type =
                    existing_peer.as_ref().and_then(|record| record.peer_type.clone());
                let prior_peer_seen =
                    existing_peer.as_ref().map(|record| (record.last_seen, record.seen_count));
                let peer_type = if self.is_static_peer(peer_id) {
                    Some("static".to_string())
                } else if existing_peer_type.as_deref() == Some("unpeered") {
                    Some("manual".to_string())
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
                self.queue_existing_propagation_for_peer(record.peer.as_str())?;
                let record_transfer_limit_bytes =
                    record.propagation_transfer_limit.map(|limit| limit as usize);
                let explicit_peer_sync_selection =
                    wanted_ids.as_ref().is_some_and(PeerSyncWantedIds::requires_offer_validation);
                let transfer_limit_bytes =
                    match (record_transfer_limit_bytes, requested_transfer_limit_bytes) {
                        (Some(record_limit), Some(requested_limit)) => {
                            Some(record_limit.min(requested_limit))
                        }
                        (Some(record_limit), None) => Some(record_limit),
                        (None, Some(requested_limit)) => Some(requested_limit),
                        (None, None) => None,
                    };
                let sync_limit_bytes = record
                    .propagation_sync_limit
                    .map(|limit| limit as usize)
                    .or(transfer_limit_bytes);
                if record.next_sync_attempt > 0 && timestamp < record.next_sync_attempt {
                    return Ok(self.postponed_peer_sync_response(
                        request.id,
                        &record,
                        timestamp,
                        "backoff",
                        transfer_limit_bytes,
                        sync_limit_bytes,
                    ));
                }
                self.store
                    .remove_stale_peer_unhandled_propagation(peer_id)
                    .map_err(std::io::Error::other)?;
                let mut pending_propagation = self
                    .store
                    .list_peer_unhandled_propagation(peer_id)
                    .map_err(std::io::Error::other)?;
                let mut propagation_transfer_limited = 0usize;
                let mut propagation_transfer_limited_bytes = 0u64;
                let mut propagation_transfer_limited_ids = Vec::new();
                let mut propagation_rejected = 0usize;
                let mut propagation_rejected_bytes = 0u64;
                let mut propagation_rejected_ids = Vec::new();
                pending_propagation.sort_by(|left, right| {
                    let left_weight = propagation_peer_sync_weight(
                        left,
                        timestamp,
                        prioritised_destinations.as_slice(),
                    );
                    let right_weight = propagation_peer_sync_weight(
                        right,
                        timestamp,
                        prioritised_destinations.as_slice(),
                    );
                    left_weight
                        .partial_cmp(&right_weight)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| left.transient_id.cmp(&right.transient_id))
                });
                validate_peer_sync_wanted_ids_in_offer(
                    wanted_ids.as_ref(),
                    pending_propagation.as_slice(),
                    transfer_limit_bytes,
                    sync_limit_bytes,
                )?;
                let (policy_relevant_pending, policy_relevant_has_stamp) =
                    peer_sync_policy_relevance(
                        pending_propagation.as_slice(),
                        wanted_ids.as_ref(),
                        sync_limit_bytes,
                    );
                if policy_relevant_pending > 0 && policy_relevant_has_stamp {
                    if let Some(min_accepted_stamp_value) =
                        peer_minimum_accepted_stamp_value(&record)
                    {
                        let mut accepted_propagation =
                            Vec::with_capacity(pending_propagation.len());
                        for entry in pending_propagation {
                            if entry
                                .stamp_value
                                .is_some_and(|value| value < min_accepted_stamp_value)
                            {
                                propagation_rejected = propagation_rejected.saturating_add(1);
                                propagation_rejected_bytes =
                                    propagation_rejected_bytes.saturating_add(entry.size_bytes);
                                propagation_rejected_ids.push(entry.transient_id.clone());
                                self.store
                                    .remove_peer_unhandled_propagation(
                                        peer_id,
                                        entry.transient_id.as_str(),
                                    )
                                    .map_err(std::io::Error::other)?;
                                continue;
                            }
                            accepted_propagation.push(entry);
                        }
                        pending_propagation = accepted_propagation;
                    }
                }
                if let Some(limit) = transfer_limit_bytes {
                    let mut candidates = Vec::with_capacity(pending_propagation.len());
                    for entry in pending_propagation {
                        let entry_size = usize::try_from(entry.size_bytes).unwrap_or(usize::MAX);
                        let transfer_size = entry_size.saturating_add(16);
                        let wanted = wanted_ids
                            .as_ref()
                            .map_or(true, |ids| ids.wants(entry.transient_id.as_str()));
                        if wanted && transfer_size > limit {
                            propagation_transfer_limited =
                                propagation_transfer_limited.saturating_add(1);
                            propagation_transfer_limited_bytes =
                                propagation_transfer_limited_bytes.saturating_add(entry.size_bytes);
                            let transient_id = entry.transient_id;
                            self.store
                                .mark_peer_transfer_limited_propagation(
                                    peer_id,
                                    transient_id.as_str(),
                                )
                                .map_err(std::io::Error::other)?;
                            propagation_transfer_limited_ids.push(transient_id);
                            continue;
                        }
                        candidates.push(entry);
                    }
                    pending_propagation = candidates;
                }
                let (remaining_policy_relevant, remaining_policy_relevant_has_stamp) =
                    peer_sync_policy_relevance(
                        pending_propagation.as_slice(),
                        wanted_ids.as_ref(),
                        sync_limit_bytes,
                    );
                let peer_policy_required = remaining_policy_relevant > 0
                    && (!explicit_peer_sync_selection
                        || wanted_ids.is_some()
                        || remaining_policy_relevant_has_stamp
                        || peer_stamp_policy_partially_known(&record));
                if peer_policy_required && !peer_stamp_policy_known(&record) {
                    return Ok(self.postponed_peer_sync_response(
                        request.id,
                        &record,
                        timestamp,
                        "stamp_policy",
                        transfer_limit_bytes,
                        sync_limit_bytes,
                    ));
                }
                if peer_policy_required
                    && peer_peering_key_value(&record, self.identity_hash.as_str()).is_none()
                {
                    return Ok(self.postponed_peer_sync_response(
                        request.id,
                        &record,
                        timestamp,
                        "peering_key",
                        transfer_limit_bytes,
                        sync_limit_bytes,
                    ));
                }
                let mut cumulative_size = 24usize;
                let mut propagation_handled = 0usize;
                let mut propagation_transferred = 0usize;
                let mut propagation_skipped = 0usize;
                let mut propagation_bytes = 0u64;
                let mut propagation_offered_bytes = 0u64;
                let mut propagation_remaining_bytes = 0u64;
                let mut propagation_handled_ids = Vec::new();
                let mut propagation_transferred_ids = Vec::new();
                let mut propagation_skipped_ids = Vec::new();
                let mut propagation_messages = Vec::new();
                let mut propagation_resource_payloads = Vec::new();
                for entry in pending_propagation {
                    let entry_size = usize::try_from(entry.size_bytes).unwrap_or(usize::MAX);
                    let transfer_size = entry_size.saturating_add(16);
                    if transfer_limit_bytes.is_some_and(|limit| transfer_size > limit) {
                        propagation_transfer_limited =
                            propagation_transfer_limited.saturating_add(1);
                        propagation_transfer_limited_bytes =
                            propagation_transfer_limited_bytes.saturating_add(entry.size_bytes);
                        let transient_id = entry.transient_id;
                        self.store
                            .mark_peer_transfer_limited_propagation(peer_id, transient_id.as_str())
                            .map_err(std::io::Error::other)?;
                        propagation_transfer_limited_ids.push(transient_id);
                        continue;
                    }
                    let next_size = cumulative_size.saturating_add(transfer_size);
                    if sync_limit_bytes.is_some_and(|limit| next_size >= limit) {
                        propagation_skipped = propagation_skipped.saturating_add(1);
                        propagation_remaining_bytes =
                            propagation_remaining_bytes.saturating_add(entry.size_bytes);
                        propagation_skipped_ids.push(entry.transient_id);
                        continue;
                    }
                    cumulative_size = next_size;
                    let wanted = wanted_ids
                        .as_ref()
                        .map_or(true, |ids| ids.wants(entry.transient_id.as_str()));
                    let transient_id = entry.transient_id.clone();
                    propagation_handled = propagation_handled.saturating_add(1);
                    propagation_offered_bytes =
                        propagation_offered_bytes.saturating_add(entry.size_bytes);
                    if wanted {
                        let payload_bytes =
                            hex::decode(entry.payload_hex.as_str()).map_err(|err| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    format!("invalid propagation payload hex: {err}"),
                                )
                            })?;
                        let propagation_message = json!({
                            "transient_id": entry.transient_id,
                            "destination": entry.destination,
                            "payload_hex": entry.payload_hex,
                            "received_at": entry.received_at,
                            "size_bytes": entry.size_bytes,
                            "stamp_value": entry.stamp_value,
                        });
                        self.store
                            .mark_peer_transferred_propagation(peer_id, transient_id.as_str())
                            .map_err(std::io::Error::other)?;
                        propagation_transferred = propagation_transferred.saturating_add(1);
                        propagation_bytes = propagation_bytes.saturating_add(entry.size_bytes);
                        propagation_transferred_ids.push(transient_id.clone());
                        propagation_messages.push(propagation_message);
                        propagation_resource_payloads.push(payload_bytes);
                    } else {
                        self.store
                            .mark_peer_handled_propagation(peer_id, transient_id.as_str())
                            .map_err(std::io::Error::other)?;
                    }
                    propagation_handled_ids.push(transient_id);
                }
                let propagation_resource_bytes =
                    peer_sync_resource_data_size(propagation_resource_payloads.as_slice())?;
                let mut propagation_sync = json!({
                    "synced": true,
                    "postponed": false,
                    "handled": propagation_handled,
                    "transferred": propagation_transferred,
                    "skipped": propagation_skipped,
                    "rejected": propagation_rejected,
                    "offered": propagation_handled,
                    "bytes": propagation_bytes,
                    "offered_bytes": propagation_offered_bytes,
                    "rejected_bytes": propagation_rejected_bytes,
                    "remaining": propagation_skipped,
                    "remaining_bytes": propagation_remaining_bytes,
                    "handled_ids": propagation_handled_ids,
                    "transferred_ids": propagation_transferred_ids,
                    "skipped_ids": propagation_skipped_ids,
                    "rejected_ids": propagation_rejected_ids,
                    "transfer_limited": propagation_transfer_limited,
                    "transfer_limited_bytes": propagation_transfer_limited_bytes,
                    "transfer_limited_ids": propagation_transfer_limited_ids,
                    "messages": propagation_messages,
                    "transfer_limit": transfer_limit_bytes,
                    "sync_limit": sync_limit_bytes,
                    "target_stamp_cost": record.propagation_stamp_cost,
                    "stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                });
                let (
                    acceptance_rate,
                    last_sync_attempt,
                    next_sync_attempt,
                    sync_backoff,
                    sync_transfer_rate,
                    tx_bytes,
                    alive,
                    last_heard,
                    seen_count,
                ) = {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    if let Some(existing) = guard.get_mut(&record.peer) {
                        let propagation_offered = propagation_handled;
                        let propagation_pending = propagation_skipped;
                        let propagation_completed = propagation_handled > 0
                            || propagation_rejected > 0
                            || propagation_transfer_limited > 0
                            || propagation_skipped > 0;
                        let propagation_no_work =
                            !propagation_completed && propagation_pending == 0;
                        let propagation_no_transfer_offer_response =
                            wanted_ids.as_ref().is_some_and(PeerSyncWantedIds::wants_none)
                                && propagation_transferred == 0
                                && propagation_handled > 0;
                        let had_prior_peer_activity = existing.last_sync_attempt > 0
                            || existing.offered > 0
                            || existing.outgoing > 0
                            || existing.incoming > 0
                            || existing.rx_bytes > 0
                            || existing.tx_bytes > 0
                            || existing.sync_transfer_rate > 0.0
                            || existing.acceptance_rate > 0.0;
                        let was_alive = existing.alive;
                        existing.last_sync_attempt = timestamp;
                        if propagation_no_transfer_offer_response {
                            if let Some((last_seen, seen_count)) = prior_peer_seen {
                                existing.last_seen = last_seen;
                                existing.seen_count = seen_count;
                            }
                        }
                        existing.alive = if (propagation_no_work
                            && existing.sync_backoff == 0
                            && had_prior_peer_activity)
                            || propagation_no_transfer_offer_response
                        {
                            was_alive
                        } else {
                            propagation_completed || existing.last_sync_attempt < existing.last_seen
                        };
                        existing.tx_bytes =
                            existing.tx_bytes.saturating_add(propagation_resource_bytes);
                        if propagation_transferred > 0 {
                            existing.sync_transfer_rate = propagation_resource_bytes as f64;
                        }
                        if propagation_offered > 0 {
                            existing.offered =
                                existing.offered.saturating_add(propagation_offered as u64);
                            existing.outgoing =
                                existing.outgoing.saturating_add(propagation_transferred as u64);
                            existing.acceptance_rate = if existing.offered == 0 {
                                0.0
                            } else {
                                (existing.outgoing as f64 / existing.offered as f64).clamp(0.0, 1.0)
                            };
                        }
                        if propagation_completed {
                            existing.sync_backoff = 0;
                            existing.next_sync_attempt = 0;
                        } else if propagation_pending > 0 {
                            existing.sync_backoff = existing
                                .sync_backoff
                                .saturating_add(LXMF_PEER_SYNC_BACKOFF_STEP_SECS);
                            existing.next_sync_attempt =
                                timestamp.saturating_add(i64::from(existing.sync_backoff));
                        }
                        (
                            existing.acceptance_rate,
                            existing.last_sync_attempt,
                            existing.next_sync_attempt,
                            existing.sync_backoff,
                            existing.sync_transfer_rate,
                            existing.tx_bytes,
                            existing.alive,
                            existing.last_seen,
                            existing.seen_count,
                        )
                    } else {
                        (
                            record.acceptance_rate,
                            record.last_sync_attempt,
                            record.next_sync_attempt,
                            record.sync_backoff,
                            record.sync_transfer_rate,
                            record.tx_bytes,
                            record.alive,
                            record.last_seen,
                            record.seen_count,
                        )
                    }
                };
                let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes) =
                    self.peer_message_stats(record.peer.as_str()).unwrap_or((0, 0, 0, 0, 0, 0));
                let acceptance_rate =
                    peer_acceptance_rate_for_reporting(acceptance_rate, outgoing, offered, alive);
                let handled_ids = self
                    .store
                    .list_peer_handled_propagation_ids(record.peer.as_str())
                    .unwrap_or_default();
                let unhandled_ids = self
                    .store
                    .list_peer_unhandled_propagation_ids(record.peer.as_str())
                    .unwrap_or_default();
                let messages = json!({
                    "offered": offered,
                    "outgoing": outgoing,
                    "incoming": incoming,
                    "unhandled": unhandled,
                    "offered_bytes": offered_bytes,
                    "unhandled_bytes": unhandled_bytes,
                    "handled_ids": handled_ids,
                    "unhandled_ids": unhandled_ids,
                });
                let peer_type_value = record.peer_type.clone();
                let peer_status_type =
                    if self.is_static_peer(record.peer.as_str()) { "static" } else { "discovered" };
                let peering_key = peer_peering_key_value(&record, self.identity_hash.as_str());
                let peering_key_status = peer_peering_key_status(&record, peering_key);
                if let Some(propagation) = propagation_sync.as_object_mut() {
                    propagation.insert(
                        "peering_key".to_string(),
                        peering_key.map_or(JsonValue::Null, JsonValue::from),
                    );
                    propagation.insert("peering_key_status".to_string(), json!(peering_key_status));
                }
                let event = RpcEvent {
                    event_type: "peer_sync".into(),
                    payload: json!({
                        "peer": &record.peer,
                        "peer_type": peer_type_value,
                        "type": peer_status_type,
                        "timestamp": timestamp,
                        "name": &record.name,
                        "name_source": &record.name_source,
                        "last_heard": last_heard,
                        "first_seen": record.first_seen,
                        "seen_count": seen_count,
                        "state": 0,
                        "sync_strategy": 2,
                        "ler": 0,
                        "peering_timebase": record.peering_timebase,
                        "network_distance": record.network_distance,
                        "rx_bytes": record.rx_bytes,
                        "tx_bytes": tx_bytes,
                        "alive": alive,
                        "acceptance_rate": acceptance_rate,
                        "last_sync_attempt": last_sync_attempt,
                        "next_sync_attempt": next_sync_attempt,
                        "sync_backoff": sync_backoff,
                        "sync_transfer_rate": sync_transfer_rate,
                        "str": sync_transfer_rate as u64,
                        "synced": true,
                        "propagation_transfer_limit": record.propagation_transfer_limit,
                        "propagation_sync_limit": record.propagation_sync_limit,
                        "propagation_stamp_cost": record.propagation_stamp_cost,
                        "propagation_stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                        "peering_key": peering_key,
                        "peering_key_status": peering_key_status,
                        "transfer_limit": transfer_limit_bytes,
                        "sync_limit": sync_limit_bytes,
                        "target_stamp_cost": record.propagation_stamp_cost,
                        "stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                        "offered": offered,
                        "outgoing": outgoing,
                        "incoming": incoming,
                        "messages": messages,
                        "propagation": propagation_sync.clone(),
                    }),
                };
                self.publish_event(event);

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "peer": record.peer,
                        "peer_type": peer_type_value,
                        "type": peer_status_type,
                        "name": record.name,
                        "name_source": record.name_source,
                        "first_seen": record.first_seen,
                        "seen_count": seen_count,
                        "synced": true,
                        "state": 0,
                        "sync_strategy": 2,
                        "ler": 0,
                        "peering_timebase": record.peering_timebase,
                        "network_distance": record.network_distance,
                        "rx_bytes": record.rx_bytes,
                        "tx_bytes": tx_bytes,
                        "alive": alive,
                        "acceptance_rate": acceptance_rate,
                        "last_heard": last_heard,
                        "last_sync_attempt": last_sync_attempt,
                        "next_sync_attempt": next_sync_attempt,
                        "sync_backoff": sync_backoff,
                        "sync_transfer_rate": sync_transfer_rate,
                        "str": sync_transfer_rate as u64,
                        "propagation_transfer_limit": record.propagation_transfer_limit,
                        "propagation_sync_limit": record.propagation_sync_limit,
                        "propagation_stamp_cost": record.propagation_stamp_cost,
                        "propagation_stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                        "peering_key": peering_key,
                        "peering_key_status": peering_key_status,
                        "transfer_limit": transfer_limit_bytes,
                        "sync_limit": sync_limit_bytes,
                        "target_stamp_cost": record.propagation_stamp_cost,
                        "stamp_cost_flexibility": record.propagation_stamp_cost_flexibility,
                        "offered": offered,
                        "outgoing": outgoing,
                        "incoming": incoming,
                        "messages": messages,
                        "propagation": propagation_sync,
                    })),
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

                let cleanup = self.unpeer_local_state(peer_id)?;
                let offered = cleanup.messages["offered"].as_u64().unwrap_or(0);
                let outgoing = cleanup.messages["outgoing"].as_u64().unwrap_or(0);
                let incoming = cleanup.messages["incoming"].as_u64().unwrap_or(0);
                let event = RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: json!({
                        "peer": peer_id,
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
                        "peer": peer_id,
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
        let stamp_state = lxmf
            .and_then(|lxmf| lxmf.get("stamp_state"))
            .and_then(JsonValue::as_str)
            .map(|state| state.trim().to_ascii_lowercase());
        let propagation_stamp_state = lxmf
            .and_then(|lxmf| lxmf.get("propagation_stamp_state"))
            .and_then(JsonValue::as_str)
            .map(|state| state.trim().to_ascii_lowercase());
        let explicit_progress =
            lxmf.and_then(|lxmf| lxmf.get("progress")).and_then(JsonValue::as_f64);

        match stamp_state.as_deref() {
            Some("failed" | "cancelled") => None,
            Some("generating") => Some(0.0),
            _ => match propagation_stamp_state.as_deref() {
                Some("failed" | "cancelled") => None,
                Some("generating") => Some(0.0),
                _ if explicit_progress.is_some() => {
                    explicit_progress.map(|progress| progress.clamp(0.0, 1.0))
                }
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
        if Self::lxmf_state_is_terminal(lxmf, "stamp_state") {
            return None;
        }
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
        if Self::lxmf_state_is_terminal(lxmf, "propagation_stamp_state") {
            return None;
        }
        Self::json_u32(lxmf.get("propagation_target_cost"))
            .or_else(|| Self::json_u32(lxmf.get("propagation_stamp_target_cost")))
    }

    fn lxmf_state_is_terminal(lxmf: &serde_json::Map<String, JsonValue>, state_key: &str) -> bool {
        lxmf.get(state_key).and_then(JsonValue::as_str).is_some_and(|state| {
            matches!(state.trim().to_ascii_lowercase().as_str(), "failed" | "cancelled")
        })
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

pub(super) struct LocalUnpeerCleanup {
    pub(super) removed: bool,
    pub(super) propagation_cleared: usize,
    pub(super) propagation_cleared_bytes: u64,
    pub(super) messages: JsonValue,
}

impl RpcDaemon {
    pub(super) fn unpeer_local_state(
        &self,
        peer_id: &str,
    ) -> Result<LocalUnpeerCleanup, std::io::Error> {
        let propagation_stats =
            self.store.peer_propagation_message_stats(peer_id).map_err(std::io::Error::other)?;
        let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes) =
            self.peer_message_stats(peer_id)?;
        let handled_ids =
            self.store.list_peer_handled_propagation_ids(peer_id).map_err(std::io::Error::other)?;
        let unhandled_ids = self
            .store
            .list_peer_unhandled_propagation_ids(peer_id)
            .map_err(std::io::Error::other)?;
        self.store.clear_peer_propagation_marks(peer_id).map_err(std::io::Error::other)?;
        let messages = json!({
            "offered": offered,
            "outgoing": outgoing,
            "incoming": incoming,
            "unhandled": unhandled,
            "offered_bytes": offered_bytes,
            "unhandled_bytes": unhandled_bytes,
            "handled_ids": handled_ids,
            "unhandled_ids": unhandled_ids,
        });
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
        let mut cleared_selected_node = false;
        {
            let mut guard =
                self.outbound_propagation_node.lock().expect("propagation node mutex poisoned");
            if guard.as_deref() == Some(peer_id) {
                *guard = None;
                cleared_selected_node = true;
            }
        }
        if cleared_selected_node {
            let state = {
                let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
                guard.selected_node = None;
                guard.clone()
            };
            self.update_daemon_status_snapshot(|snapshot| {
                snapshot.propagation = state;
            });
        }
        Ok(LocalUnpeerCleanup {
            removed,
            propagation_cleared: propagation_stats
                .offered
                .saturating_add(propagation_stats.unhandled)
                as usize,
            propagation_cleared_bytes: propagation_stats
                .offered_bytes
                .saturating_add(propagation_stats.unhandled_bytes),
            messages,
        })
    }
}

pub(super) fn peer_peering_key_value(peer: &PeerRecord, local_identity_hash: &str) -> Option<u32> {
    let peering_cost = peer.peering_cost?;
    let remote_hash = decode_truncated_hash(peer.peer.as_str())?;
    let local_hash = decode_truncated_hash(local_identity_hash)?;
    let mut material = Vec::with_capacity(remote_hash.len() + local_hash.len());
    material.extend_from_slice(remote_hash.as_slice());
    material.extend_from_slice(local_hash.as_slice());
    generate_peering_key_value(material.as_slice(), peering_cost)
}

pub(super) fn peer_peering_key_status(peer: &PeerRecord, peering_key: Option<u32>) -> &'static str {
    match (peer.peering_cost, peering_key) {
        (None, _) => "unconfigured",
        (Some(_), Some(_)) => "ready",
        (Some(_), None) => "not_ready",
    }
}

pub(super) fn peer_acceptance_rate_for_reporting(
    cached_rate: f64,
    outgoing: u64,
    offered: u64,
    alive: bool,
) -> f64 {
    if offered > 0 {
        (outgoing as f64 / offered as f64).clamp(0.0, 1.0)
    } else if !alive {
        0.0
    } else {
        cached_rate.clamp(0.0, 1.0)
    }
}

fn peer_stamp_policy_known(peer: &PeerRecord) -> bool {
    peer.propagation_stamp_cost.is_some()
        && peer.propagation_stamp_cost_flexibility.is_some()
        && peer.peering_cost.is_some()
}

fn peer_stamp_policy_partially_known(peer: &PeerRecord) -> bool {
    peer.propagation_stamp_cost.is_some()
        || peer.propagation_stamp_cost_flexibility.is_some()
        || peer.peering_cost.is_some()
}

fn peer_minimum_accepted_stamp_value(peer: &PeerRecord) -> Option<u32> {
    let _cost = peer.propagation_stamp_cost?;
    let _flexibility = peer.propagation_stamp_cost_flexibility?;
    // Python LXMPeer uses min(0, cost - flexibility), so positive stamp values are never rejected here.
    Some(0)
}

fn peer_sync_policy_relevance(
    pending_propagation: &[PropagationEntryRecord],
    wanted_ids: Option<&PeerSyncWantedIds>,
    sync_limit_bytes: Option<usize>,
) -> (usize, bool) {
    let mut policy_relevant_pending = 0usize;
    let mut policy_relevant_has_stamp = false;
    let mut policy_relevant_size = 24usize;
    let policy_wanted_ids = wanted_ids.filter(|ids| !ids.wants_none());
    for entry in pending_propagation.iter().filter(|entry| {
        policy_wanted_ids.map_or(true, |ids| ids.wants(entry.transient_id.as_str()))
    }) {
        let entry_size = usize::try_from(entry.size_bytes).unwrap_or(usize::MAX);
        let transfer_size = entry_size.saturating_add(16);
        let next_size = policy_relevant_size.saturating_add(transfer_size);
        if sync_limit_bytes.is_some_and(|limit| next_size >= limit) {
            continue;
        }
        policy_relevant_size = next_size;
        policy_relevant_pending = policy_relevant_pending.saturating_add(1);
        policy_relevant_has_stamp |= entry.stamp_value.is_some();
    }
    (policy_relevant_pending, policy_relevant_has_stamp)
}

#[derive(Debug)]
enum PeerSyncWantedIds {
    All,
    Selected(std::collections::HashSet<String>),
}

impl PeerSyncWantedIds {
    fn wants(&self, transient_id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Selected(ids) => ids.contains(transient_id),
        }
    }

    fn wants_none(&self) -> bool {
        matches!(self, Self::Selected(ids) if ids.is_empty())
    }

    fn requires_offer_validation(&self) -> bool {
        matches!(self, Self::Selected(_))
    }

    fn selected_ids(&self) -> Option<&std::collections::HashSet<String>> {
        match self {
            Self::All => None,
            Self::Selected(ids) => Some(ids),
        }
    }
}

fn canonical_peer_sync_wanted_ids(
    wanted_ids: Option<&JsonValue>,
) -> Result<Option<PeerSyncWantedIds>, std::io::Error> {
    let Some(value) = wanted_ids else {
        return Ok(None);
    };
    if value.as_bool() == Some(true) {
        return Ok(Some(PeerSyncWantedIds::All));
    }
    if value.as_bool() == Some(false) {
        return Ok(Some(PeerSyncWantedIds::Selected(std::collections::HashSet::new())));
    }
    let wanted_ids = value.as_array().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "wanted_ids must be true, false, or a list of 32-byte transient ids",
        )
    })?;
    let mut canonical = std::collections::HashSet::with_capacity(wanted_ids.len());
    for wanted_id in wanted_ids {
        let wanted_id = wanted_id.as_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "wanted_ids must contain 32-byte transient ids",
            )
        })?;
        let wanted_id = wanted_id.trim();
        if wanted_id.len() != 64 || !wanted_id.as_bytes().iter().all(u8::is_ascii_hexdigit) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "wanted_ids must contain 32-byte transient ids",
            ));
        }
        canonical.insert(wanted_id.to_ascii_lowercase());
    }
    Ok(Some(PeerSyncWantedIds::Selected(canonical)))
}

fn validate_peer_sync_wanted_ids_in_offer(
    wanted_ids: Option<&PeerSyncWantedIds>,
    pending_propagation: &[PropagationEntryRecord],
    transfer_limit_bytes: Option<usize>,
    sync_limit_bytes: Option<usize>,
) -> Result<(), std::io::Error> {
    let Some(wanted_ids) = wanted_ids.and_then(PeerSyncWantedIds::selected_ids) else {
        return Ok(());
    };
    let mut offerable_ids = std::collections::HashSet::with_capacity(pending_propagation.len());
    let mut cumulative_size = 24usize;
    for entry in pending_propagation {
        let entry_size = usize::try_from(entry.size_bytes).unwrap_or(usize::MAX);
        let transfer_size = entry_size.saturating_add(16);
        if transfer_limit_bytes.is_some_and(|limit| transfer_size > limit) {
            continue;
        }
        let next_size = cumulative_size.saturating_add(transfer_size);
        if sync_limit_bytes.is_some_and(|limit| next_size >= limit) {
            continue;
        }
        cumulative_size = next_size;
        offerable_ids.insert(entry.transient_id.as_str());
    }
    for wanted_id in wanted_ids {
        if !offerable_ids.contains(wanted_id.as_str()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "wanted_ids must reference the current peer offer",
            ));
        }
    }
    Ok(())
}

fn peer_sync_resource_data_size(payloads: &[Vec<u8>]) -> Result<u64, std::io::Error> {
    if payloads.is_empty() {
        return Ok(0);
    }
    let packed = rmp_serde::to_vec(&(1.0_f64, payloads)).map_err(std::io::Error::other)?;
    Ok(packed.len() as u64)
}

fn propagation_peer_sync_weight(
    entry: &PropagationEntryRecord,
    now: i64,
    prioritised_destinations: &[String],
) -> f64 {
    const FOUR_DAYS_SECS: f64 = 4.0 * 24.0 * 60.0 * 60.0;

    let age_secs = now.saturating_sub(entry.received_at) as f64;
    let age_weight = (age_secs / FOUR_DAYS_SECS).max(1.0);
    let priority_weight = if prioritised_destinations
        .iter()
        .any(|destination| entry.destination.eq_ignore_ascii_case(destination.trim()))
    {
        0.1
    } else {
        1.0
    };
    priority_weight * age_weight * entry.size_bytes as f64
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

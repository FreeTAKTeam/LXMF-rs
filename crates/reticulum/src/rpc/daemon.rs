use super::*;

impl RpcDaemon {
    pub fn with_store(store: MessagesStore, identity_hash: String) -> Self {
        let (events, _rx) = broadcast::channel(64);
        Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            event_queue: Mutex::new(VecDeque::new()),
            peers: Mutex::new(HashMap::new()),
            interfaces: Mutex::new(Vec::new()),
            delivery_policy: Mutex::new(DeliveryPolicy::default()),
            propagation_state: Mutex::new(PropagationState::default()),
            propagation_payloads: Mutex::new(HashMap::new()),
            outbound_propagation_node: Mutex::new(None),
            paper_ingest_seen: Mutex::new(HashSet::new()),
            stamp_policy: Mutex::new(StampPolicy::default()),
            ticket_cache: Mutex::new(HashMap::new()),
            delivery_traces: Mutex::new(HashMap::new()),
            outbound_bridge: None,
            announce_bridge: None,
        }
    }

    pub fn with_store_and_bridge(
        store: MessagesStore,
        identity_hash: String,
        outbound_bridge: Arc<dyn OutboundBridge>,
    ) -> Self {
        let (events, _rx) = broadcast::channel(64);
        Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            event_queue: Mutex::new(VecDeque::new()),
            peers: Mutex::new(HashMap::new()),
            interfaces: Mutex::new(Vec::new()),
            delivery_policy: Mutex::new(DeliveryPolicy::default()),
            propagation_state: Mutex::new(PropagationState::default()),
            propagation_payloads: Mutex::new(HashMap::new()),
            outbound_propagation_node: Mutex::new(None),
            paper_ingest_seen: Mutex::new(HashSet::new()),
            stamp_policy: Mutex::new(StampPolicy::default()),
            ticket_cache: Mutex::new(HashMap::new()),
            delivery_traces: Mutex::new(HashMap::new()),
            outbound_bridge: Some(outbound_bridge),
            announce_bridge: None,
        }
    }

    pub fn with_store_and_bridges(
        store: MessagesStore,
        identity_hash: String,
        outbound_bridge: Option<Arc<dyn OutboundBridge>>,
        announce_bridge: Option<Arc<dyn AnnounceBridge>>,
    ) -> Self {
        let (events, _rx) = broadcast::channel(64);
        Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            event_queue: Mutex::new(VecDeque::new()),
            peers: Mutex::new(HashMap::new()),
            interfaces: Mutex::new(Vec::new()),
            delivery_policy: Mutex::new(DeliveryPolicy::default()),
            propagation_state: Mutex::new(PropagationState::default()),
            propagation_payloads: Mutex::new(HashMap::new()),
            outbound_propagation_node: Mutex::new(None),
            paper_ingest_seen: Mutex::new(HashSet::new()),
            stamp_policy: Mutex::new(StampPolicy::default()),
            ticket_cache: Mutex::new(HashMap::new()),
            delivery_traces: Mutex::new(HashMap::new()),
            outbound_bridge,
            announce_bridge,
        }
    }

    pub fn test_instance() -> Self {
        let store = MessagesStore::in_memory().expect("in-memory store");
        Self::with_store(store, "test-identity".into())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn test_instance_with_identity(identity: impl Into<String>) -> Self {
        let store = MessagesStore::in_memory().expect("in-memory store");
        Self::with_store(store, identity.into())
    }

    pub fn set_delivery_destination_hash(&self, hash: Option<String>) {
        let mut guard = self
            .delivery_destination_hash
            .lock()
            .expect("delivery_destination_hash mutex poisoned");
        *guard = hash.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
    }

    pub fn replace_interfaces(&self, interfaces: Vec<InterfaceRecord>) {
        let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
        *guard = interfaces;
    }

    pub fn set_propagation_state(
        &self,
        enabled: bool,
        store_root: Option<String>,
        target_cost: u32,
    ) {
        let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
        guard.enabled = enabled;
        guard.store_root = store_root;
        guard.target_cost = target_cost;
    }

    pub fn update_propagation_sync_state<F>(&self, update: F)
    where
        F: FnOnce(&mut PropagationState),
    {
        let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
        update(&mut guard);
    }

    fn store_inbound_record(&self, record: MessageRecord) -> Result<(), std::io::Error> {
        self.store.insert_message(&record).map_err(std::io::Error::other)?;
        let event =
            RpcEvent { event_type: "inbound".into(), payload: json!({ "message": record }) };
        self.push_event(event.clone());
        let _ = self.events.send(event);
        Ok(())
    }

    pub fn accept_inbound(&self, record: MessageRecord) -> Result<(), std::io::Error> {
        self.store_inbound_record(record)
    }

    pub fn accept_announce(&self, peer: String, timestamp: i64) -> Result<(), std::io::Error> {
        self.accept_announce_with_metadata(
            peer, timestamp, None, None, None, None, None, None, None, None, None, None, None,
            None, None, None, None, None,
        )
    }

    pub fn accept_announce_with_details(
        &self,
        peer: String,
        timestamp: i64,
        name: Option<String>,
        name_source: Option<String>,
    ) -> Result<(), std::io::Error> {
        self.accept_announce_with_metadata(
            peer,
            timestamp,
            name,
            name_source,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn accept_announce_with_metadata(
        &self,
        peer: String,
        timestamp: i64,
        name: Option<String>,
        name_source: Option<String>,
        app_data_hex: Option<String>,
        capabilities: Option<Vec<String>>,
        rssi: Option<f64>,
        snr: Option<f64>,
        q: Option<f64>,
        stamp_cost: Option<u32>,
        stamp_cost_flexibility: Option<Option<u32>>,
        peering_cost: Option<Option<u32>>,
        aspect: Option<String>,
        hops: Option<u32>,
        interface: Option<String>,
        source_private_key: Option<String>,
        source_identity: Option<String>,
        source_node: Option<String>,
    ) -> Result<(), std::io::Error> {
        let _ = stamp_cost;
        let stamp_cost_flexibility = stamp_cost_flexibility.flatten();
        let peering_cost = peering_cost.flatten();
        let record = self.upsert_peer(peer, timestamp, name, name_source);
        let capability_list = if let Some(caps) = capabilities {
            normalize_capabilities(caps)
        } else {
            parse_capabilities_from_app_data_hex(app_data_hex.as_deref())
        };

        let announce_record = AnnounceRecord {
            id: format!("announce-{}-{}-{}", record.last_seen, record.peer, record.seen_count),
            peer: record.peer.clone(),
            timestamp: record.last_seen,
            name: record.name.clone(),
            name_source: record.name_source.clone(),
            first_seen: record.first_seen,
            seen_count: record.seen_count,
            app_data_hex: clean_optional_text(app_data_hex),
            capabilities: capability_list.clone(),
            rssi,
            snr,
            q,
            stamp_cost_flexibility,
            peering_cost,
        };
        self.store.insert_announce(&announce_record).map_err(std::io::Error::other)?;

        let event = RpcEvent {
            event_type: "announce_received".into(),
            payload: json!({
                "id": announce_record.id,
                "peer": record.peer,
                "timestamp": record.last_seen,
                "name": record.name,
                "name_source": record.name_source,
                "first_seen": record.first_seen,
                "seen_count": record.seen_count,
                "app_data_hex": announce_record.app_data_hex,
                "capabilities": capability_list,
                "rssi": rssi,
                "snr": snr,
                "q": q,
                "stamp_cost_flexibility": stamp_cost_flexibility,
                "peering_cost": peering_cost,
                "aspect": aspect,
                "hops": hops,
                "interface": interface,
                "source_private_key": source_private_key,
                "source_identity": source_identity,
                "source_node": source_node,
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
        Ok(())
    }

    fn upsert_peer(
        &self,
        peer: String,
        timestamp: i64,
        name: Option<String>,
        name_source: Option<String>,
    ) -> PeerRecord {
        let cleaned_name = clean_optional_text(name);
        let cleaned_name_source = clean_optional_text(name_source);

        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        if let Some(existing) = guard.get_mut(&peer) {
            existing.last_seen = timestamp;
            existing.seen_count = existing.seen_count.saturating_add(1);
            if let Some(name) = cleaned_name {
                existing.name = Some(name);
                existing.name_source = cleaned_name_source;
            }
            return existing.clone();
        }

        let record = PeerRecord {
            peer: peer.clone(),
            last_seen: timestamp,
            name: cleaned_name,
            name_source: cleaned_name_source,
            first_seen: timestamp,
            seen_count: 1,
        };
        guard.insert(peer, record.clone());
        record
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn accept_inbound_for_test(
        &self,
        record: MessageRecord,
    ) -> Result<(), std::io::Error> {
        self.store_inbound_record(record)
    }

    pub fn handle_rpc(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "status" => Ok(RpcResponse {
                id: request.id,
                result: Some(json!({
                    "identity_hash": self.identity_hash,
                    "delivery_destination_hash": self.local_delivery_hash(),
                    "running": true
                })),
                error: None,
            }),
            "daemon_status_ex" => {
                let peer_count = self.peers.lock().expect("peers mutex poisoned").len();
                let interfaces = self.interfaces.lock().expect("interfaces mutex poisoned").clone();
                let message_count =
                    self.store.list_messages(10_000, None).map_err(std::io::Error::other)?.len();
                let delivery_policy =
                    self.delivery_policy.lock().expect("policy mutex poisoned").clone();
                let propagation =
                    self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                let stamp_policy = self.stamp_policy.lock().expect("stamp mutex poisoned").clone();

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "identity_hash": self.identity_hash,
                        "delivery_destination_hash": self.local_delivery_hash(),
                        "running": true,
                        "peer_count": peer_count,
                        "message_count": message_count,
                        "interface_count": interfaces.len(),
                        "interfaces": interfaces,
                        "delivery_policy": delivery_policy,
                        "propagation": propagation,
                        "stamp_policy": stamp_policy,
                        "capabilities": Self::capabilities(),
                    })),
                    error: None,
                })
            }
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

                {
                    let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
                    *guard = parsed.interfaces.clone();
                }

                let event = RpcEvent {
                    event_type: "interfaces_updated".into(),
                    payload: json!({ "interfaces": parsed.interfaces }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "updated": true })),
                    error: None,
                })
            }
            "reload_config" => {
                let timestamp = now_i64();
                let event = RpcEvent {
                    event_type: "config_reloaded".into(),
                    payload: json!({ "timestamp": timestamp }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "reloaded": true, "timestamp": timestamp })),
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
                        "peer": record.peer.clone(),
                        "timestamp": timestamp,
                        "name": record.name.clone(),
                        "name_source": record.name_source.clone(),
                        "first_seen": record.first_seen,
                        "seen_count": record.seen_count,
                    }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);

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
                self.push_event(event.clone());
                let _ = self.events.send(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "removed": removed })),
                    error: None,
                })
            }
            "send_message" | "send_message_v2" => {
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
                self.store
                    .update_receipt_status(&parsed.message_id, &parsed.status)
                    .map_err(std::io::Error::other)?;
                let message_id = parsed.message_id;
                let status = parsed.status;
                self.append_delivery_trace(&message_id, status.clone());
                let reason_code = delivery_reason_code(&status);
                let event = RpcEvent {
                    event_type: "receipt".into(),
                    payload: json!({
                        "message_id": message_id,
                        "status": status,
                        "reason_code": reason_code,
                    }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "message_id": message_id,
                        "status": status,
                        "reason_code": reason_code,
                    })),
                    error: None,
                })
            }
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
                    guard.clone()
                };
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
                    guard.clone()
                };

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
                self.push_event(event.clone());
                let _ = self.events.send(event);
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
                    guard.clone()
                };

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

                let ttl_secs = parsed.ttl_secs.unwrap_or(3600);
                let ttl = i64::try_from(ttl_secs).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("ttl_secs exceeds supported range: {ttl_secs}"),
                    )
                })?;
                let now = now_i64();
                let expires_at = now.checked_add(ttl).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("ttl_secs causes timestamp overflow: {ttl_secs}"),
                    )
                })?;
                let mut hasher = Sha256::new();
                hasher.update(parsed.destination.as_bytes());
                hasher.update(now.to_be_bytes());
                let ticket = encode_hex(hasher.finalize());
                let record = TicketRecord {
                    destination: parsed.destination.clone(),
                    ticket: ticket.clone(),
                    expires_at,
                };

                self.ticket_cache
                    .lock()
                    .expect("ticket mutex poisoned")
                    .insert(parsed.destination, record.clone());

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "ticket": record.ticket,
                        "destination": record.destination,
                        "expires_at": record.expires_at,
                        "ttl_secs": ttl_secs,
                    })),
                    error: None,
                })
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
                self.push_event(event.clone());
                let _ = self.events.send(event);
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
                let (parsed_stamp_cost_flexibility, parsed_peering_cost) =
                    parse_announce_costs_from_app_data_hex(parsed.app_data_hex.as_deref());
                let stamp_cost_flexibility =
                    parsed.stamp_cost_flexibility.or(parsed_stamp_cost_flexibility);
                let peering_cost = parsed.peering_cost.or(parsed_peering_cost);
                self.accept_announce_with_metadata(
                    parsed.peer,
                    timestamp,
                    parsed.name,
                    parsed.name_source,
                    parsed.app_data_hex,
                    parsed.capabilities,
                    parsed.rssi,
                    parsed.snr,
                    parsed.q,
                    None,
                    Some(stamp_cost_flexibility),
                    Some(peering_cost),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )?;
                let record =
                    self.peers.lock().expect("peers mutex poisoned").get(peer.as_str()).cloned();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "peer": record })),
                    error: None,
                })
            }
            "clear_messages" => {
                self.store.clear_messages().map_err(std::io::Error::other)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "messages" })),
                    error: None,
                })
            }
            "clear_resources" => Ok(RpcResponse {
                id: request.id,
                result: Some(json!({ "cleared": "resources" })),
                error: None,
            }),
            "clear_peers" => {
                {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    guard.clear();
                }
                self.store.clear_announces().map_err(std::io::Error::other)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "peers" })),
                    error: None,
                })
            }
            "clear_all" => {
                self.store.clear_messages().map_err(std::io::Error::other)?;
                self.store.clear_announces().map_err(std::io::Error::other)?;
                {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    guard.clear();
                }
                {
                    let mut guard =
                        self.delivery_traces.lock().expect("delivery traces mutex poisoned");
                    guard.clear();
                }
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "all" })),
                    error: None,
                })
            }
            _ => Ok(RpcResponse {
                id: request.id,
                result: None,
                error: Some(RpcError {
                    code: "NOT_IMPLEMENTED".into(),
                    message: "method not implemented".into(),
                }),
            }),
        }
    }

    fn append_delivery_trace(&self, message_id: &str, status: String) {
        const MAX_DELIVERY_TRACE_ENTRIES: usize = 32;
        const MAX_TRACKED_MESSAGE_TRACES: usize = 2048;

        let timestamp = now_i64();
        let reason_code = delivery_reason_code(&status).map(ToOwned::to_owned);
        let mut guard = self.delivery_traces.lock().expect("delivery traces mutex poisoned");
        let entry = guard.entry(message_id.to_string()).or_default();
        entry.push(DeliveryTraceEntry { status, timestamp, reason_code });
        if entry.len() > MAX_DELIVERY_TRACE_ENTRIES {
            let drain_count = entry.len().saturating_sub(MAX_DELIVERY_TRACE_ENTRIES);
            entry.drain(0..drain_count);
        }

        if guard.len() > MAX_TRACKED_MESSAGE_TRACES {
            let overflow = guard.len() - MAX_TRACKED_MESSAGE_TRACES;
            let mut evicted_ids = Vec::with_capacity(overflow);
            for key in guard.keys() {
                if key != message_id {
                    evicted_ids.push(key.clone());
                    if evicted_ids.len() == overflow {
                        break;
                    }
                }
            }
            for id in evicted_ids {
                guard.remove(&id);
            }

            if guard.len() > MAX_TRACKED_MESSAGE_TRACES {
                let still_over = guard.len() - MAX_TRACKED_MESSAGE_TRACES;
                let mut fallback = Vec::with_capacity(still_over);
                for key in guard.keys().take(still_over).cloned() {
                    fallback.push(key);
                }
                for id in fallback {
                    guard.remove(&id);
                }
            }
        }
    }

    fn response_meta(&self) -> JsonValue {
        json!({
            "contract_version": "v2",
            "profile": JsonValue::Null,
            "rpc_endpoint": JsonValue::Null,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn store_outbound(
        &self,
        request_id: u64,
        id: String,
        source: String,
        destination: String,
        title: String,
        content: String,
        fields: Option<JsonValue>,
        method: Option<String>,
        stamp_cost: Option<u32>,
        options: OutboundDeliveryOptions,
        include_ticket: Option<bool>,
    ) -> Result<RpcResponse, std::io::Error> {
        let timestamp = now_i64();
        self.append_delivery_trace(&id, "queued".to_string());
        let mut record = MessageRecord {
            id: id.clone(),
            source,
            destination,
            title,
            content,
            timestamp,
            direction: "out".into(),
            fields: merge_fields_with_options(fields, method.clone(), stamp_cost, include_ticket),
            receipt_status: None,
        };

        self.store.insert_message(&record).map_err(std::io::Error::other)?;
        self.append_delivery_trace(&id, "sending".to_string());
        let deliver_result = if let Some(bridge) = &self.outbound_bridge {
            bridge.deliver(&record, &options)
        } else {
            let _delivered = crate::transport::test_bridge::deliver_outbound(&record);
            Ok(())
        };
        if let Err(err) = deliver_result {
            let status = format!("failed: {err}");
            let _ = self.store.update_receipt_status(&id, &status);
            record.receipt_status = Some(status);
            let resolved_status = record.receipt_status.clone().unwrap_or_default();
            self.append_delivery_trace(&id, resolved_status.clone());
            let reason_code = delivery_reason_code(&resolved_status);
            let event = RpcEvent {
                event_type: "outbound".into(),
                payload: json!({
                    "message": record,
                    "method": method,
                    "error": err.to_string(),
                    "reason_code": reason_code,
                }),
            };
            self.push_event(event.clone());
            let _ = self.events.send(event);
            return Ok(RpcResponse {
                id: request_id,
                result: None,
                error: Some(RpcError { code: "DELIVERY_FAILED".into(), message: err.to_string() }),
            });
        }
        let sent_status = format!("sent: {}", method.as_deref().unwrap_or("direct"));
        self.append_delivery_trace(&id, sent_status.clone());
        let event = RpcEvent {
            event_type: "outbound".into(),
            payload: json!({
                "message": record,
                "method": method,
                "reason_code": delivery_reason_code(&sent_status),
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);

        Ok(RpcResponse { id: request_id, result: Some(json!({ "message_id": id })), error: None })
    }

    fn local_delivery_hash(&self) -> String {
        self.delivery_destination_hash
            .lock()
            .expect("delivery_destination_hash mutex poisoned")
            .clone()
            .unwrap_or_else(|| self.identity_hash.clone())
    }

    fn capabilities() -> Vec<&'static str> {
        vec![
            "status",
            "daemon_status_ex",
            "list_messages",
            "list_announces",
            "list_peers",
            "send_message",
            "send_message_v2",
            "announce_now",
            "list_interfaces",
            "set_interfaces",
            "reload_config",
            "peer_sync",
            "peer_unpeer",
            "set_delivery_policy",
            "get_delivery_policy",
            "propagation_status",
            "propagation_enable",
            "propagation_ingest",
            "propagation_fetch",
            "get_outbound_propagation_node",
            "set_outbound_propagation_node",
            "list_propagation_nodes",
            "paper_ingest_uri",
            "stamp_policy_get",
            "stamp_policy_set",
            "ticket_generate",
            "message_delivery_trace",
        ]
    }

    pub fn handle_framed_request(&self, bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
        let request: RpcRequest = codec::decode_frame(bytes)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let response = self.handle_rpc(request)?;
        codec::encode_frame(&response).map_err(std::io::Error::other)
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<RpcEvent> {
        self.events.subscribe()
    }

    pub fn take_event(&self) -> Option<RpcEvent> {
        let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
        guard.pop_front()
    }

    pub fn push_event(&self, event: RpcEvent) {
        let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
        if guard.len() >= 32 {
            guard.pop_front();
        }
        guard.push_back(event);
    }

    pub fn emit_event(&self, event: RpcEvent) {
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }

    pub fn schedule_announce_for_test(&self, id: u64) {
        let timestamp = now_i64();
        let event = RpcEvent {
            event_type: "announce_sent".into(),
            payload: json!({ "timestamp": timestamp, "announce_id": id }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }

    pub fn start_announce_scheduler(
        self: std::rc::Rc<Self>,
        interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn_local(async move {
            if interval_secs == 0 {
                return;
            }

            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                // First tick is immediate, so we announce once at scheduler start.
                interval.tick().await;
                let id = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_secs())
                    .unwrap_or(0);

                if let Some(bridge) = &self.announce_bridge {
                    let _ = bridge.announce_now();
                }

                let timestamp = now_i64();
                let event = RpcEvent {
                    event_type: "announce_sent".into(),
                    payload: json!({ "timestamp": timestamp, "announce_id": id }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
            }
        })
    }

    pub fn inject_inbound_test_message(&self, content: &str) {
        let timestamp = now_i64();
        let record = crate::storage::messages::MessageRecord {
            id: format!("test-{}", timestamp),
            source: "test-peer".into(),
            destination: "local".into(),
            title: "".into(),
            content: content.into(),
            timestamp,
            direction: "in".into(),
            fields: None,
            receipt_status: None,
        };
        let _ = self.store.insert_message(&record);
        let event =
            RpcEvent { event_type: "inbound".into(), payload: json!({ "message": record }) };
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }

    pub fn emit_link_event_for_test(&self) {
        let event = RpcEvent {
            event_type: "link_activated".into(),
            payload: json!({ "link_id": "test-link" }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }
}

fn parse_announce_cursor(cursor: Option<&str>) -> Option<(Option<i64>, Option<String>)> {
    let raw = cursor?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some((timestamp_raw, id)) = raw.split_once(':') {
        let timestamp = timestamp_raw.parse::<i64>().ok()?;
        let before_id = if id.is_empty() { None } else { Some(id.to_string()) };
        return Some((Some(timestamp), before_id));
    }
    raw.parse::<i64>().ok().map(|timestamp| (Some(timestamp), None))
}

fn delivery_reason_code(status: &str) -> Option<&'static str> {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.contains("receipt timeout") {
        return Some("receipt_timeout");
    }
    if normalized.contains("timeout") {
        return Some("timeout");
    }
    if normalized.contains("no route")
        || normalized.contains("no path")
        || normalized.contains("no known path")
    {
        return Some("no_path");
    }
    if normalized.contains("no propagation relay selected") {
        return Some("relay_unset");
    }
    if normalized.contains("retry budget exhausted") {
        return Some("retry_budget_exhausted");
    }
    None
}

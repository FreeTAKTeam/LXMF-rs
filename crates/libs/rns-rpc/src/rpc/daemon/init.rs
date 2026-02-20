impl RpcDaemon {
    pub fn with_store(store: MessagesStore, identity_hash: String) -> Self {
        let (events, _rx) = broadcast::channel(64);
        let active_identity = identity_hash.clone();
        let mut sdk_identities = HashMap::new();
        sdk_identities
            .insert(identity_hash.clone(), Self::default_sdk_identity(identity_hash.as_str()));
        Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            event_queue: Mutex::new(VecDeque::new()),
            sdk_event_log: Mutex::new(VecDeque::new()),
            sdk_next_event_seq: Mutex::new(0),
            sdk_dropped_event_count: Mutex::new(0),
            sdk_active_contract_version: Mutex::new(2),
            sdk_profile: Mutex::new("desktop-full".to_string()),
            sdk_config_revision: Mutex::new(0),
            sdk_runtime_config: Mutex::new(JsonValue::Object(JsonMap::new())),
            sdk_config_apply_lock: Mutex::new(()),
            sdk_effective_capabilities: Mutex::new(Self::sdk_supported_capabilities()),
            sdk_stream_degraded: Mutex::new(false),
            sdk_seen_jti: Mutex::new(HashMap::new()),
            sdk_rate_window_started_ms: Mutex::new(0),
            sdk_rate_ip_counts: Mutex::new(HashMap::new()),
            sdk_rate_principal_counts: Mutex::new(HashMap::new()),
            sdk_next_domain_seq: Mutex::new(0),
            sdk_topics: Mutex::new(HashMap::new()),
            sdk_topic_order: Mutex::new(Vec::new()),
            sdk_topic_subscriptions: Mutex::new(HashSet::new()),
            sdk_telemetry_points: Mutex::new(Vec::new()),
            sdk_attachments: Mutex::new(HashMap::new()),
            sdk_attachment_payloads: Mutex::new(HashMap::new()),
            sdk_attachment_order: Mutex::new(Vec::new()),
            sdk_markers: Mutex::new(HashMap::new()),
            sdk_marker_order: Mutex::new(Vec::new()),
            sdk_identities: Mutex::new(sdk_identities),
            sdk_active_identity: Mutex::new(Some(active_identity)),
            sdk_remote_commands: Mutex::new(HashSet::new()),
            sdk_voice_sessions: Mutex::new(HashMap::new()),
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
            delivery_status_lock: Mutex::new(()),
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
        let active_identity = identity_hash.clone();
        let mut sdk_identities = HashMap::new();
        sdk_identities
            .insert(identity_hash.clone(), Self::default_sdk_identity(identity_hash.as_str()));
        Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            event_queue: Mutex::new(VecDeque::new()),
            sdk_event_log: Mutex::new(VecDeque::new()),
            sdk_next_event_seq: Mutex::new(0),
            sdk_dropped_event_count: Mutex::new(0),
            sdk_active_contract_version: Mutex::new(2),
            sdk_profile: Mutex::new("desktop-full".to_string()),
            sdk_config_revision: Mutex::new(0),
            sdk_runtime_config: Mutex::new(JsonValue::Object(JsonMap::new())),
            sdk_config_apply_lock: Mutex::new(()),
            sdk_effective_capabilities: Mutex::new(Self::sdk_supported_capabilities()),
            sdk_stream_degraded: Mutex::new(false),
            sdk_seen_jti: Mutex::new(HashMap::new()),
            sdk_rate_window_started_ms: Mutex::new(0),
            sdk_rate_ip_counts: Mutex::new(HashMap::new()),
            sdk_rate_principal_counts: Mutex::new(HashMap::new()),
            sdk_next_domain_seq: Mutex::new(0),
            sdk_topics: Mutex::new(HashMap::new()),
            sdk_topic_order: Mutex::new(Vec::new()),
            sdk_topic_subscriptions: Mutex::new(HashSet::new()),
            sdk_telemetry_points: Mutex::new(Vec::new()),
            sdk_attachments: Mutex::new(HashMap::new()),
            sdk_attachment_payloads: Mutex::new(HashMap::new()),
            sdk_attachment_order: Mutex::new(Vec::new()),
            sdk_markers: Mutex::new(HashMap::new()),
            sdk_marker_order: Mutex::new(Vec::new()),
            sdk_identities: Mutex::new(sdk_identities),
            sdk_active_identity: Mutex::new(Some(active_identity)),
            sdk_remote_commands: Mutex::new(HashSet::new()),
            sdk_voice_sessions: Mutex::new(HashMap::new()),
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
            delivery_status_lock: Mutex::new(()),
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
        let active_identity = identity_hash.clone();
        let mut sdk_identities = HashMap::new();
        sdk_identities
            .insert(identity_hash.clone(), Self::default_sdk_identity(identity_hash.as_str()));
        Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            event_queue: Mutex::new(VecDeque::new()),
            sdk_event_log: Mutex::new(VecDeque::new()),
            sdk_next_event_seq: Mutex::new(0),
            sdk_dropped_event_count: Mutex::new(0),
            sdk_active_contract_version: Mutex::new(2),
            sdk_profile: Mutex::new("desktop-full".to_string()),
            sdk_config_revision: Mutex::new(0),
            sdk_runtime_config: Mutex::new(JsonValue::Object(JsonMap::new())),
            sdk_config_apply_lock: Mutex::new(()),
            sdk_effective_capabilities: Mutex::new(Self::sdk_supported_capabilities()),
            sdk_stream_degraded: Mutex::new(false),
            sdk_seen_jti: Mutex::new(HashMap::new()),
            sdk_rate_window_started_ms: Mutex::new(0),
            sdk_rate_ip_counts: Mutex::new(HashMap::new()),
            sdk_rate_principal_counts: Mutex::new(HashMap::new()),
            sdk_next_domain_seq: Mutex::new(0),
            sdk_topics: Mutex::new(HashMap::new()),
            sdk_topic_order: Mutex::new(Vec::new()),
            sdk_topic_subscriptions: Mutex::new(HashSet::new()),
            sdk_telemetry_points: Mutex::new(Vec::new()),
            sdk_attachments: Mutex::new(HashMap::new()),
            sdk_attachment_payloads: Mutex::new(HashMap::new()),
            sdk_attachment_order: Mutex::new(Vec::new()),
            sdk_markers: Mutex::new(HashMap::new()),
            sdk_marker_order: Mutex::new(Vec::new()),
            sdk_identities: Mutex::new(sdk_identities),
            sdk_active_identity: Mutex::new(Some(active_identity)),
            sdk_remote_commands: Mutex::new(HashSet::new()),
            sdk_voice_sessions: Mutex::new(HashMap::new()),
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
            delivery_status_lock: Mutex::new(()),
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
        self.publish_event(event);
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
        self.publish_event(event);
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

    #[allow(dead_code)]
    pub(crate) fn accept_inbound_for_test(
        &self,
        record: MessageRecord,
    ) -> Result<(), std::io::Error> {
        self.store_inbound_record(record)
    }

}

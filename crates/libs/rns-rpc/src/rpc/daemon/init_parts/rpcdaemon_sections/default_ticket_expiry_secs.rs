impl RpcDaemon {

    pub(super) const DEFAULT_TICKET_EXPIRY_SECS: u64 = 21 * 24 * 60 * 60;

    pub(super) const TICKET_GRACE_SECS: i64 = 5 * 24 * 60 * 60;

    pub(super) const TICKET_RENEW_SECS: i64 = 14 * 24 * 60 * 60;

    pub(super) const TICKET_INTERVAL_SECS: i64 = 24 * 60 * 60;

    pub(super) fn active_peer_count_from_guard(
        guard: &std::collections::HashMap<String, crate::rpc::PeerRecord>,
    ) -> usize {
        guard.values().filter(|record| record.peer_type.as_deref() != Some("unpeered")).count()
    }

    pub(super) fn active_peer_ids(&self) -> Vec<String> {
        self.peers
            .lock()
            .expect("peers mutex poisoned")
            .values()
            .filter(|record| record.peer_type.as_deref() != Some("unpeered"))
            .map(|record| record.peer.clone())
            .collect()
    }

    /// Return the peers that should receive newly ingested propagation entries.
    ///
    /// The peer table can contain thousands of discovered peers while a
    /// propagation payload is only useful to a bounded set of current peers.
    /// Keep configured static peers ahead of discovered peers, then prefer the
    /// most recently observed peers so one payload cannot create an unbounded
    /// peer-entry fan-out.
    pub(super) fn propagation_fanout_peer_ids(&self) -> Vec<String> {
        let (max_peers, static_peers) = {
            let state = self.propagation_state.lock().expect("propagation mutex poisoned");
            (state.max_propagation_peers.max(1) as usize, state.static_peers.clone())
        };
        let mut peers = self
            .peers
            .lock()
            .expect("peers mutex poisoned")
            .values()
            .filter(|record| record.peer_type.as_deref() != Some("unpeered"))
            .map(|record| {
                let is_static = static_peers
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(record.peer.as_str()));
                (record.peer.clone(), record.last_seen, is_static)
            })
            .collect::<Vec<_>>();
        peers.sort_by(|left, right| {
            right
                .2
                .cmp(&left.2)
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| left.0.cmp(&right.0))
        });
        peers.truncate(max_peers);
        peers.into_iter().map(|(peer, _, _)| peer).collect()
    }

    pub fn peer_record_exists(&self, peer: &str, include_unpeered: bool) -> bool {
        self.peers.lock().expect("peers mutex poisoned").values().any(|record| {
            record.peer.eq_ignore_ascii_case(peer)
                && (include_unpeered || record.peer_type.as_deref() != Some("unpeered"))
        })
    }

    pub(super) fn queue_existing_propagation_for_peer(
        &self,
        peer: &str,
    ) -> Result<(), std::io::Error> {
        self.ensure_peer_queue_import(peer)?;
        let per_peer_limit = self
            .propagation_state
            .lock()
            .expect("propagation mutex poisoned")
            .peer_entry_limit_per_peer;
        self.store
            .mark_recent_propagation_unhandled_for_peer(peer, per_peer_limit)
            .map_err(std::io::Error::other)?;
        Ok(())
    }

    pub(super) fn normalize_static_peers(static_peers: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for peer in static_peers {
            let peer = peer.trim();
            if !peer.is_empty()
                && !normalized.iter().any(|existing: &String| existing.eq_ignore_ascii_case(peer))
            {
                normalized.push(peer.to_string());
            }
        }
        normalized
    }

    pub(super) fn next_announce_seq(&self) -> u64 {
        let mut guard = self.announce_next_seq.lock().expect("announce_next_seq mutex poisoned");
        *guard = guard.wrapping_add(1);
        *guard
    }

    pub fn with_store(store: MessagesStore, identity_hash: String) -> Self {
        Self::with_store_and_bridges_and_sinks(store, identity_hash, None, None, Vec::new())
    }

    pub fn with_store_and_bridge(
        store: MessagesStore,
        identity_hash: String,
        outbound_bridge: Arc<dyn OutboundBridge>,
    ) -> Self {
        Self::with_store_and_bridges_and_sinks(
            store,
            identity_hash,
            Some(outbound_bridge),
            None,
            Vec::new(),
        )
    }

    pub fn with_store_and_bridges(
        store: MessagesStore,
        identity_hash: String,
        outbound_bridge: Option<Arc<dyn OutboundBridge>>,
        announce_bridge: Option<Arc<dyn AnnounceBridge>>,
    ) -> Self {
        Self::with_store_and_bridges_and_sinks(
            store,
            identity_hash,
            outbound_bridge,
            announce_bridge,
            Vec::new(),
        )
    }

    pub fn with_store_and_bridges_and_sinks(
        store: MessagesStore,
        identity_hash: String,
        outbound_bridge: Option<Arc<dyn OutboundBridge>>,
        announce_bridge: Option<Arc<dyn AnnounceBridge>>,
        event_sink_bridges: Vec<Arc<dyn EventSinkBridge>>,
    ) -> Self {
        let (events, _rx) = broadcast::channel(64);
        let (sdk_events, _sdk_rx) = broadcast::channel(64);
        let active_identity = identity_hash.clone();
        let store = Arc::new(store);
        let sdk_metrics = Arc::new(Mutex::new(RpcMetrics::default()));
        let delivery_traces = Arc::new(Mutex::new(HashMap::new()));
        let delivery_status_lock = Arc::new(Mutex::new(()));
        let outbound_delivery_handoffs = Arc::new(Mutex::new(HashSet::new()));
        let (outbound_delivery_tx,outbound_delivery_workers,outbound_delivery_stop) = Self::spawn_outbound_delivery_worker(
            outbound_bridge.clone(),
            Arc::clone(&store),
            Arc::clone(&delivery_traces),
            Arc::clone(&delivery_status_lock),
            Arc::clone(&outbound_delivery_handoffs),
        );
        let event_sink_tx = if !event_sink_bridges.is_empty() {
            Self::spawn_event_sink_worker(Arc::clone(&sdk_metrics))
                .inspect_err(|err| log::error!("[daemon] failed to spawn event sink worker: {err}"))
                .ok()
        } else {
            None
        };
        let mut sdk_identities = HashMap::new();
        sdk_identities
            .insert(identity_hash.clone(), Self::default_sdk_identity(identity_hash.as_str()));
        let mut sdk_identity_sessions = HashMap::new();
        sdk_identity_sessions.insert(
            super::LEGACY_RPC_SESSION_ID.to_owned(),
            SdkIdentitySession {
                broker_negotiated: false,
                authorized_identities: HashSet::from([identity_hash.clone()]),
                active_identity: Some(identity_hash.clone()),
            },
        );
        let daemon = Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            propagation_destination_hash: Mutex::new(None),
            events,
            sdk_events,
            event_queue: Mutex::new(VecDeque::new()),
            sdk_event_log: Mutex::new(VecDeque::new()),
            zmq_pipeline_metrics: Arc::default(),
            sdk_next_event_seq: Mutex::new(0),
            announce_next_seq: Mutex::new(0),
            sdk_dropped_event_count: Mutex::new(0),
            sdk_active_contract_version: Mutex::new(2),
            sdk_profile: Mutex::new("desktop-full".to_string()),
            sdk_config_revision: Mutex::new(0),
            sdk_runtime_config: Mutex::new(JsonValue::Object(JsonMap::new())),
            sdk_config_apply_lock: Mutex::new(()),
            sdk_effective_capabilities: Mutex::new(Self::sdk_supported_capabilities()),
            sdk_custom_operations: Mutex::new(Vec::new()),
            sdk_stream_degraded: Mutex::new(false),
            sdk_seen_jti: Mutex::new(HashMap::new()),
            sdk_rate_window_started_ms: Mutex::new(0),
            sdk_rate_ip_counts: Mutex::new(HashMap::new()),
            sdk_rate_principal_counts: Mutex::new(HashMap::new()),
            sdk_domain_state_lock: Mutex::new(()),
            sdk_next_domain_seq: Mutex::new(0),
            sdk_topics: Mutex::new(HashMap::new()),
            sdk_topic_order: Mutex::new(Vec::new()),
            sdk_topic_subscriptions: Mutex::new(HashSet::new()),
            sdk_telemetry_points: Mutex::new(Vec::new()),
            sdk_attachments: Mutex::new(HashMap::new()),
            sdk_attachment_payloads: Mutex::new(HashMap::new()),
            sdk_attachment_order: Mutex::new(Vec::new()),
            sdk_attachment_uploads: Mutex::new(HashMap::new()),
            sdk_cursor_hints: Mutex::new(HashMap::new()),
            sdk_markers: Mutex::new(HashMap::new()),
            sdk_marker_order: Mutex::new(Vec::new()),
            sdk_identities: Mutex::new(sdk_identities),
            sdk_identity_sessions: Mutex::new(sdk_identity_sessions),
            sdk_contacts: Mutex::new(HashMap::new()),
            sdk_contact_order: Mutex::new(Vec::new()),
            sdk_active_identity: Mutex::new(Some(active_identity)),
            sdk_remote_commands: Mutex::new(HashMap::new()),
            sdk_voice_sessions: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            peer_queue_imports: Mutex::new(HashSet::new()),
            interfaces: Mutex::new(Vec::new()),
            delivery_policy: Mutex::new(DeliveryPolicy::default()),
            blackholed_identities: Mutex::new(HashMap::new()),
            propagation_state: Mutex::new(PropagationState::default()),
            remote_unpeer_failure_state: Mutex::new(None),
            throttled_propagation_peers: Mutex::new(HashMap::new()),
            outbound_propagation_node: Mutex::new(None),
            paper_ingest_seen: Mutex::new(HashSet::new()),
            stamp_policy: Mutex::new(StampPolicy::default()),
            ticket_cache: Mutex::new(HashMap::new()),
            ticket_last_deliveries: Mutex::new(HashMap::new()),
            router_information_storage_limit_bytes: Mutex::new(None),
            router_retain_node_lxms: Mutex::new(false),
            delivery_traces,
            daemon_status_snapshot: std::sync::RwLock::new(DaemonStatusSnapshot::default()),
            delivery_status_lock,
            outbound_delivery_handoffs,
            sdk_metrics,
            outbound_bridge,
            outbound_delivery_tx,
            outbound_delivery_workers:Mutex::new(outbound_delivery_workers),
            outbound_delivery_stop,
            announce_bridge,
            service_identity_bridge: Mutex::new(None),
            event_sink_bridges,
            event_sink_tx,
            interface_mutation_bridge: Mutex::new(None),
            path_lookup_bridge: Mutex::new(None),
            remote_control_bridge: Mutex::new(None),
            rnode_management_bridge: Mutex::new(None),
            weave_display_control_bridge: Mutex::new(None),
            started_at: std::time::Instant::now(),
        };
        if let Err(error) = daemon.restore_sdk_domain_snapshot() {
            log::error!("failed to restore persisted SDK domain snapshot: {error}");
        }
        daemon
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
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

    pub fn set_service_identity_bridge(&self, bridge: Arc<dyn ServiceIdentityBridge>) {
        let mut guard = self
            .service_identity_bridge
            .lock()
            .expect("service_identity_bridge mutex poisoned");
        *guard = Some(bridge);
    }

}

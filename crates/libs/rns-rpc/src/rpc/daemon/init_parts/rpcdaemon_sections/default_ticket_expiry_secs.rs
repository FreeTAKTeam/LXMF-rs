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
        self.store
            .merge_case_insensitive_peer_propagation_marks(peer)
            .map_err(std::io::Error::other)?;
        self.store.mark_all_propagation_unhandled_for_peer(peer).map_err(std::io::Error::other)?;
        let unhandled_ids =
            self.store.list_peer_unhandled_propagation_ids(peer).map_err(std::io::Error::other)?;
        self.record_peer_queue_unhandled(peer, unhandled_ids.as_slice());
        let handled_ids =
            self.store.list_peer_handled_propagation_ids(peer).map_err(std::io::Error::other)?;
        for transient_id in handled_ids {
            self.record_peer_queue_handled_id(peer, transient_id.as_str());
        }
        Ok(())
    }

    pub(super) fn record_peer_queue_unhandled(&self, peer: &str, transient_ids: &[String]) {
        for transient_id in transient_ids {
            self.record_peer_queue_unhandled_id(peer, transient_id);
        }
    }

    pub(super) fn record_peer_queue_unhandled_id(&self, peer: &str, transient_id: &str) {
        let transient_id = transient_id.trim().to_ascii_lowercase();
        if transient_id.is_empty() {
            return;
        }
        let existing_peer_key = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard.keys().find(|existing| existing.eq_ignore_ascii_case(peer)).cloned()
        };
        let Some(existing_peer_key) = existing_peer_key else {
            return;
        };
        if self
            .store
            .peer_completed_propagation_mark_exists(
                existing_peer_key.as_str(),
                transient_id.as_str(),
            )
            .unwrap_or(false)
        {
            self.record_peer_queue_handled_id(existing_peer_key.as_str(), transient_id.as_str());
            return;
        }
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        let Some(record) = guard.get_mut(&existing_peer_key) else {
            return;
        };
        if record
            .restored_handled_ids
            .iter()
            .any(|id| id.eq_ignore_ascii_case(transient_id.as_str()))
            || record
                .restored_unhandled_ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case(transient_id.as_str()))
        {
            return;
        }
        record.restored_unhandled_ids.push(transient_id);
    }

    pub(super) fn record_peer_queue_handled_id(&self, peer: &str, transient_id: &str) {
        let transient_id = transient_id.trim().to_ascii_lowercase();
        if transient_id.is_empty() {
            return;
        }
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        let existing_peer_key =
            guard.keys().find(|existing| existing.eq_ignore_ascii_case(peer)).cloned();
        let Some(existing_peer_key) = existing_peer_key else {
            return;
        };
        let Some(record) = guard.get_mut(&existing_peer_key) else {
            return;
        };
        record.restored_unhandled_ids.retain(|id| !id.eq_ignore_ascii_case(transient_id.as_str()));
        if !record
            .restored_handled_ids
            .iter()
            .any(|id| id.eq_ignore_ascii_case(transient_id.as_str()))
        {
            record.restored_handled_ids.push(transient_id);
        }
    }

    pub(super) fn record_payload_backed_peer_queue_snapshot(
        &self,
        peer: &str,
    ) -> Result<(), std::io::Error> {
        fn push_unique(ids: &mut Vec<String>, transient_id: String) {
            if !ids.iter().any(|id| id.eq_ignore_ascii_case(transient_id.as_str())) {
                ids.push(transient_id);
            }
        }

        let peer_key = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard.keys().find(|existing| existing.eq_ignore_ascii_case(peer)).cloned()
        };
        let Some(peer_key) = peer_key else {
            return Ok(());
        };
        let record = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard.get(&peer_key).cloned()
        };
        if let Some(record) = record {
            self.restore_peer_record_queue_marks(&record)?;
        }

        let mut unhandled_ids = Vec::new();
        let mut handled_ids = Vec::new();
        for entry in self
            .store
            .list_peer_unhandled_propagation(peer_key.as_str())
            .map_err(std::io::Error::other)?
        {
            let transient_id = entry.transient_id.trim().to_ascii_lowercase();
            if self
                .store
                .peer_completed_propagation_mark_exists(peer_key.as_str(), transient_id.as_str())
                .map_err(std::io::Error::other)?
            {
                push_unique(&mut handled_ids, transient_id);
            } else {
                push_unique(&mut unhandled_ids, transient_id);
            }
        }
        for transient_id in self
            .store
            .list_peer_handled_propagation_ids(peer_key.as_str())
            .map_err(std::io::Error::other)?
        {
            let transient_id = transient_id.trim().to_ascii_lowercase();
            if self
                .store
                .get_propagation_entry(transient_id.as_str())
                .map_err(std::io::Error::other)?
                .is_some()
            {
                push_unique(&mut handled_ids, transient_id);
            }
        }
        unhandled_ids.retain(|transient_id| {
            !handled_ids.iter().any(|handled_id| handled_id.eq_ignore_ascii_case(transient_id))
        });

        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        if let Some(record) = guard.get_mut(&peer_key) {
            record.restored_handled_ids = handled_ids;
            record.restored_unhandled_ids = unhandled_ids;
        }
        Ok(())
    }

    pub(super) fn remove_peer_queue_snapshot_id(&self, transient_id: &str) {
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        for record in guard.values_mut() {
            record.restored_handled_ids.retain(|id| !id.eq_ignore_ascii_case(transient_id));
            record.restored_unhandled_ids.retain(|id| !id.eq_ignore_ascii_case(transient_id));
        }
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
        let outbound_delivery_tx = Self::spawn_outbound_delivery_worker(
            outbound_bridge.clone(),
            Arc::clone(&store),
            Arc::clone(&delivery_traces),
            Arc::clone(&delivery_status_lock),
        );
        let event_sink_tx =
            Self::spawn_event_sink_worker(!event_sink_bridges.is_empty(), Arc::clone(&sdk_metrics));
        let mut sdk_identities = HashMap::new();
        sdk_identities
            .insert(identity_hash.clone(), Self::default_sdk_identity(identity_hash.as_str()));
        let daemon = Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            sdk_events,
            event_queue: Mutex::new(VecDeque::new()),
            sdk_event_log: Mutex::new(VecDeque::new()),
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
            sdk_contacts: Mutex::new(HashMap::new()),
            sdk_contact_order: Mutex::new(Vec::new()),
            sdk_active_identity: Mutex::new(Some(active_identity)),
            sdk_remote_commands: Mutex::new(HashMap::new()),
            sdk_voice_sessions: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            interfaces: Mutex::new(Vec::new()),
            delivery_policy: Mutex::new(DeliveryPolicy::default()),
            propagation_state: Mutex::new(PropagationState::default()),
            remote_unpeer_failure_state: Mutex::new(None),
            propagation_payloads: Mutex::new(HashMap::new()),
            throttled_propagation_peers: Mutex::new(HashMap::new()),
            outbound_propagation_node: Mutex::new(None),
            paper_ingest_seen: Mutex::new(HashSet::new()),
            stamp_policy: Mutex::new(StampPolicy::default()),
            ticket_cache: Mutex::new(HashMap::new()),
            ticket_last_deliveries: Mutex::new(HashMap::new()),
            delivery_traces,
            daemon_status_snapshot: std::sync::RwLock::new(DaemonStatusSnapshot::default()),
            delivery_status_lock,
            sdk_metrics,
            outbound_bridge,
            outbound_delivery_tx,
            announce_bridge,
            event_sink_bridges,
            event_sink_tx,
            interface_mutation_bridge: Mutex::new(None),
            remote_control_bridge: Mutex::new(None),
            started_at: std::time::Instant::now(),
        };
        let _ = daemon.restore_sdk_domain_snapshot();
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

    pub fn set_sdk_custom_operations(&self, operations: Vec<SdkCustomOperationSpec>) {
        let mut guard =
            self.sdk_custom_operations.lock().expect("sdk_custom_operations mutex poisoned");
        *guard = operations
            .into_iter()
            .map(|mut operation| {
                operation.id = operation.id.trim().to_owned();
                operation.group = operation.group.trim().to_owned();
                operation.kind = operation.kind.trim().to_ascii_lowercase();
                operation.transport_variant = operation.transport_variant.trim().to_owned();
                operation.description = operation.description.trim().to_owned();
                operation.aliases = operation
                    .aliases
                    .into_iter()
                    .map(|alias| alias.trim().to_owned())
                    .filter(|alias| !alias.is_empty())
                    .collect();
                operation.required_capabilities = operation
                    .required_capabilities
                    .into_iter()
                    .map(|capability| capability.trim().to_owned())
                    .filter(|capability| !capability.is_empty())
                    .collect();
                operation
            })
            .filter(|operation| {
                !operation.id.is_empty()
                    && !operation.group.is_empty()
                    && matches!(operation.kind.as_str(), "query" | "command")
                    && !operation.transport_variant.is_empty()
            })
            .collect();
    }

    pub fn with_sdk_custom_operations(self, operations: Vec<SdkCustomOperationSpec>) -> Self {
        self.set_sdk_custom_operations(operations);
        self
    }

    pub fn ensure_ticket(
        &self,
        destination: &str,
        ttl_secs: Option<u64>,
    ) -> Result<TicketRecord, std::io::Error> {
        self.issue_ticket(destination, ttl_secs)
    }

    pub fn generate_ticket(
        &self,
        destination: &str,
        ttl_secs: Option<u64>,
    ) -> Result<Option<TicketRecord>, std::io::Error> {
        if self.ticket_interval_active(destination) {
            return Ok(None);
        }
        self.issue_ticket(destination, ttl_secs).map(Some)
    }
}

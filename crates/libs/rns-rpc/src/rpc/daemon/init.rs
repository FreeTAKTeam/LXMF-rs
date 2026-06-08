use super::dispatch_legacy_messages::{peer_sync_backoff_active, LocalUnpeerCleanup};
use super::*;

pub(super) const LXMF_PEER_SYNC_BACKOFF_STEP_SECS: u32 = 12 * 60;
pub(super) const LXMF_PEER_MAX_UNREACHABLE_SECS: i64 = 14 * 24 * 60 * 60;
const LXMF_PEER_FASTEST_RANDOM_POOL: usize = 2;
const LXMF_PEER_ROTATION_HEADROOM_PCT: usize = 10;
const LXMF_PEER_ROTATION_ACCEPTANCE_RATE_MAX: f64 = 0.5;

#[derive(Debug, Clone, Copy)]
pub(super) struct PeerPropagationState {
    pub(super) transfer_limit: Option<u32>,
    pub(super) sync_limit: Option<u32>,
    pub(super) stamp_cost: Option<u32>,
    pub(super) stamp_cost_flexibility: Option<u32>,
    pub(super) peering_cost: Option<u32>,
    pub(super) network_distance: Option<u32>,
    pub(super) peering_timebase: Option<i64>,
}

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

    fn issue_ticket(
        &self,
        destination: &str,
        ttl_secs: Option<u64>,
    ) -> Result<TicketRecord, std::io::Error> {
        use rand_core::{OsRng, RngCore};

        let ttl_secs = ttl_secs.unwrap_or(Self::DEFAULT_TICKET_EXPIRY_SECS);
        let ttl = i64::try_from(ttl_secs).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("ttl_secs exceeds supported range: {ttl_secs}"),
            )
        })?;
        let now = now_i64();
        self.prune_expired_tickets(now);
        let mut guard = self.ticket_cache.lock().expect("ticket mutex poisoned");
        if let Some(existing) = guard.get(destination).cloned() {
            if existing.expires_at - now > Self::TICKET_RENEW_SECS {
                return Ok(existing);
            }
        }
        for (ticket, expires_at) in
            self.store.get_tickets_for_destination(destination).map_err(std::io::Error::other)?
        {
            if expires_at - now <= Self::TICKET_RENEW_SECS {
                continue;
            }
            let record = TicketRecord { destination: destination.to_string(), ticket, expires_at };
            guard.insert(destination.to_string(), record.clone());
            return Ok(record);
        }

        let expires_at = now.checked_add(ttl).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("ttl_secs causes timestamp overflow: {ttl_secs}"),
            )
        })?;
        let mut ticket = [0u8; 16];
        OsRng.fill_bytes(&mut ticket);
        let record = TicketRecord {
            destination: destination.to_string(),
            ticket: hex::encode(ticket),
            expires_at,
        };
        self.store
            .upsert_ticket(record.destination.as_str(), record.ticket.as_str(), record.expires_at)
            .map_err(std::io::Error::other)?;
        guard.insert(destination.to_string(), record.clone());
        Ok(record)
    }

    pub fn mark_ticket_delivered(&self, destination: &str) {
        let delivered_at = now_i64();
        self.ticket_last_deliveries
            .lock()
            .expect("ticket delivery mutex poisoned")
            .insert(destination.to_string(), delivered_at);
        let _ = self.store.upsert_ticket_last_delivery(destination, delivered_at);
    }

    fn ticket_interval_active(&self, destination: &str) -> bool {
        let now = now_i64();
        if self
            .ticket_last_deliveries
            .lock()
            .expect("ticket delivery mutex poisoned")
            .get(destination)
            .is_some_and(|last_delivery| {
                now.saturating_sub(*last_delivery) < Self::TICKET_INTERVAL_SECS
            })
        {
            return true;
        }

        self.store.get_ticket_last_delivery(destination).ok().flatten().is_some_and(
            |last_delivery| now.saturating_sub(last_delivery) < Self::TICKET_INTERVAL_SECS,
        )
    }

    pub fn current_stamp_policy(&self) -> StampPolicy {
        self.stamp_policy.lock().expect("stamp mutex poisoned").clone()
    }

    pub fn current_propagation_state(&self) -> PropagationState {
        self.propagation_state.lock().expect("propagation mutex poisoned").clone()
    }

    pub fn valid_issued_tickets_for(&self, destination: &str) -> Vec<Vec<u8>> {
        let now = now_i64();
        self.prune_expired_tickets(now);
        let mut seen = HashSet::new();
        let mut tickets = Vec::new();
        if let Some(ticket) = self
            .ticket_cache
            .lock()
            .expect("ticket mutex poisoned")
            .get(destination)
            .filter(|record| record.expires_at > now)
            .and_then(|record| hex::decode(record.ticket.as_str()).ok())
        {
            seen.insert(ticket.clone());
            tickets.push(ticket);
        }

        for (ticket, expires_at) in
            self.store.get_tickets_for_destination(destination).unwrap_or_default()
        {
            if expires_at <= now {
                continue;
            }
            let Ok(ticket) = hex::decode(ticket.as_str()) else {
                continue;
            };
            if seen.insert(ticket.clone()) {
                tickets.push(ticket);
            }
        }
        tickets
    }

    pub fn remember_outbound_ticket(
        &self,
        destination: &str,
        ticket: &str,
        expires_at: i64,
    ) -> Result<(), std::io::Error> {
        let ticket = ticket.trim();
        if hex::decode(ticket).map(|bytes| bytes.len()).unwrap_or_default() != 16 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "outbound ticket must be 16 bytes of hex",
            ));
        }
        self.store
            .upsert_outbound_ticket(destination, ticket, expires_at)
            .map_err(std::io::Error::other)
    }

    pub fn outbound_ticket_for(
        &self,
        destination: &str,
    ) -> Result<Option<TicketRecord>, std::io::Error> {
        self.prune_expired_tickets(now_i64());
        let Some((ticket, expires_at)) =
            self.store.get_outbound_ticket(destination).map_err(std::io::Error::other)?
        else {
            return Ok(None);
        };
        if expires_at <= now_i64() {
            return Ok(None);
        }
        Ok(Some(TicketRecord { destination: destination.to_string(), ticket, expires_at }))
    }

    fn prune_expired_tickets(&self, now: i64) {
        let _ = self.store.prune_expired_tickets(now, Self::TICKET_GRACE_SECS);
    }

    pub fn message_receipt_status(
        &self,
        message_id: &str,
    ) -> Result<Option<String>, std::io::Error> {
        Ok(self
            .store
            .get_message(message_id)
            .map_err(std::io::Error::other)?
            .and_then(|message| message.receipt_status))
    }

    pub fn record_message_lxmf_metadata(
        &self,
        message_id: &str,
        key: &str,
        value: JsonValue,
    ) -> Result<(), std::io::Error> {
        self.record_message_lxmf_metadata_entries(message_id, [(key.to_string(), value)])
    }

    pub fn record_message_lxmf_metadata_entries(
        &self,
        message_id: &str,
        entries: impl IntoIterator<Item = (String, JsonValue)>,
    ) -> Result<(), std::io::Error> {
        let Some(message) = self.store.get_message(message_id).map_err(std::io::Error::other)?
        else {
            return Ok(());
        };
        let mut root = match message.fields {
            Some(JsonValue::Object(map)) => map,
            Some(other) => {
                let mut map = serde_json::Map::new();
                map.insert("_fields_raw".to_string(), other);
                map
            }
            None => serde_json::Map::new(),
        };
        let mut lxmf = match root.remove("_lxmf") {
            Some(JsonValue::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        for (key, value) in entries {
            lxmf.insert(key, value);
        }
        root.insert("_lxmf".to_string(), JsonValue::Object(lxmf));
        self.store
            .update_message_fields(message_id, Some(&JsonValue::Object(root)))
            .map_err(std::io::Error::other)
    }

    pub fn replace_interfaces(&self, interfaces: Vec<InterfaceRecord>) {
        let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
        *guard = interfaces.clone();
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.interfaces = interfaces;
        });
    }

    pub fn set_interface_mutation_bridge(&self, bridge: Arc<dyn InterfaceMutationBridge>) {
        let mut guard = self
            .interface_mutation_bridge
            .lock()
            .expect("interface mutation bridge mutex poisoned");
        *guard = Some(bridge);
    }

    pub fn set_remote_control_bridge(&self, bridge: Arc<dyn RemoteControlBridge>) {
        let mut guard =
            self.remote_control_bridge.lock().expect("remote_control_bridge mutex poisoned");
        *guard = Some(bridge);
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
        let state = guard.clone();
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.propagation = state;
        });
    }

    pub fn outbound_propagation_node(&self) -> Option<String> {
        self.outbound_propagation_node.lock().expect("propagation node mutex poisoned").clone()
    }

    pub fn outbound_stamp_cost_for(
        &self,
        destination: &str,
    ) -> Result<Option<u32>, std::io::Error> {
        self.store.latest_announce_stamp_cost_for(destination).map_err(std::io::Error::other)
    }

    pub fn message_storage_stats(&self) -> Result<(u64, u64), std::io::Error> {
        let stats = self.store.message_storage_stats().map_err(std::io::Error::other)?;
        Ok((stats.count, stats.bytes))
    }

    pub fn message_exists(&self, message_id: &str) -> Result<bool, std::io::Error> {
        Ok(self.store.get_message(message_id).map_err(std::io::Error::other)?.is_some())
    }

    pub fn peer_message_stats(
        &self,
        peer: &str,
    ) -> Result<(u64, u64, u64, u64, u64, u64), std::io::Error> {
        let stats = self.store.peer_message_stats(peer).map_err(std::io::Error::other)?;
        let propagation =
            self.store.peer_propagation_message_stats(peer).map_err(std::io::Error::other)?;
        let (record_offered, record_outgoing, record_incoming) = self
            .peers
            .lock()
            .ok()
            .and_then(|guard| {
                guard.get(peer).map(|record| (record.offered, record.outgoing, record.incoming))
            })
            .unwrap_or((0, 0, 0));
        Ok((
            stats.outgoing.saturating_add(record_outgoing.max(propagation.outgoing)),
            stats.incoming.saturating_add(record_incoming.max(propagation.incoming)),
            stats.offered.saturating_add(record_offered.max(propagation.offered)),
            stats.unhandled.saturating_add(propagation.unhandled),
            propagation.offered_bytes,
            propagation.unhandled_bytes,
        ))
    }

    pub fn record_inbound_peer_activity(&self, peer: &str, bytes: usize) {
        let peer = peer.trim();
        if let Ok(mut guard) = self.peers.lock() {
            if let Some(existing) =
                guard.values_mut().find(|record| record.peer.eq_ignore_ascii_case(peer))
            {
                existing.alive = true;
                existing.last_seen = now_i64();
                existing.rx_bytes = existing.rx_bytes.saturating_add(bytes as u64);
            }
        }
    }

    pub fn record_inbound_propagation_peer_activity(&self, peer: &str, bytes: usize) -> bool {
        self.record_inbound_propagation_peer_activity_count(peer, bytes, 1)
    }

    pub fn record_inbound_propagation_peer_activity_count(
        &self,
        peer: &str,
        bytes: usize,
        messages: usize,
    ) -> bool {
        let peer = peer.trim();
        if let Ok(mut guard) = self.peers.lock() {
            if let Some(existing) = guard.values_mut().find(|record| {
                record.peer_type.as_deref() != Some("unpeered")
                    && record.peer.eq_ignore_ascii_case(peer)
            }) {
                existing.alive = true;
                existing.last_seen = now_i64();
                existing.incoming = existing.incoming.saturating_add(messages as u64);
                existing.rx_bytes = existing.rx_bytes.saturating_add(bytes as u64);
                return true;
            }
        }
        false
    }

    pub fn record_outbound_peer_activity(&self, peer: &str, bytes: usize, delivered: bool) {
        let peer = peer.trim();
        if let Ok(mut guard) = self.peers.lock() {
            if let Some(existing) =
                guard.values_mut().find(|record| record.peer.eq_ignore_ascii_case(peer))
            {
                let now = now_i64();
                existing.tx_bytes = existing.tx_bytes.saturating_add(bytes as u64);
                existing.last_sync_attempt = now;
                if !delivered {
                    existing.sync_backoff =
                        existing.sync_backoff.saturating_add(LXMF_PEER_SYNC_BACKOFF_STEP_SECS);
                    existing.next_sync_attempt =
                        now.saturating_add(i64::from(existing.sync_backoff));
                    existing.alive = false;
                    existing.acceptance_rate = (existing.acceptance_rate * 0.9).max(0.0);
                } else {
                    existing.alive = true;
                    existing.last_seen = now;
                    existing.sync_backoff = 0;
                    existing.next_sync_attempt = 0;
                    existing.acceptance_rate =
                        ((existing.acceptance_rate * 0.8) + 0.2).clamp(0.0, 1.0);
                }
            }
        }
    }

    pub fn record_outbound_peer_sent(&self, peer: &str, bytes: usize) {
        let peer = peer.trim();
        if let Ok(mut guard) = self.peers.lock() {
            if let Some(existing) =
                guard.values_mut().find(|record| record.peer.eq_ignore_ascii_case(peer))
            {
                existing.tx_bytes = existing.tx_bytes.saturating_add(bytes as u64);
                existing.last_sync_attempt = now_i64();
            }
        }
    }

    pub fn record_message_delivery_receipt(&self, message_id: &str) -> Result<(), std::io::Error> {
        let Some(message) = self.store.get_message(message_id).map_err(std::io::Error::other)?
        else {
            return Ok(());
        };
        if message.direction == "out" {
            self.record_outbound_peer_activity(message.destination.as_str(), 0, true);
        }
        Ok(())
    }

    pub fn record_unpeered_propagation_attempt(&self, bytes: usize) {
        let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
        guard.unpeered_propagation_incoming = guard.unpeered_propagation_incoming.saturating_add(1);
        guard.unpeered_propagation_rx_bytes =
            guard.unpeered_propagation_rx_bytes.saturating_add(bytes as u64);
        let state = guard.clone();
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.propagation = state;
        });
    }

    pub fn update_propagation_sync_state<F>(&self, update: F)
    where
        F: FnOnce(&mut PropagationState),
    {
        let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
        update(&mut guard);
        let state = guard.clone();
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.propagation = state;
        });
    }

    pub(super) fn update_daemon_status_snapshot<F>(&self, update: F)
    where
        F: FnOnce(&mut DaemonStatusSnapshot),
    {
        let mut guard =
            self.daemon_status_snapshot.write().expect("daemon_status_snapshot rwlock poisoned");
        update(&mut guard);
    }

    pub(super) fn daemon_status_snapshot(&self) -> DaemonStatusSnapshot {
        self.daemon_status_snapshot.read().expect("daemon_status_snapshot rwlock poisoned").clone()
    }

    pub(super) fn store_inbound_record(
        &self,
        record: MessageRecord,
        raw_lxmf_bytes: Option<&[u8]>,
    ) -> Result<(), std::io::Error> {
        self.store.insert_message(&record).map_err(std::io::Error::other)?;
        let storage_limit_bytes = self
            .propagation_state
            .lock()
            .expect("propagation mutex poisoned")
            .message_storage_limit_mb
            .map(|value| value.saturating_mul(1_000_000));
        if let Some(limit_bytes) = storage_limit_bytes {
            self.store
                .schedule_prune_messages_to_limit_bytes(limit_bytes)
                .map_err(std::io::Error::other)?;
        }
        let mut payload = json!({ "message": record });
        if let Some(raw_lxmf_bytes) = raw_lxmf_bytes {
            payload["lxmf_bytes_hex"] = json!(hex::encode(raw_lxmf_bytes));
        }
        let event = RpcEvent { event_type: "inbound".into(), payload };
        self.publish_event(event);
        Ok(())
    }

    pub fn accept_inbound(&self, record: MessageRecord) -> Result<(), std::io::Error> {
        self.remember_outbound_ticket_from_inbound(&record)?;
        if self.message_exists(record.id.as_str())? {
            return Ok(());
        }
        self.store_inbound_record(record.clone(), None)?;
        let _ = self.correlate_inbound_sdk_command(&record)?;
        Ok(())
    }

    pub fn accept_inbound_with_raw(
        &self,
        record: MessageRecord,
        raw_lxmf_bytes: &[u8],
    ) -> Result<(), std::io::Error> {
        self.remember_outbound_ticket_from_inbound(&record)?;
        if self.message_exists(record.id.as_str())? {
            return Ok(());
        }
        self.store_inbound_record(record.clone(), Some(raw_lxmf_bytes))?;
        let _ = self.correlate_inbound_sdk_command(&record)?;
        Ok(())
    }

    fn remember_outbound_ticket_from_inbound(
        &self,
        record: &MessageRecord,
    ) -> Result<(), std::io::Error> {
        let Some((expires_at, ticket)) = inbound_ticket_from_record(record) else {
            return Ok(());
        };
        if expires_at <= now_i64() {
            return Ok(());
        }
        self.remember_outbound_ticket(record.source.as_str(), ticket.as_str(), expires_at)
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
        self.accept_announce_with_metadata_inner(
            peer,
            timestamp,
            name,
            name_source,
            app_data_hex,
            capabilities,
            rssi,
            snr,
            q,
            stamp_cost,
            stamp_cost_flexibility,
            peering_cost,
            aspect,
            hops,
            interface,
            source_private_key,
            source_identity,
            source_node,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn accept_announce_with_metadata_for_path_response(
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
        is_path_response: bool,
    ) -> Result<(), std::io::Error> {
        self.accept_announce_with_metadata_inner(
            peer,
            timestamp,
            name,
            name_source,
            app_data_hex,
            capabilities,
            rssi,
            snr,
            q,
            stamp_cost,
            stamp_cost_flexibility,
            peering_cost,
            aspect,
            hops,
            interface,
            source_private_key,
            source_identity,
            source_node,
            is_path_response,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn accept_announce_with_metadata_inner(
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
        is_path_response: bool,
    ) -> Result<(), std::io::Error> {
        let stamp_cost_flexibility = stamp_cost_flexibility.flatten();
        let peering_cost = peering_cost.flatten();
        let (propagation_transfer_limit, propagation_sync_limit) =
            parse_propagation_limits_from_app_data_hex(app_data_hex.as_deref());
        let propagation_enabled =
            parse_propagation_enabled_from_app_data_hex(app_data_hex.as_deref());
        let peering_timebase =
            parse_propagation_timebase_from_app_data_hex(app_data_hex.as_deref());
        let propagation_peer_state = PeerPropagationState {
            transfer_limit: propagation_transfer_limit,
            sync_limit: propagation_sync_limit,
            stamp_cost,
            stamp_cost_flexibility,
            peering_cost,
            network_distance: hops,
            peering_timebase,
        };
        let is_static = self.is_static_peer(peer.as_str());
        let remote_peering_cost_allowed = self.remote_peering_cost_allowed(peering_cost);
        if !is_static && !remote_peering_cost_allowed {
            self.remove_peer_if_stale_or_expensive(peer.as_str(), timestamp)?;
        }
        if !is_static && propagation_enabled == Some(false) {
            self.remove_autopeered_peer_if_propagation_disabled(
                peer.as_str(),
                peering_timebase.unwrap_or(timestamp),
            )?;
        }
        let static_peer_last_seen = self
            .peers
            .lock()
            .expect("peers mutex poisoned")
            .values()
            .find(|record| record.peer.eq_ignore_ascii_case(peer.as_str()))
            .map(|record| record.last_seen)
            .unwrap_or_default();
        let static_path_response_refresh_allowed = !is_path_response || static_peer_last_seen == 0;
        let should_peer = (is_static && static_path_response_refresh_allowed)
            || (!is_static
                && propagation_enabled != Some(false)
                && remote_peering_cost_allowed
                && self.should_autopeer_peer(hops));
        let peer_type = if is_static {
            Some("static".to_string())
        } else if should_peer {
            Some("auto".to_string())
        } else {
            Some("discovered".to_string())
        };
        let capability_list = if let Some(caps) = capabilities {
            normalize_capabilities(caps)
        } else {
            parse_capabilities_from_app_data_hex(app_data_hex.as_deref())
        };
        let record = if should_peer {
            let record = match self.upsert_peer(
                peer.clone(),
                timestamp,
                capability_list.clone(),
                name.clone(),
                name_source.clone(),
                peer_type,
            ) {
                Ok(record) => {
                    self.refresh_peer_propagation_state(
                        record.peer.as_str(),
                        timestamp,
                        propagation_peer_state,
                    );
                    self.queue_existing_propagation_for_peer(record.peer.as_str())?;
                    record
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock && !is_static => self
                    .transient_peer_record(
                        peer,
                        timestamp,
                        capability_list.clone(),
                        name,
                        name_source,
                        Some("discovered".to_string()),
                    ),
                Err(err) => return Err(err),
            };
            record
        } else {
            self.transient_peer_record(
                peer,
                timestamp,
                capability_list.clone(),
                name,
                name_source,
                peer_type,
            )
        };

        let announce_record = AnnounceRecord {
            id: format!("announce-{}-{}-{}", timestamp, record.peer, self.next_announce_seq()),
            peer: record.peer.clone(),
            timestamp,
            name: record.name.clone(),
            name_source: record.name_source.clone(),
            first_seen: record.first_seen,
            seen_count: record.seen_count,
            app_data_hex: clean_optional_text(app_data_hex),
            capabilities: capability_list.clone(),
            rssi,
            snr,
            q,
            stamp_cost,
            stamp_cost_flexibility,
            peering_cost,
        };
        self.store.insert_announce(&announce_record).map_err(std::io::Error::other)?;

        let event = RpcEvent {
            event_type: "announce_received".into(),
            payload: json!({
                "id": announce_record.id,
                "peer": record.peer,
                "timestamp": timestamp,
                "name": record.name,
                "name_source": record.name_source,
                "first_seen": record.first_seen,
                "seen_count": record.seen_count,
                "app_data_hex": announce_record.app_data_hex,
                "capabilities": capability_list,
                "rssi": rssi,
                "snr": snr,
                "q": q,
                "stamp_cost": stamp_cost,
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

    pub(super) fn upsert_peer(
        &self,
        peer: String,
        timestamp: i64,
        capabilities: Vec<String>,
        name: Option<String>,
        name_source: Option<String>,
        peer_type: Option<String>,
    ) -> Result<PeerRecord, std::io::Error> {
        let cleaned_name = clean_optional_text(name);
        let cleaned_name_source = clean_optional_text(name_source);
        let cleaned_capabilities = normalize_capabilities(capabilities);

        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        let existing_peer_key =
            guard.keys().find(|existing| existing.eq_ignore_ascii_case(peer.as_str())).cloned();
        if let Some(existing_peer_key) = existing_peer_key {
            let active_peer_count = Self::active_peer_count_from_guard(&guard);
            let existing = guard.get_mut(&existing_peer_key).expect("peer record disappeared");
            let is_newer = timestamp >= existing.last_seen;
            let reactivating_unpeered = existing.peer_type.as_deref() == Some("unpeered")
                && peer_type.as_deref() != Some("unpeered");
            if reactivating_unpeered {
                self.ensure_peer_admission_allowed(&existing_peer_key, active_peer_count)?;
            }
            existing.last_seen = existing.last_seen.max(timestamp);
            existing.seen_count = existing.seen_count.saturating_add(1);
            if is_newer && !cleaned_capabilities.is_empty() {
                existing.capabilities = cleaned_capabilities;
            }
            if is_newer {
                if let Some(name) = cleaned_name {
                    existing.name = Some(name);
                    existing.name_source = cleaned_name_source;
                }
                if let Some(peer_type) = peer_type {
                    existing.peer_type = Some(peer_type);
                }
            }
            if reactivating_unpeered {
                existing.restored_handled_ids.clear();
                existing.restored_unhandled_ids.clear();
            }
            let record = existing.clone();
            let peer_count = Self::active_peer_count_from_guard(&guard);
            drop(guard);
            self.update_daemon_status_snapshot(|snapshot| {
                snapshot.peer_count = peer_count;
            });
            return Ok(record);
        }
        self.ensure_peer_admission_allowed(&peer, Self::active_peer_count_from_guard(&guard))?;

        let record = PeerRecord {
            peer: peer.clone(),
            last_seen: timestamp,
            capabilities: cleaned_capabilities,
            name: cleaned_name,
            name_source: cleaned_name_source,
            peer_type,
            alive: true,
            last_sync_attempt: 0,
            next_sync_attempt: 0,
            sync_backoff: 0,
            network_distance: 1,
            offered: 0,
            outgoing: 0,
            incoming: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            sync_transfer_rate: 0.0,
            acceptance_rate: 0.0,
            first_seen: timestamp,
            seen_count: 1,
            peering_timebase: 0,
            sync_strategy: 2,
            propagation_transfer_limit: None,
            propagation_sync_limit: None,
            propagation_stamp_cost: None,
            propagation_stamp_cost_flexibility: None,
            peering_cost: None,
            peering_key_stamp: None,
            peering_key_value: None,
            restored_handled_ids: Vec::new(),
            restored_unhandled_ids: Vec::new(),
        };
        guard.insert(peer, record.clone());
        let peer_count = Self::active_peer_count_from_guard(&guard);
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.peer_count = peer_count;
        });
        Ok(record)
    }

    pub(super) fn ensure_peer_for_sync(
        &self,
        peer: &str,
        timestamp: i64,
    ) -> Result<PeerRecord, std::io::Error> {
        let peer = peer.trim();
        if peer.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "peer is required"));
        }
        let existing_peer_type = self
            .peers
            .lock()
            .expect("peers mutex poisoned")
            .values()
            .find(|record| record.peer.eq_ignore_ascii_case(peer))
            .and_then(|record| record.peer_type.clone());
        let peer_type = if self.is_static_peer(peer) {
            Some("static".to_string())
        } else if existing_peer_type.as_deref() == Some("unpeered") {
            Some("manual".to_string())
        } else {
            existing_peer_type.or(Some("manual".to_string()))
        };
        self.upsert_peer(peer.to_string(), timestamp, Vec::new(), None, None, peer_type)
    }

    pub(super) fn activate_static_peers(
        &self,
        static_peers: &[String],
    ) -> Result<(), std::io::Error> {
        let configured_static_peers = Self::normalize_static_peers(static_peers);
        let from_static_only =
            self.propagation_state.lock().expect("propagation mutex poisoned").from_static_only;
        let mut removed_static_peers = Vec::new();
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        for existing in guard.values_mut() {
            let is_configured_static = configured_static_peers
                .iter()
                .any(|peer| peer.eq_ignore_ascii_case(existing.peer.as_str()));
            if is_configured_static {
                if existing.peer_type.as_deref() == Some("unpeered") {
                    existing.restored_handled_ids.clear();
                    existing.restored_unhandled_ids.clear();
                }
                existing.peer_type = Some("static".to_string());
            } else if existing.peer_type.as_deref() == Some("static") {
                if from_static_only {
                    removed_static_peers.push(existing.peer.clone());
                } else {
                    existing.peer_type = Some("manual".to_string());
                }
            }
        }
        let mut static_peers_to_queue = Vec::new();
        for peer in &configured_static_peers {
            let existing_peer_key =
                guard.keys().find(|existing| existing.eq_ignore_ascii_case(peer.as_str())).cloned();
            if let Some(existing_peer_key) = existing_peer_key {
                let existing = guard.get_mut(&existing_peer_key).expect("peer record disappeared");
                existing.peer_type = Some("static".to_string());
                static_peers_to_queue.push(existing_peer_key);
                continue;
            }

            guard.insert(
                peer.clone(),
                PeerRecord {
                    peer: peer.clone(),
                    last_seen: 0,
                    capabilities: Vec::new(),
                    name: None,
                    name_source: None,
                    peer_type: Some("static".to_string()),
                    alive: false,
                    last_sync_attempt: 0,
                    next_sync_attempt: 0,
                    sync_backoff: 0,
                    network_distance: 1,
                    offered: 0,
                    outgoing: 0,
                    incoming: 0,
                    rx_bytes: 0,
                    tx_bytes: 0,
                    sync_transfer_rate: 0.0,
                    acceptance_rate: 0.0,
                    first_seen: 0,
                    seen_count: 0,
                    peering_timebase: 0,
                    sync_strategy: 2,
                    propagation_transfer_limit: None,
                    propagation_sync_limit: None,
                    propagation_stamp_cost: None,
                    propagation_stamp_cost_flexibility: None,
                    peering_cost: None,
                    peering_key_stamp: None,
                    peering_key_value: None,
                    restored_handled_ids: Vec::new(),
                    restored_unhandled_ids: Vec::new(),
                },
            );
            static_peers_to_queue.push(peer.clone());
        }
        let peer_count = Self::active_peer_count_from_guard(&guard);
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.peer_count = peer_count;
        });
        for peer in removed_static_peers {
            let cleanup = self.unpeer_local_state(peer.as_str())?;
            if cleanup.removed {
                self.publish_event(RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: policy_unpeer_event_payload(
                        cleanup.peer.as_str(),
                        "static_only_policy",
                        &cleanup,
                    ),
                });
            }
        }
        for peer in static_peers_to_queue {
            self.queue_existing_propagation_for_peer(peer.as_str())?;
        }
        Ok(())
    }

    pub(super) fn enforce_static_only_peer_policy(&self) -> Result<(), std::io::Error> {
        let propagation =
            self.propagation_state.lock().expect("propagation mutex poisoned").clone();
        if !propagation.from_static_only {
            return Ok(());
        }
        let peers_to_remove = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard
                .values()
                .filter(|record| record.peer_type.as_deref() != Some("unpeered"))
                .filter(|record| {
                    !propagation
                        .static_peers
                        .iter()
                        .any(|peer| peer.eq_ignore_ascii_case(record.peer.as_str()))
                })
                .map(|record| record.peer.clone())
                .collect::<Vec<_>>()
        };
        for peer in peers_to_remove {
            let cleanup = self.unpeer_local_state(peer.as_str())?;
            if cleanup.removed {
                self.publish_event(RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: policy_unpeer_event_payload(
                        peer.as_str(),
                        "static_only_policy",
                        &cleanup,
                    ),
                });
            }
        }
        Ok(())
    }

    pub(super) fn enforce_autopeer_enabled_policy(&self) -> Result<(), std::io::Error> {
        let autopeer = self.propagation_state.lock().expect("propagation mutex poisoned").autopeer;
        if autopeer {
            return Ok(());
        }
        let peers_to_remove = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard
                .values()
                .filter(|record| record.peer_type.as_deref() == Some("auto"))
                .map(|record| record.peer.clone())
                .collect::<Vec<_>>()
        };
        for peer in peers_to_remove {
            let cleanup = self.unpeer_local_state(peer.as_str())?;
            if cleanup.removed {
                self.publish_event(RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: policy_unpeer_event_payload(
                        peer.as_str(),
                        "autopeer_disabled",
                        &cleanup,
                    ),
                });
            }
        }
        Ok(())
    }

    pub(super) fn enforce_autopeer_maxdepth_policy(&self) -> Result<(), std::io::Error> {
        let propagation =
            self.propagation_state.lock().expect("propagation mutex poisoned").clone();
        if !propagation.autopeer || propagation.from_static_only {
            return Ok(());
        }
        let max_depth = propagation.autopeer_maxdepth.max(1);
        let peers_to_remove = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard
                .values()
                .filter(|record| record.peer_type.as_deref() == Some("auto"))
                .filter(|record| record.network_distance > max_depth)
                .map(|record| record.peer.clone())
                .collect::<Vec<_>>()
        };
        for peer in peers_to_remove {
            let cleanup = self.unpeer_local_state(peer.as_str())?;
            if cleanup.removed {
                self.publish_event(RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: policy_unpeer_event_payload(
                        peer.as_str(),
                        "autopeer_maxdepth",
                        &cleanup,
                    ),
                });
            }
        }
        Ok(())
    }

    pub(super) fn cull_unreachable_non_static_peers(
        &self,
        timestamp: i64,
    ) -> Result<Vec<String>, std::io::Error> {
        let static_peers =
            self.propagation_state.lock().expect("propagation mutex poisoned").static_peers.clone();
        let mut peers_to_remove = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard
                .values()
                .filter(|record| record.peer_type.as_deref() != Some("unpeered"))
                .filter(|record| {
                    !static_peers.iter().any(|peer| peer.eq_ignore_ascii_case(record.peer.as_str()))
                })
                .filter(|record| {
                    timestamp > record.last_seen.saturating_add(LXMF_PEER_MAX_UNREACHABLE_SECS)
                })
                .map(|record| record.peer.clone())
                .collect::<Vec<_>>()
        };
        peers_to_remove.sort();
        let mut removed = Vec::new();
        for peer in peers_to_remove {
            let cleanup = self.unpeer_local_state(peer.as_str())?;
            if cleanup.removed {
                self.publish_event(RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: policy_unpeer_event_payload(
                        peer.as_str(),
                        "max_unreachable",
                        &cleanup,
                    ),
                });
                removed.push(peer);
            }
        }
        Ok(removed)
    }

    pub(super) fn rotate_low_acceptance_non_static_peers(
        &self,
    ) -> Result<Vec<String>, std::io::Error> {
        let (max_peers, static_peers) = {
            let propagation = self.propagation_state.lock().expect("propagation mutex poisoned");
            let Some(max_peers) = propagation.max_peers else {
                return Ok(Vec::new());
            };
            (max_peers as usize, propagation.static_peers.clone())
        };
        if max_peers == 0 {
            return Ok(Vec::new());
        }
        let headroom = ((max_peers * LXMF_PEER_ROTATION_HEADROOM_PCT) / 100).max(1);
        let active_peers = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard
                .values()
                .filter(|record| record.peer_type.as_deref() != Some("unpeered"))
                .cloned()
                .collect::<Vec<_>>()
        };
        let required_drops = active_peers.len().saturating_sub(max_peers.saturating_sub(headroom));
        if required_drops == 0 || active_peers.len().saturating_sub(required_drops) <= 1 {
            return Ok(Vec::new());
        }
        let untested_count =
            active_peers.iter().filter(|record| record.last_sync_attempt == 0).count();
        if untested_count >= headroom {
            return Ok(Vec::new());
        }

        let mut peer_stats = Vec::with_capacity(active_peers.len());
        for record in active_peers {
            let stats = self
                .store
                .peer_propagation_message_stats(record.peer.as_str())
                .map_err(std::io::Error::other)?;
            peer_stats.push((record, stats.unhandled));
        }
        if peer_stats.iter().any(|(_, unhandled)| *unhandled == 0) {
            peer_stats.retain(|(_, unhandled)| *unhandled == 0);
        }

        let mut unresponsive = Vec::new();
        let mut waiting = Vec::new();
        for (record, _unhandled) in peer_stats {
            let is_static =
                static_peers.iter().any(|peer| peer.eq_ignore_ascii_case(record.peer.as_str()));
            if is_static {
                continue;
            }
            if record.alive {
                if record.offered > 0 {
                    waiting.push(record);
                }
            } else {
                unresponsive.push(record);
            }
        }

        let mut drop_pool = Vec::new();
        if unresponsive.is_empty() {
            drop_pool.extend(waiting);
        } else {
            drop_pool.extend(unresponsive);
            drop_pool.extend(waiting);
        }
        drop_pool.sort_by(|left, right| {
            peer_rotation_acceptance_rate(left)
                .total_cmp(&peer_rotation_acceptance_rate(right))
                .then_with(|| left.peer.cmp(&right.peer))
        });

        let mut removed = Vec::new();
        for record in drop_pool.into_iter().take(required_drops) {
            if peer_rotation_acceptance_rate(&record) >= LXMF_PEER_ROTATION_ACCEPTANCE_RATE_MAX {
                continue;
            }
            let cleanup = self.unpeer_local_state(record.peer.as_str())?;
            if cleanup.removed {
                self.publish_event(RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: policy_unpeer_event_payload(
                        record.peer.as_str(),
                        "peer_rotation",
                        &cleanup,
                    ),
                });
                removed.push(record.peer);
            }
        }
        removed.sort();
        Ok(removed)
    }

    pub(super) fn select_peer_for_maintenance_sync(
        &self,
        timestamp: i64,
    ) -> Result<Option<String>, std::io::Error> {
        let active_peers = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard
                .values()
                .filter(|record| record.peer_type.as_deref() != Some("unpeered"))
                .cloned()
                .collect::<Vec<_>>()
        };

        let mut waiting = Vec::new();
        let mut unresponsive = Vec::new();
        for record in active_peers {
            if timestamp > record.last_seen.saturating_add(LXMF_PEER_MAX_UNREACHABLE_SECS) {
                continue;
            }
            let stats = self
                .store
                .peer_propagation_message_stats(record.peer.as_str())
                .map_err(std::io::Error::other)?;
            if stats.unhandled == 0 {
                continue;
            }
            if peer_sync_backoff_active(timestamp, record.next_sync_attempt) {
                continue;
            }
            if record.alive {
                waiting.push(record);
            } else {
                unresponsive.push(record);
            }
        }

        if !waiting.is_empty() {
            waiting.sort_by(|left, right| {
                right
                    .sync_transfer_rate
                    .total_cmp(&left.sync_transfer_rate)
                    .then_with(|| left.peer.cmp(&right.peer))
            });
            let fastest_count = LXMF_PEER_FASTEST_RANDOM_POOL.min(waiting.len());
            let mut peer_pool = waiting.iter().take(fastest_count).cloned().collect::<Vec<_>>();
            peer_pool.extend(
                waiting
                    .iter()
                    .filter(|record| record.sync_transfer_rate == 0.0)
                    .take(fastest_count)
                    .cloned(),
            );
            let selected_index = timestamp.rem_euclid(peer_pool.len() as i64) as usize;
            return Ok(peer_pool.into_iter().nth(selected_index).map(|record| record.peer));
        }

        if !unresponsive.is_empty() {
            unresponsive.sort_by(|left, right| left.peer.cmp(&right.peer));
            let selected_index = timestamp.rem_euclid(unresponsive.len() as i64) as usize;
            return Ok(unresponsive.into_iter().nth(selected_index).map(|record| record.peer));
        }
        Ok(None)
    }

    pub(super) fn ensure_peer_admission_allowed(
        &self,
        peer: &str,
        current_peer_count: usize,
    ) -> Result<(), std::io::Error> {
        let propagation =
            self.propagation_state.lock().expect("propagation mutex poisoned").clone();
        let is_static_peer =
            propagation.static_peers.iter().any(|candidate| candidate.eq_ignore_ascii_case(peer));
        if propagation.from_static_only && !is_static_peer {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("peer {peer} rejected by from_static_only policy"),
            ));
        }
        if let Some(limit) = propagation.max_peers {
            if current_peer_count >= limit as usize && !is_static_peer {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    format!("peer {peer} rejected because max_peers={limit} is reached"),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn is_static_peer(&self, peer: &str) -> bool {
        let propagation = self.propagation_state.lock().expect("propagation mutex poisoned");
        propagation.static_peers.iter().any(|candidate| candidate.eq_ignore_ascii_case(peer))
    }

    pub(super) fn should_autopeer_peer(&self, hops: Option<u32>) -> bool {
        let propagation = self.propagation_state.lock().expect("propagation mutex poisoned");
        if propagation.from_static_only || !propagation.autopeer {
            return false;
        }
        hops.unwrap_or(1) <= propagation.autopeer_maxdepth.max(1)
    }

    pub(super) fn remote_peering_cost_allowed(&self, peering_cost: Option<u32>) -> bool {
        let propagation = self.propagation_state.lock().expect("propagation mutex poisoned");
        match (peering_cost, propagation.remote_peering_cost_max) {
            (Some(remote_cost), Some(max_cost)) => remote_cost <= max_cost,
            _ => true,
        }
    }

    pub(super) fn refresh_peer_propagation_state(
        &self,
        peer: &str,
        timestamp: i64,
        state: PeerPropagationState,
    ) {
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        let Some(existing) = guard.get_mut(peer) else {
            return;
        };
        let peering_timebase = state.peering_timebase.unwrap_or(timestamp);
        if peering_timebase <= existing.peering_timebase {
            return;
        }

        existing.alive = true;
        existing.sync_backoff = 0;
        existing.next_sync_attempt = 0;
        existing.peering_timebase = peering_timebase;
        existing.propagation_transfer_limit = state.transfer_limit;
        existing.propagation_sync_limit = state.sync_limit.or(state.transfer_limit);
        existing.propagation_stamp_cost = state.stamp_cost;
        existing.propagation_stamp_cost_flexibility = state.stamp_cost_flexibility;
        existing.peering_cost = state.peering_cost;
        if let Some(network_distance) = state.network_distance {
            existing.network_distance = network_distance.max(1);
        }
    }

    pub(super) fn remove_peer_if_stale_or_expensive(
        &self,
        peer: &str,
        timestamp: i64,
    ) -> Result<(), std::io::Error> {
        let peer_key = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard
                .keys()
                .find(|existing| existing.eq_ignore_ascii_case(peer))
                .cloned()
                .unwrap_or_else(|| peer.to_string())
        };
        let propagation_stats = self
            .store
            .peer_propagation_message_stats(peer_key.as_str())
            .map_err(std::io::Error::other)?;
        let handled_ids = self
            .store
            .list_peer_handled_propagation_ids(peer_key.as_str())
            .map_err(std::io::Error::other)?;
        let unhandled_ids = self
            .store
            .list_peer_unhandled_propagation_ids(peer_key.as_str())
            .map_err(std::io::Error::other)?;
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        let should_remove = guard
            .get(peer_key.as_str())
            .is_some_and(|existing| timestamp >= existing.peering_timebase);
        if !should_remove {
            return Ok(());
        }
        let removed = guard.remove(peer_key.as_str()).is_some();
        if !removed {
            return Ok(());
        }
        let peer_count = Self::active_peer_count_from_guard(&guard);
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.peer_count = peer_count;
        });
        self.store
            .clear_peer_propagation_marks(peer_key.as_str())
            .map_err(std::io::Error::other)?;
        let messages = json!({
            "offered": propagation_stats.offered,
            "unhandled": propagation_stats.unhandled,
            "offered_bytes": propagation_stats.offered_bytes,
            "unhandled_bytes": propagation_stats.unhandled_bytes,
            "handled_ids": handled_ids,
            "unhandled_ids": unhandled_ids,
        });
        self.publish_event(RpcEvent {
            event_type: "peer_unpeer".into(),
            payload: json!({
                "peer": peer_key.as_str(),
                "removed": true,
                "reason": "peering_cost_policy",
                "propagation_cleared": propagation_stats
                    .offered
                    .saturating_add(propagation_stats.unhandled),
                "propagation_cleared_bytes": propagation_stats
                    .offered_bytes
                    .saturating_add(propagation_stats.unhandled_bytes),
                "messages": messages,
            }),
        });
        let mut cleared_selected_node = false;
        {
            let mut selected =
                self.outbound_propagation_node.lock().expect("propagation node mutex poisoned");
            if selected
                .as_deref()
                .is_some_and(|selected| selected.eq_ignore_ascii_case(peer_key.as_str()))
            {
                *selected = None;
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
        Ok(())
    }

    pub(super) fn remove_autopeered_peer_if_propagation_disabled(
        &self,
        peer: &str,
        peering_timebase: i64,
    ) -> Result<(), std::io::Error> {
        let peer_key = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard
                .keys()
                .find(|existing| existing.eq_ignore_ascii_case(peer))
                .cloned()
                .unwrap_or_else(|| peer.to_string())
        };
        let should_remove = {
            let guard = self.peers.lock().expect("peers mutex poisoned");
            guard.get(peer_key.as_str()).is_some_and(|existing| {
                existing.peer_type.as_deref() == Some("auto")
                    && peering_timebase >= existing.peering_timebase
            })
        };
        if !should_remove {
            return Ok(());
        }
        let cleanup = self.unpeer_local_state(peer_key.as_str())?;
        if cleanup.removed {
            self.publish_event(RpcEvent {
                event_type: "peer_unpeer".into(),
                payload: policy_unpeer_event_payload(
                    cleanup.peer.as_str(),
                    "propagation_disabled",
                    &cleanup,
                ),
            });
        }
        Ok(())
    }

    pub(super) fn transient_peer_record(
        &self,
        peer: String,
        timestamp: i64,
        capabilities: Vec<String>,
        name: Option<String>,
        name_source: Option<String>,
        peer_type: Option<String>,
    ) -> PeerRecord {
        PeerRecord {
            peer,
            last_seen: timestamp,
            capabilities: normalize_capabilities(capabilities),
            name: clean_optional_text(name),
            name_source: clean_optional_text(name_source),
            peer_type,
            alive: true,
            last_sync_attempt: 0,
            next_sync_attempt: 0,
            sync_backoff: 0,
            network_distance: 1,
            offered: 0,
            outgoing: 0,
            incoming: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            sync_transfer_rate: 0.0,
            acceptance_rate: 0.0,
            first_seen: timestamp,
            seen_count: 1,
            peering_timebase: 0,
            sync_strategy: 2,
            propagation_transfer_limit: None,
            propagation_sync_limit: None,
            propagation_stamp_cost: None,
            propagation_stamp_cost_flexibility: None,
            peering_cost: None,
            peering_key_stamp: None,
            peering_key_value: None,
            restored_handled_ids: Vec::new(),
            restored_unhandled_ids: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn accept_inbound_for_test(
        &self,
        record: MessageRecord,
    ) -> Result<(), std::io::Error> {
        self.accept_inbound(record)
    }
}

fn policy_unpeer_event_payload(
    peer: &str,
    reason: &str,
    cleanup: &LocalUnpeerCleanup,
) -> JsonValue {
    let offered = cleanup.messages["offered"].as_u64().unwrap_or(0);
    let outgoing = cleanup.messages["outgoing"].as_u64().unwrap_or(0);
    let incoming = cleanup.messages["incoming"].as_u64().unwrap_or(0);
    json!({
        "peer": peer,
        "removed": true,
        "reason": reason,
        "propagation_cleared": cleanup.propagation_cleared,
        "propagation_cleared_bytes": cleanup.propagation_cleared_bytes,
        "offered": offered,
        "outgoing": outgoing,
        "incoming": incoming,
        "messages": cleanup.messages.clone(),
    })
}

fn peer_rotation_acceptance_rate(peer: &PeerRecord) -> f64 {
    if peer.offered == 0 {
        0.0
    } else {
        (peer.outgoing as f64 / peer.offered as f64).clamp(0.0, 1.0)
    }
}

use super::*;

impl RpcDaemon {
    pub(super) const DEFAULT_TICKET_EXPIRY_SECS: u64 = 21 * 24 * 60 * 60;
    pub(super) const TICKET_RENEW_SECS: i64 = 14 * 24 * 60 * 60;
    pub(super) const TICKET_INTERVAL_SECS: i64 = 24 * 60 * 60;

    pub(super) fn active_peer_count_from_guard(
        guard: &std::collections::HashMap<String, crate::rpc::PeerRecord>,
    ) -> usize {
        guard.values().filter(|record| record.peer_type.as_deref() != Some("unpeered")).count()
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
        let active_identity = identity_hash.clone();
        let mut sdk_identities = HashMap::new();
        sdk_identities
            .insert(identity_hash.clone(), Self::default_sdk_identity(identity_hash.as_str()));
        let daemon = Self {
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
            outbound_propagation_node: Mutex::new(None),
            paper_ingest_seen: Mutex::new(HashSet::new()),
            stamp_policy: Mutex::new(StampPolicy::default()),
            ticket_cache: Mutex::new(HashMap::new()),
            ticket_last_deliveries: Mutex::new(HashMap::new()),
            delivery_traces: Mutex::new(HashMap::new()),
            daemon_status_snapshot: std::sync::RwLock::new(DaemonStatusSnapshot::default()),
            delivery_status_lock: Mutex::new(()),
            sdk_metrics: Mutex::new(RpcMetrics::default()),
            outbound_bridge,
            announce_bridge,
            event_sink_bridges,
            interface_mutation_bridge: Mutex::new(None),
            remote_control_bridge: Mutex::new(None),
        };
        let _ = daemon.restore_sdk_domain_snapshot();
        daemon
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
        let mut guard = self.ticket_cache.lock().expect("ticket mutex poisoned");
        if let Some(existing) = guard.get(destination).cloned() {
            if existing.expires_at - now > Self::TICKET_RENEW_SECS {
                return Ok(existing);
            }
        }
        if let Some((ticket, expires_at)) =
            self.store.get_ticket(destination).map_err(std::io::Error::other)?
        {
            if expires_at - now > Self::TICKET_RENEW_SECS {
                let record =
                    TicketRecord { destination: destination.to_string(), ticket, expires_at };
                guard.insert(destination.to_string(), record.clone());
                return Ok(record);
            }
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
        if let Some(ticket) = self
            .ticket_cache
            .lock()
            .expect("ticket mutex poisoned")
            .get(destination)
            .filter(|record| record.expires_at > now)
            .and_then(|record| hex::decode(record.ticket.as_str()).ok())
        {
            return vec![ticket];
        }

        self.store
            .get_ticket(destination)
            .ok()
            .flatten()
            .filter(|(_, expires_at)| *expires_at > now)
            .and_then(|(ticket, _)| hex::decode(ticket.as_str()).ok())
            .into_iter()
            .collect()
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

    pub fn peer_message_stats(&self, peer: &str) -> Result<(u64, u64, u64, u64), std::io::Error> {
        let stats = self.store.peer_message_stats(peer).map_err(std::io::Error::other)?;
        Ok((stats.outgoing, stats.incoming, stats.offered, stats.unhandled))
    }

    pub fn record_inbound_peer_activity(&self, peer: &str, bytes: usize) {
        if let Ok(mut guard) = self.peers.lock() {
            if let Some(existing) = guard.get_mut(peer) {
                existing.alive = true;
                existing.last_seen = now_i64();
                existing.rx_bytes = existing.rx_bytes.saturating_add(bytes as u64);
            }
        }
    }

    pub fn record_outbound_peer_activity(&self, peer: &str, bytes: usize, delivered: bool) {
        if let Ok(mut guard) = self.peers.lock() {
            if let Some(existing) = guard.get_mut(peer) {
                let now = now_i64();
                existing.tx_bytes = existing.tx_bytes.saturating_add(bytes as u64);
                existing.alive = true;
                existing.last_sync_attempt = now;
                if !delivered {
                    existing.sync_backoff = existing.sync_backoff.saturating_add(1);
                    existing.next_sync_attempt =
                        now.saturating_add(i64::from(existing.sync_backoff) * 30);
                    existing.acceptance_rate = (existing.acceptance_rate * 0.9).max(0.0);
                } else {
                    existing.sync_backoff = 0;
                    existing.next_sync_attempt = 0;
                    existing.acceptance_rate =
                        ((existing.acceptance_rate * 0.8) + 0.2).clamp(0.0, 1.0);
                }
            }
        }
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
            .map(|value| value.saturating_mul(1024 * 1024));
        if let Some(limit_bytes) = storage_limit_bytes {
            let pruned_ids = self
                .store
                .prune_messages_to_limit_bytes(limit_bytes)
                .map_err(std::io::Error::other)?;
            if !pruned_ids.is_empty() {
                self.publish_event(RpcEvent {
                    event_type: "propagation_store_pruned".into(),
                    payload: json!({
                        "limit_bytes": limit_bytes,
                        "pruned_ids": pruned_ids,
                    }),
                });
            }
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
        let stamp_cost_flexibility = stamp_cost_flexibility.flatten();
        let peering_cost = peering_cost.flatten();
        let is_static = self.is_static_peer(peer.as_str());
        let remote_peering_cost_allowed = self.remote_peering_cost_allowed(peering_cost);
        if !is_static && !remote_peering_cost_allowed {
            self.remove_autopeered_peer_if_stale_or_expensive(peer.as_str(), timestamp);
        }
        let should_peer =
            is_static || (remote_peering_cost_allowed && self.should_autopeer_peer(hops));
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
            let record = self.upsert_peer(
                peer,
                timestamp,
                capability_list.clone(),
                name,
                name_source,
                peer_type,
            )?;
            self.refresh_peer_propagation_state(
                record.peer.as_str(),
                timestamp,
                stamp_cost,
                stamp_cost_flexibility,
                peering_cost,
            );
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
            id: format!("announce-{}-{}-{}", timestamp, record.peer, record.seen_count),
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
        if let Some(existing) = guard.get_mut(&peer) {
            let is_newer = timestamp >= existing.last_seen;
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
            rx_bytes: 0,
            tx_bytes: 0,
            acceptance_rate: 1.0,
            first_seen: timestamp,
            seen_count: 1,
            peering_timebase: 0,
            propagation_stamp_cost: None,
            propagation_stamp_cost_flexibility: None,
            peering_cost: None,
        };
        guard.insert(peer, record.clone());
        let peer_count = Self::active_peer_count_from_guard(&guard);
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.peer_count = peer_count;
        });
        Ok(record)
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
        stamp_cost: Option<u32>,
        stamp_cost_flexibility: Option<u32>,
        peering_cost: Option<u32>,
    ) {
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        let Some(existing) = guard.get_mut(peer) else {
            return;
        };
        if timestamp < existing.peering_timebase {
            return;
        }

        existing.alive = true;
        existing.sync_backoff = 0;
        existing.next_sync_attempt = 0;
        existing.peering_timebase = timestamp;
        existing.propagation_stamp_cost = stamp_cost;
        existing.propagation_stamp_cost_flexibility = stamp_cost_flexibility;
        existing.peering_cost = peering_cost;
    }

    pub(super) fn remove_autopeered_peer_if_stale_or_expensive(&self, peer: &str, timestamp: i64) {
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        let should_remove = guard.get(peer).is_some_and(|existing| {
            existing.peer_type.as_deref() == Some("auto") && timestamp >= existing.peering_timebase
        });
        if !should_remove {
            return;
        }
        let removed = guard.remove(peer).is_some();
        if !removed {
            return;
        }
        let peer_count = Self::active_peer_count_from_guard(&guard);
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.peer_count = peer_count;
        });
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
            rx_bytes: 0,
            tx_bytes: 0,
            acceptance_rate: 1.0,
            first_seen: timestamp,
            seen_count: 1,
            peering_timebase: 0,
            propagation_stamp_cost: None,
            propagation_stamp_cost_flexibility: None,
            peering_cost: None,
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

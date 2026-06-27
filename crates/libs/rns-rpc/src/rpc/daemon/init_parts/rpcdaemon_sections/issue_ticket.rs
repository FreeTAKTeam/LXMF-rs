impl RpcDaemon {

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

    pub fn propagation_peer_admission_allowed(&self, peer: &str) -> bool {
        let guard = self.peers.lock().expect("peers mutex poisoned");
        if guard.values().any(|record| {
            record.peer.eq_ignore_ascii_case(peer)
                && record.peer_type.as_deref() != Some("unpeered")
        }) {
            return true;
        }
        self.ensure_peer_admission_allowed(peer, Self::active_peer_count_from_guard(&guard)).is_ok()
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

    pub fn update_interface_runtime_metadata_by_iface(
        &self,
        runtime_iface: &str,
        namespace: &str,
        key: &str,
        value: JsonValue,
    ) -> bool {
        let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
        let Some(record) = guard
            .iter_mut()
            .find(|record| Self::interface_runtime_iface(record) == Some(runtime_iface))
        else {
            return false;
        };

        Self::upsert_interface_runtime_metadata(record, namespace, key, value);
        let interfaces = guard.clone();
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.interfaces = interfaces;
        });
        true
    }

    fn interface_runtime_iface(record: &InterfaceRecord) -> Option<&str> {
        record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("iface"))
            .and_then(JsonValue::as_str)
    }

    fn upsert_interface_runtime_metadata(
        record: &mut InterfaceRecord,
        namespace: &str,
        key: &str,
        value: JsonValue,
    ) {
        let mut settings = match record.settings.take() {
            Some(JsonValue::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        let mut runtime = match settings.remove("_runtime") {
            Some(JsonValue::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        let mut scoped = match runtime.remove(namespace) {
            Some(JsonValue::Object(map)) => map,
            _ => serde_json::Map::new(),
        };

        scoped.insert(key.to_string(), value);
        runtime.insert(namespace.to_string(), JsonValue::Object(scoped));
        settings.insert("_runtime".to_string(), JsonValue::Object(runtime));
        record.settings = Some(JsonValue::Object(settings));
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

    pub fn set_rnode_management_bridge(&self, bridge: Arc<dyn RNodeManagementBridge>) {
        let mut guard = self
            .rnode_management_bridge
            .lock()
            .expect("rnode_management_bridge mutex poisoned");
        *guard = Some(bridge);
    }

    pub fn set_weave_display_control_bridge(&self, bridge: Arc<dyn WeaveDisplayControlBridge>) {
        let mut guard = self
            .weave_display_control_bridge
            .lock()
            .expect("weave_display_control_bridge mutex poisoned");
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
        guard.propagation_node_enabled = enabled;
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

    pub fn propagation_transient_exists(&self, transient_id: &str) -> Result<bool, std::io::Error> {
        let transient_id = transient_id.trim();
        if transient_id.is_empty() {
            return Ok(false);
        }
        Ok(self.store.get_propagation_entry(transient_id).map_err(std::io::Error::other)?.is_some())
    }

    pub fn peer_message_stats(
        &self,
        peer: &str,
    ) -> Result<(u64, u64, u64, u64, u64, u64), std::io::Error> {
        let stats = self.store.peer_message_stats(peer).map_err(std::io::Error::other)?;
        let propagation =
            self.store.peer_propagation_message_stats(peer).map_err(std::io::Error::other)?;
        let (record_offered, record_outgoing, record_incoming) = match self.peers.lock() {
            Ok(guard) => {
                guard.get(peer).map(|record| (record.offered, record.outgoing, record.incoming))
            }
            Err(err) => {
                log::warn!("[rpc-daemon] failed to read peer message stats cache for {peer}: {err}");
                None
            }
        }
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
        self.record_inbound_propagation_peer_activity_count_inner(peer, bytes, messages, false)
    }

    pub fn record_successful_remote_propagation_peer_activity_count(
        &self,
        peer: &str,
        bytes: usize,
        messages: usize,
    ) -> bool {
        self.record_inbound_propagation_peer_activity_count_inner(peer, bytes, messages, true)
    }

    fn record_inbound_propagation_peer_activity_count_inner(
        &self,
        peer: &str,
        bytes: usize,
        messages: usize,
        clear_backoff: bool,
    ) -> bool {
        let peer = peer.trim();
        if let Ok(mut guard) = self.peers.lock() {
            if let Some(existing) = guard.values_mut().find(|record| {
                record.peer_type.as_deref() != Some("unpeered")
                    && record.peer.eq_ignore_ascii_case(peer)
            }) {
                let now = now_i64();
                existing.alive = true;
                existing.last_seen = now;
                existing.incoming = existing.incoming.saturating_add(messages as u64);
                existing.rx_bytes = existing.rx_bytes.saturating_add(bytes as u64);
                if clear_backoff {
                    existing.last_sync_attempt = now;
                    existing.sync_backoff = 0;
                    existing.next_sync_attempt = 0;
                }
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
}

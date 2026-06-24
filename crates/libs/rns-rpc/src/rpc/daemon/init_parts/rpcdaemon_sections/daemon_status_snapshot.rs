impl RpcDaemon {

    pub fn record_announce_identity(
        &self,
        peer: &str,
        public_key_hex: &str,
        verifying_key_hex: &str,
        updated_at: i64,
    ) -> Result<(), std::io::Error> {
        self.store
            .upsert_announce_identity(peer, public_key_hex, verifying_key_hex, updated_at)
            .map_err(std::io::Error::other)
    }

    pub fn announce_identity_keys(
        &self,
        peer: &str,
    ) -> Result<Option<(String, String)>, std::io::Error> {
        self.store.announce_identity_keys(peer).map_err(std::io::Error::other)
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
            parse_propagation_limits_from_app_data_hex(app_data_hex.as_deref())
                .unwrap_or_else(|err| {
                    log::warn!("[daemon] failed to decode propagation limits from app_data: {err}");
                    (None, None)
                });
        let propagation_enabled =
            parse_propagation_enabled_from_app_data_hex(app_data_hex.as_deref())
                .unwrap_or_else(|err| {
                    log::warn!(
                        "[daemon] failed to decode propagation enabled from app_data: {err}"
                    );
                    None
                });
        let peering_timebase =
            parse_propagation_timebase_from_app_data_hex(app_data_hex.as_deref())
                .unwrap_or_else(|err| {
                    log::warn!(
                        "[daemon] failed to decode peering timebase from app_data: {err}"
                    );
                    None
                });
        let metadata =
            parse_propagation_metadata_from_app_data_hex(app_data_hex.as_deref())
                .unwrap_or_else(|err| {
                    log::warn!(
                        "[daemon] failed to decode propagation metadata from app_data: {err}"
                    );
                    JsonValue::Null
                });
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
            let record = match self.upsert_peer_with_metadata(PeerUpsertRequest {
                peer: peer.clone(),
                timestamp,
                capabilities: capability_list.clone(),
                name: name.clone(),
                name_source: name_source.clone(),
                metadata: Some(metadata.clone()),
                peer_type,
            }) {
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
                    .transient_peer_record_with_state(
                        peer,
                        timestamp,
                        capability_list.clone(),
                        name,
                        name_source,
                        metadata,
                        Some("discovered".to_string()),
                        propagation_peer_state,
                    ),
                Err(err) => return Err(err),
            };
            record
        } else {
            self.transient_peer_record_with_state(
                peer,
                timestamp,
                capability_list.clone(),
                name,
                name_source,
                metadata,
                peer_type,
                propagation_peer_state,
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
        self.upsert_peer_with_metadata(PeerUpsertRequest {
            peer,
            timestamp,
            capabilities,
            name,
            name_source,
            metadata: None,
            peer_type,
        })
    }
}

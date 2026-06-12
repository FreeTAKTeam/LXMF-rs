impl RpcDaemon {

    pub(super) fn upsert_peer_with_metadata(
        &self,
        request: PeerUpsertRequest,
    ) -> Result<PeerRecord, std::io::Error> {
        let PeerUpsertRequest {
            peer,
            timestamp,
            capabilities,
            name,
            name_source,
            metadata,
            peer_type,
        } = request;
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
                if let Some(metadata) = metadata.filter(|value| !value.is_null()) {
                    existing.metadata = metadata;
                }
            }
            if let Some(peer_type) = peer_type.filter(|_| is_newer || reactivating_unpeered) {
                existing.peer_type = Some(peer_type);
            }
            if reactivating_unpeered {
                existing.restored_handled_ids.clear();
                existing.restored_unhandled_ids.clear();
                existing.sync_backoff = 0;
                existing.next_sync_attempt = 0;
            }
            let record = existing.clone();
            let reactivated_peer_key = reactivating_unpeered.then(|| existing.peer.clone());
            let peer_count = Self::active_peer_count_from_guard(&guard);
            drop(guard);
            if let Some(peer) = reactivated_peer_key {
                self.store
                    .clear_peer_propagation_marks(peer.as_str())
                    .map_err(std::io::Error::other)?;
            }
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
            metadata: metadata.unwrap_or(JsonValue::Null),
            peer_type,
            alive: true,
            last_sync_attempt: 0,
            next_sync_attempt: 0,
            sync_backoff: 0,
            sync_schedule_reason: None,
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
        let existing_peer = self
            .peers
            .lock()
            .expect("peers mutex poisoned")
            .values()
            .find(|record| record.peer.eq_ignore_ascii_case(peer))
            .map(|record| (record.peer.clone(), record.peer_type.clone()));
        let peer_key = existing_peer
            .as_ref()
            .map(|(peer, _)| peer.clone())
            .unwrap_or_else(|| peer.to_string());
        let existing_peer_type =
            existing_peer.as_ref().and_then(|(_, peer_type)| peer_type.clone());
        let peer_type = if self.is_static_peer(peer_key.as_str()) {
            Some("static".to_string())
        } else if existing_peer_type.as_deref() == Some("unpeered") {
            Some("manual".to_string())
        } else {
            existing_peer_type.or(Some("manual".to_string()))
        };
        self.upsert_peer(peer_key, timestamp, Vec::new(), None, None, peer_type)
    }

    pub(super) fn activate_static_peers(
        &self,
        static_peers: &[String],
    ) -> Result<(), std::io::Error> {
        let configured_static_peers = Self::normalize_static_peers(static_peers);
        let from_static_only =
            self.propagation_state.lock().expect("propagation mutex poisoned").from_static_only;
        let mut removed_static_peers = Vec::new();
        let mut reactivated_unpeered_static_peers = Vec::new();
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        for existing in guard.values_mut() {
            let is_configured_static = configured_static_peers
                .iter()
                .any(|peer| peer.eq_ignore_ascii_case(existing.peer.as_str()));
            if is_configured_static {
                if existing.peer_type.as_deref() == Some("unpeered") {
                    existing.restored_handled_ids.clear();
                    existing.restored_unhandled_ids.clear();
                    existing.sync_backoff = 0;
                    existing.next_sync_attempt = 0;
                    reactivated_unpeered_static_peers.push(existing.peer.clone());
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
                    metadata: JsonValue::Null,
                    peer_type: Some("static".to_string()),
                    alive: false,
                    last_sync_attempt: 0,
                    next_sync_attempt: 0,
                    sync_backoff: 0,
                    sync_schedule_reason: None,
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
        for peer in reactivated_unpeered_static_peers {
            self.store
                .clear_peer_propagation_marks(peer.as_str())
                .map_err(std::io::Error::other)?;
        }
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
}

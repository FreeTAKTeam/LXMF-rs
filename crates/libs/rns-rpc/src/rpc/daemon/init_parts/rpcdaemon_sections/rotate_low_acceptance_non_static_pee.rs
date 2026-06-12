impl RpcDaemon {

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
            self.restore_peer_record_queue_marks(&record)?;
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
            self.restore_peer_record_queue_marks(&record)?;
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
            let selected = peer_pool.into_iter().nth(selected_index).map(|record| record.peer);
            self.claim_peer_for_maintenance_sync(selected.as_deref(), timestamp);
            return Ok(selected);
        }

        if !unresponsive.is_empty() {
            unresponsive.sort_by(|left, right| left.peer.cmp(&right.peer));
            let selected_index = timestamp.rem_euclid(unresponsive.len() as i64) as usize;
            let selected = unresponsive.into_iter().nth(selected_index).map(|record| record.peer);
            self.claim_peer_for_maintenance_sync(selected.as_deref(), timestamp);
            return Ok(selected);
        }
        Ok(None)
    }

    fn claim_peer_for_maintenance_sync(&self, peer: Option<&str>, timestamp: i64) {
        let Some(peer) = peer else {
            return;
        };
        let mut peers = self.peers.lock().expect("peers mutex poisoned");
        let Some(record) = peers.values_mut().find(|record| record.peer.eq_ignore_ascii_case(peer))
        else {
            return;
        };
        record.last_sync_attempt = timestamp;
        record.sync_backoff = record.sync_backoff.saturating_add(LXMF_PEER_SYNC_BACKOFF_STEP_SECS);
        record.next_sync_attempt = timestamp.saturating_add(i64::from(record.sync_backoff));
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

    #[cfg(test)]
    pub(super) fn transient_peer_record(
        &self,
        peer: String,
        timestamp: i64,
        capabilities: Vec<String>,
        name: Option<String>,
        name_source: Option<String>,
        peer_type: Option<String>,
    ) -> PeerRecord {
        self.transient_peer_record_with_state(
            peer,
            timestamp,
            capabilities,
            name,
            name_source,
            JsonValue::Null,
            peer_type,
            PeerPropagationState {
                transfer_limit: None,
                sync_limit: None,
                stamp_cost: None,
                stamp_cost_flexibility: None,
                peering_cost: None,
                network_distance: None,
                peering_timebase: None,
            },
        )
    }
}

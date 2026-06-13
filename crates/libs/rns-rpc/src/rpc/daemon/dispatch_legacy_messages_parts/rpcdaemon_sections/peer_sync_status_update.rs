struct PeerSyncStatusUpdate {
    acceptance_rate: f64,
    last_sync_attempt: i64,
    next_sync_attempt: i64,
    sync_backoff: u32,
    sync_transfer_rate: f64,
    tx_bytes: u64,
    alive: bool,
    last_heard: i64,
    seen_count: u64,
}

impl RpcDaemon {
    #[allow(clippy::too_many_arguments)]
    fn update_peer_sync_status(
        &self,
        record: &PeerRecord,
        wanted_ids: Option<&PeerSyncWantedIds>,
        prior_peer_seen: Option<(i64, u64)>,
        timestamp: i64,
        propagation_handled: usize,
        propagation_transferred: usize,
        propagation_skipped: usize,
        propagation_rejected: usize,
        propagation_transfer_limited: usize,
        propagation_resource_bytes: u64,
        propagation_last_resource_bytes: u64,
    ) -> PeerSyncStatusUpdate {
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        if let Some(existing) = guard.get_mut(&record.peer) {
            let propagation_offered = propagation_handled;
            let propagation_pending = propagation_skipped;
            let propagation_completed = propagation_handled > 0
                || propagation_rejected > 0
                || propagation_transfer_limited > 0
                || propagation_skipped > 0;
            let propagation_no_work = !propagation_completed && propagation_pending == 0;
            let propagation_no_transfer_offer_response = wanted_ids
                .is_some_and(PeerSyncWantedIds::wants_none)
                && propagation_transferred == 0
                && propagation_handled > 0;
            let had_prior_peer_activity = existing.last_sync_attempt > 0
                || existing.offered > 0
                || existing.outgoing > 0
                || existing.incoming > 0
                || existing.rx_bytes > 0
                || existing.tx_bytes > 0
                || existing.sync_transfer_rate > 0.0
                || existing.acceptance_rate > 0.0;
            let was_alive = existing.alive;
            existing.last_sync_attempt = timestamp;
            if propagation_no_transfer_offer_response {
                if let Some((last_seen, seen_count)) = prior_peer_seen {
                    existing.last_seen = last_seen;
                    existing.seen_count = seen_count;
                }
            }
            existing.alive = if (propagation_no_work
                && existing.sync_backoff == 0
                && had_prior_peer_activity)
                || propagation_no_transfer_offer_response
            {
                was_alive
            } else {
                propagation_completed || existing.last_sync_attempt < existing.last_seen
            };
            existing.tx_bytes = existing.tx_bytes.saturating_add(propagation_resource_bytes);
            if propagation_transferred > 0 {
                existing.sync_transfer_rate = propagation_last_resource_bytes as f64;
            }
            if propagation_offered > 0 {
                existing.offered = existing.offered.saturating_add(propagation_offered as u64);
                existing.outgoing =
                    existing.outgoing.saturating_add(propagation_transferred as u64);
                existing.acceptance_rate = if existing.offered == 0 {
                    0.0
                } else {
                    (existing.outgoing as f64 / existing.offered as f64).max(0.0)
                };
            }
            if propagation_completed {
                existing.sync_backoff = 0;
                existing.next_sync_attempt = 0;
            } else if propagation_pending > 0 {
                existing.sync_backoff =
                    existing.sync_backoff.saturating_add(LXMF_PEER_SYNC_BACKOFF_STEP_SECS);
                existing.next_sync_attempt =
                    timestamp.saturating_add(i64::from(existing.sync_backoff));
            }
            return PeerSyncStatusUpdate {
                acceptance_rate: existing.acceptance_rate,
                last_sync_attempt: existing.last_sync_attempt,
                next_sync_attempt: existing.next_sync_attempt,
                sync_backoff: existing.sync_backoff,
                sync_transfer_rate: existing.sync_transfer_rate,
                tx_bytes: existing.tx_bytes,
                alive: existing.alive,
                last_heard: existing.last_seen,
                seen_count: existing.seen_count,
            };
        }

        PeerSyncStatusUpdate {
            acceptance_rate: record.acceptance_rate,
            last_sync_attempt: record.last_sync_attempt,
            next_sync_attempt: record.next_sync_attempt,
            sync_backoff: record.sync_backoff,
            sync_transfer_rate: record.sync_transfer_rate,
            tx_bytes: record.tx_bytes,
            alive: record.alive,
            last_heard: record.last_seen,
            seen_count: record.seen_count,
        }
    }
}

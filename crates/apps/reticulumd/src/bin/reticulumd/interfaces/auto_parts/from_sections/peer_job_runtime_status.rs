impl AutoRuntimeStatusHandle {
    #[allow(dead_code)]
    pub(crate) fn record_carrier_events(&self, events: &[AutoMulticastCarrierEvent]) -> bool {
        let mut guard = self.inner.lock().expect("auto runtime status mutex poisoned");
        if !guard.state.record_carrier_events(events) {
            return false;
        }
        guard.carrier_events = events.to_vec();
        true
    }

    pub(crate) fn record_peer_job_summary(&self, summary: &AutoPeerJobRuntimeSummary) -> bool {
        let mut guard = self.inner.lock().expect("auto runtime status mutex poisoned");
        let changed = guard.state.record_carrier_events(&summary.carrier_events);
        if changed {
            guard.carrier_events = summary.carrier_events.clone();
        }
        guard.last_peer_job = Some(summary.clone());
        changed
    }
}

fn peer_job_summary_json(summary: &AutoPeerJobRuntimeSummary) -> JsonValue {
    json!({
        "expired_peer_count": summary.expired_peer_count,
        "reverse_peer_announce_count": summary.reverse_peer_announce_count,
        "missing_initial_echo_count": summary.missing_initial_echo_count,
        "carrier_changed": summary.carrier_changed,
        "carrier_event_count": summary.carrier_event_count,
        "carrier_events": summary
            .carrier_events
            .iter()
            .map(carrier_event_json)
            .collect::<Vec<_>>(),
        "peer_count_after": summary.peer_count_after,
    })
}

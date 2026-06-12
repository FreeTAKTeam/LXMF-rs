impl RpcDaemon {

    #[allow(clippy::too_many_arguments)]
    pub(super) fn transient_peer_record_with_state(
        &self,
        peer: String,
        timestamp: i64,
        capabilities: Vec<String>,
        name: Option<String>,
        name_source: Option<String>,
        metadata: JsonValue,
        peer_type: Option<String>,
        state: PeerPropagationState,
    ) -> PeerRecord {
        let peering_timebase = state.peering_timebase.unwrap_or(timestamp);
        PeerRecord {
            peer,
            last_seen: timestamp,
            capabilities: normalize_capabilities(capabilities),
            name: clean_optional_text(name),
            name_source: clean_optional_text(name_source),
            metadata,
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
            peering_timebase,
            sync_strategy: 2,
            propagation_transfer_limit: state.transfer_limit,
            propagation_sync_limit: state.sync_limit.or(state.transfer_limit),
            propagation_stamp_cost: state.stamp_cost,
            propagation_stamp_cost_flexibility: state.stamp_cost_flexibility,
            peering_cost: state.peering_cost,
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

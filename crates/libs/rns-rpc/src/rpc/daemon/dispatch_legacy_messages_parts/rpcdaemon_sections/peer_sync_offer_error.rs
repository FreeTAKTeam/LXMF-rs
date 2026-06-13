impl RpcDaemon {
    fn peer_sync_offer_error_response(
        &self,
        request_id: u64,
        record: &PeerRecord,
        offer_error: u8,
        timestamp: i64,
        requested_transfer_limit_bytes: Option<usize>,
    ) -> Result<RpcResponse, std::io::Error> {
        match offer_error {
            LXMF_PEER_ERROR_NO_ACCESS => {
                let cleanup = self.unpeer_local_state(record.peer.as_str())?;
                let offered = cleanup.messages["offered"].as_u64().unwrap_or(0);
                let outgoing = cleanup.messages["outgoing"].as_u64().unwrap_or(0);
                let incoming = cleanup.messages["incoming"].as_u64().unwrap_or(0);
                let payload = json!({
                    "peer": cleanup.peer.as_str(),
                    "reason": "access_denied",
                    "offer_response": offer_error,
                    "unpeered": true,
                    "removed": cleanup.removed,
                    "propagation_cleared": cleanup.propagation_cleared,
                    "propagation_cleared_bytes": cleanup.propagation_cleared_bytes,
                    "offered": offered,
                    "outgoing": outgoing,
                    "incoming": incoming,
                    "messages": cleanup.messages,
                });
                self.publish_event(RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: payload.clone(),
                });
                Ok(RpcResponse {
                    id: request_id,
                    result: Some(payload),
                    error: None,
                })
            }
            LXMF_PEER_ERROR_THROTTLED => {
                self.restore_peer_record_queue_marks(record)?;
                let (transfer_limit_bytes, sync_limit_bytes) =
                    peer_sync_limits(record, requested_transfer_limit_bytes);
                {
                    let mut peers = self.peers.lock().expect("peers mutex poisoned");
                    if let Some(peer) = peers.get_mut(record.peer.as_str()) {
                        peer.next_sync_attempt =
                            timestamp.saturating_add(PN_STAMP_THROTTLE_SECS);
                    }
                }
                Ok(self.postponed_peer_sync_response(
                    request_id,
                    record,
                    timestamp,
                    "throttled",
                    transfer_limit_bytes,
                    sync_limit_bytes,
                ))
            }
            _ => {
                self.restore_peer_record_queue_marks(record)?;
                let (transfer_limit_bytes, sync_limit_bytes) =
                    peer_sync_limits(record, requested_transfer_limit_bytes);
                {
                    let mut peers = self.peers.lock().expect("peers mutex poisoned");
                    if let Some(peer) = peers.get_mut(record.peer.as_str()) {
                        peer.last_sync_attempt = timestamp;
                        peer.next_sync_attempt = 0;
                    }
                }
                Ok(self.local_peer_offer_error_response(
                    request_id,
                    record,
                    timestamp,
                    local_retryable_peer_offer_error_reason(offer_error),
                    offer_error,
                    (transfer_limit_bytes, sync_limit_bytes),
                ))
            }
        }
    }
}

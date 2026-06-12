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

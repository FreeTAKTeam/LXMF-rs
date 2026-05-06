use super::*;
use std::time::SystemTime;

pub(super) fn compose_python_status(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
) -> Value {
    let status = daemon
        .handle_rpc(RpcRequest { id: 0, method: "daemon_status_ex".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .unwrap_or_else(|| json!({}));
    let peers = daemon
        .handle_rpc(RpcRequest { id: 0, method: "list_peers".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .unwrap_or_else(|| json!({ "peers": [] }));
    let propagation = status.get("propagation").cloned().unwrap_or_else(|| json!({}));
    let (message_count, message_bytes) = daemon.message_storage_stats().unwrap_or((0, 0));
    let static_peer_count = propagation
        .get("static_peers")
        .and_then(Value::as_array)
        .map(|rows| rows.len())
        .unwrap_or(0);
    let mut discovered_peer_count = 0_u64;
    let mut total_peer_count = 0_u64;
    let peer_map = peers
        .get("peers")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let peer = row.get("peer")?.as_str()?.to_string();
                    let peer_type =
                        row.get("peer_type").and_then(Value::as_str).unwrap_or("discovered");
                    let (outgoing, incoming, offered, unhandled) =
                        daemon.peer_message_stats(peer.as_str()).unwrap_or((0, 0, 0, 0));
                    total_peer_count = total_peer_count.saturating_add(1);
                    if matches!(peer_type, "discovered" | "auto") {
                        discovered_peer_count = discovered_peer_count.saturating_add(1);
                    }
                    Some((
                        peer,
                        json!({
                            "type": peer_type,
                            "state": 0,
                            "alive": row.get("alive").and_then(Value::as_bool).unwrap_or(true),
                            "name": row.get("name").cloned().unwrap_or(Value::Null),
                            "last_heard": row.get("last_seen").and_then(Value::as_i64).unwrap_or(0),
                            "next_sync_attempt": row.get("next_sync_attempt").and_then(Value::as_i64).unwrap_or(0),
                            "last_sync_attempt": row.get("last_sync_attempt").and_then(Value::as_i64).unwrap_or(0),
                            "sync_backoff": row.get("sync_backoff").and_then(Value::as_u64).unwrap_or(0),
                            "peering_timebase": 0,
                            "ler": 0,
                            "str": 0,
                            "transfer_limit": 256,
                            "sync_limit": 10240,
                            "target_stamp_cost": propagation.get("target_cost").and_then(Value::as_u64).unwrap_or(16),
                            "stamp_cost_flexibility": propagation.get("stamp_cost_flexibility").and_then(Value::as_u64).unwrap_or(3),
                            "peering_cost": propagation.get("peering_cost").and_then(Value::as_u64).unwrap_or(18),
                            "peering_key": Value::Null,
                            "network_distance": row.get("network_distance").and_then(Value::as_u64).unwrap_or(1),
                            "rx_bytes": row.get("rx_bytes").and_then(Value::as_u64).unwrap_or(0),
                            "tx_bytes": row.get("tx_bytes").and_then(Value::as_u64).unwrap_or(0),
                            "acceptance_rate": row.get("acceptance_rate").and_then(Value::as_f64).unwrap_or(1.0),
                            "messages": {
                                "offered": offered,
                                "outgoing": outgoing,
                                "incoming": incoming,
                                "unhandled": unhandled
                            }
                        }),
                    ))
                })
                .collect::<serde_json::Map<String, Value>>()
        })
        .unwrap_or_default();
    json!({
        "identity_hash": status.get("identity_hash").cloned().unwrap_or(Value::Null),
        "destination_hash": control.propagation_destination_hash_hex.clone().unwrap_or_default(),
        "uptime": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs(),
        "delivery_limit": 1000,
        "propagation_limit": 256,
        "sync_limit": 10240,
        "target_stamp_cost": propagation.get("target_cost").and_then(Value::as_u64).unwrap_or(16),
        "stamp_cost_flexibility": propagation.get("stamp_cost_flexibility").and_then(Value::as_u64).unwrap_or(3),
        "peering_cost": propagation.get("peering_cost").and_then(Value::as_u64).unwrap_or(18),
        "max_peering_cost": propagation.get("remote_peering_cost_max").and_then(Value::as_u64).unwrap_or(26),
        "autopeer_maxdepth": propagation.get("autopeer_maxdepth").and_then(Value::as_u64).unwrap_or(6),
        "from_static_only": propagation.get("from_static_only").and_then(Value::as_bool).unwrap_or(false),
        "messagestore": {
            "count": message_count,
            "bytes": message_bytes,
            "limit": propagation.get("message_storage_limit_mb").and_then(Value::as_u64).map(|value| value * 1024 * 1024),
        },
        "clients": {
            "client_propagation_messages_received": propagation.get("client_propagation_messages_received").and_then(Value::as_u64).unwrap_or(0),
            "client_propagation_messages_served": propagation.get("client_propagation_messages_served").and_then(Value::as_u64).unwrap_or(0),
        },
        "unpeered_propagation_incoming": propagation.get("unpeered_propagation_incoming").and_then(Value::as_u64).unwrap_or(0),
        "unpeered_propagation_rx_bytes": propagation.get("unpeered_propagation_rx_bytes").and_then(Value::as_u64).unwrap_or(0),
        "static_peers": static_peer_count,
        "discovered_peers": discovered_peer_count,
        "total_peers": total_peer_count,
        "max_peers": propagation.get("max_peers").and_then(Value::as_u64).unwrap_or(20),
        "peers": peer_map,
    })
}

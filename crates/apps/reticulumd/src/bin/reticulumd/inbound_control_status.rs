use super::*;
use serde_json::Number;

pub(super) fn compose_python_status(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
) -> Value {
    let status = rpc_result_or_default(daemon, "daemon_status_ex", json!({}));
    let peers = rpc_result_or_default(daemon, "list_peers", json!({ "peers": [] }));
    let propagation = status.get("propagation").cloned().unwrap_or_else(|| json!({}));
    let (message_count, message_bytes) = match daemon.message_storage_stats() {
        Ok(stats) => stats,
        Err(error) => {
            log::error!("[daemon-control] failed to read message storage stats: {error}");
            (0, 0)
        }
    };
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
                    let peer_type = if propagation
                        .get("static_peers")
                        .and_then(Value::as_array)
                        .is_some_and(|static_peers| {
                            static_peers
                                .iter()
                                .filter_map(Value::as_str)
                                .any(|static_peer| static_peer.eq_ignore_ascii_case(peer.as_str()))
                        })
                    {
                        "static"
                    } else {
                        "discovered"
                    };
                    let (
                        outgoing,
                        incoming,
                        offered,
                        unhandled,
                        offered_bytes,
                        unhandled_bytes,
                    ) = match daemon.peer_message_stats(peer.as_str()) {
                        Ok(stats) => stats,
                        Err(error) => {
                            log::error!(
                                "[daemon-control] failed to read peer message stats peer={peer}: {error}"
                            );
                            (0, 0, 0, 0, 0, 0)
                        }
                    };
                    total_peer_count = total_peer_count.saturating_add(1);
                    if peer_type == "discovered" {
                        discovered_peer_count = discovered_peer_count.saturating_add(1);
                    }
                    let target_stamp_cost =
                        row.get("propagation_stamp_cost").cloned().unwrap_or(Value::Null);
                    let stamp_cost_flexibility = row
                        .get("propagation_stamp_cost_flexibility")
                        .cloned()
                        .unwrap_or(Value::Null);
                    let transfer_limit = row
                        .get("propagation_transfer_limit")
                        .filter(|value| !value.is_null())
                        .or_else(|| row.get("transfer_limit"))
                        .map(python_transfer_limit_kilobytes)
                        .unwrap_or(Value::Null);
                    let sync_limit = row
                        .get("propagation_sync_limit")
                        .filter(|value| !value.is_null())
                        .or_else(|| row.get("sync_limit"))
                        .map(python_sync_limit_kilobytes)
                        .unwrap_or(Value::Null);
                    let sync_transfer_rate =
                        row.get("sync_transfer_rate").and_then(Value::as_f64).unwrap_or(0.0);
                    let handled_ids = row.get("handled_ids").cloned().unwrap_or_else(|| json!([]));
                    let unhandled_ids =
                        row.get("unhandled_ids").cloned().unwrap_or_else(|| json!([]));
                    let internal_peer_type =
                        row.get("peer_type").cloned().unwrap_or(Value::Null);
                    let name_source = row.get("name_source").cloned().unwrap_or(Value::Null);
                    let first_seen = row.get("first_seen").and_then(Value::as_i64).unwrap_or(0);
                    let seen_count = row.get("seen_count").and_then(Value::as_u64).unwrap_or(0);
                    let sync_strategy =
                        row.get("sync_strategy").and_then(Value::as_u64).unwrap_or(2);
                    let messages = json!({
                        "offered": offered,
                        "outgoing": outgoing,
                        "incoming": incoming,
                        "unhandled": unhandled,
                        "offered_bytes": offered_bytes,
                        "unhandled_bytes": unhandled_bytes,
                        "handled_ids": handled_ids.clone(),
                        "unhandled_ids": unhandled_ids.clone()
                    });
                    Some((
                        peer,
                        json!({
                            "type": peer_type,
                            "peer_type": internal_peer_type,
                            "state": 0,
                            "sync_strategy": sync_strategy,
                            "alive": row.get("alive").and_then(Value::as_bool).unwrap_or(true),
                            "name": row.get("name").cloned().unwrap_or(Value::Null),
                            "name_source": name_source,
                            "first_seen": first_seen,
                            "seen_count": seen_count,
                            "last_heard": row.get("last_seen").and_then(Value::as_i64).unwrap_or(0),
                            "next_sync_attempt": row.get("next_sync_attempt").and_then(Value::as_i64).unwrap_or(0),
                            "last_sync_attempt": row.get("last_sync_attempt").and_then(Value::as_i64).unwrap_or(0),
                            "sync_backoff": row.get("sync_backoff").and_then(Value::as_u64).unwrap_or(0),
                            "peering_timebase": row.get("peering_timebase").and_then(Value::as_i64).unwrap_or(0),
                            "ler": 0,
                            "str": sync_transfer_rate as u64,
                            "sync_transfer_rate": sync_transfer_rate,
                            "transfer_limit": transfer_limit,
                            "sync_limit": sync_limit,
                            "target_stamp_cost": target_stamp_cost,
                            "stamp_cost_flexibility": stamp_cost_flexibility,
                            "peering_cost": row.get("peering_cost").cloned().unwrap_or(Value::Null),
                            "peering_key": row.get("peering_key").cloned().unwrap_or(Value::Null),
                            "peering_key_status": row.get("peering_key_status").cloned().unwrap_or(Value::Null),
                            "network_distance": row.get("network_distance").and_then(Value::as_u64).unwrap_or(1),
                            "rx_bytes": row.get("rx_bytes").and_then(Value::as_u64).unwrap_or(0),
                            "tx_bytes": row.get("tx_bytes").and_then(Value::as_u64).unwrap_or(0),
                            "acceptance_rate": row.get("acceptance_rate").and_then(Value::as_f64).unwrap_or(0.0),
                            "offered": offered,
                            "outgoing": outgoing,
                            "incoming": incoming,
                            "unhandled": unhandled,
                            "offered_bytes": offered_bytes,
                            "unhandled_bytes": unhandled_bytes,
                            "handled_ids": handled_ids.clone(),
                            "unhandled_ids": unhandled_ids.clone(),
                            "messages": messages
                        }),
                    ))
                })
                .collect::<serde_json::Map<String, Value>>()
        })
        .unwrap_or_default();
    json!({
        "identity_hash": status.get("identity_hash").cloned().unwrap_or(Value::Null),
        "destination_hash": control.propagation_destination_hash_hex.clone().unwrap_or_default(),
        "uptime": daemon.uptime_secs(),
        "delivery_limit": propagation.get("delivery_limit").and_then(Value::as_u64).unwrap_or(1000),
        "propagation_limit": propagation.get("propagation_limit").and_then(Value::as_u64).unwrap_or(256),
        "sync_limit": propagation.get("sync_limit").and_then(Value::as_u64).unwrap_or(10240),
        "target_stamp_cost": propagation.get("target_cost").and_then(Value::as_u64).unwrap_or(16),
        "stamp_cost_flexibility": propagation.get("stamp_cost_flexibility").and_then(Value::as_u64).unwrap_or(3),
        "peering_cost": propagation.get("peering_cost").and_then(Value::as_u64).unwrap_or(18),
        "max_peering_cost": propagation.get("remote_peering_cost_max").and_then(Value::as_u64).unwrap_or(26),
        "autopeer": propagation.get("autopeer").and_then(Value::as_bool).unwrap_or(true),
        "autopeer_maxdepth": propagation.get("autopeer_maxdepth").and_then(Value::as_u64).unwrap_or(6),
        "from_static_only": propagation.get("from_static_only").and_then(Value::as_bool).unwrap_or(false),
        "retain_synced_on_node": propagation.get("retain_synced_on_node").and_then(Value::as_bool).unwrap_or(false),
        "total_ingested": propagation.get("total_ingested").and_then(Value::as_u64).unwrap_or(0),
        "last_ingest_count": propagation.get("last_ingest_count").and_then(Value::as_u64).unwrap_or(0),
        "messages_received": propagation.get("messages_received").and_then(Value::as_u64).unwrap_or(0),
        "max_messages": propagation.get("max_messages").and_then(Value::as_u64).unwrap_or(0),
        "selected_node": propagation.get("selected_node").cloned().unwrap_or(Value::Null),
        "sync_state": propagation.get("sync_state").and_then(Value::as_u64).unwrap_or(0),
        "state_name": propagation.get("state_name").cloned().unwrap_or(Value::Null),
        "sync_progress": propagation.get("sync_progress").and_then(Value::as_f64).unwrap_or(0.0),
        "last_sync_started": propagation.get("last_sync_started").cloned().unwrap_or(Value::Null),
        "last_sync_completed": propagation.get("last_sync_completed").cloned().unwrap_or(Value::Null),
        "last_sync_error": propagation.get("last_sync_error").cloned().unwrap_or(Value::Null),
        "messagestore": {
            "count": message_count,
            "bytes": message_bytes,
            "limit": propagation.get("message_storage_limit_mb").and_then(Value::as_u64).map(|value| value * 1_000_000),
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

fn rpc_result_or_default(daemon: &RpcDaemon, method: &str, default: Value) -> Value {
    match daemon.handle_rpc(RpcRequest { id: 0, method: method.to_string(), params: None }) {
        Ok(response) => {
            if let Some(error) = response.error {
                log::error!(
                    "[daemon-control] status RPC failed method={} code={} error={}",
                    method,
                    error.code,
                    error.message
                );
                default
            } else {
                response.result.unwrap_or_else(|| {
                    log::error!("[daemon-control] status RPC returned no result method={method}");
                    default
                })
            }
        }
        Err(error) => {
            log::error!("[daemon-control] status RPC failed method={method}: {error}");
            default
        }
    }
}

fn normalize_python_limit_value(value: Value) -> Value {
    let Some(number) = value.as_f64() else {
        return value;
    };
    if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number > u64::MAX as f64 {
        return value;
    }
    Value::Number(Number::from(number as u64))
}

fn python_transfer_limit_kilobytes(value: &Value) -> Value {
    let Some(bytes) = value.as_f64() else {
        return value.clone();
    };
    if !bytes.is_finite() || bytes < 0.0 {
        return value.clone();
    }
    let Some(kilobytes) = Number::from_f64(bytes / 1000.0) else {
        return value.clone();
    };
    normalize_python_limit_value(Value::Number(kilobytes))
}

fn python_sync_limit_kilobytes(value: &Value) -> Value {
    let Some(bytes) = value.as_u64() else {
        return value.clone();
    };
    if bytes == 0 {
        Value::Number(Number::from(0))
    } else {
        Value::Number(Number::from(bytes.saturating_add(999) / 1000))
    }
}

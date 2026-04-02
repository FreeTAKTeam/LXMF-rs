use super::*;

pub(super) fn spawn_control_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
) {
    tokio::spawn(async move {
        let mut rx = transport.in_link_events();
        let identified = Arc::new(Mutex::new(HashMap::<AddressHash, Identity>::new()));
        loop {
            let Ok(event) = rx.recv().await else {
                break;
            };
            let LinkEvent::Data(payload) = event.event else {
                continue;
            };
            let Some(control_hash) = control.control_destination_hash_hex.as_deref() else {
                continue;
            };
            if hex::encode(event.address_hash.as_slice()) != control_hash {
                continue;
            }
            match payload.context() {
                PacketContext::LinkIdentify => {
                    if let Some(identity) =
                        parse_link_identify_payload(payload.as_slice(), &event.id)
                    {
                        if let Ok(mut guard) = identified.lock() {
                            guard.insert(event.id, identity);
                        }
                    }
                }
                PacketContext::Request => {
                    let Some(request_id) = payload.request_id() else {
                        continue;
                    };
                    let remote_identity =
                        identified.lock().ok().and_then(|guard| guard.get(&event.id).cloned());
                    let response = handle_control_request(
                        daemon.as_ref(),
                        &control,
                        payload.as_slice(),
                        remote_identity.as_ref(),
                    );
                    let _ =
                        send_control_response(transport.as_ref(), &event.id, request_id, response)
                            .await;
                }
                _ => {}
            }
        }
    });
}

fn parse_link_identify_payload(payload: &[u8], link_id: &AddressHash) -> Option<Identity> {
    if payload.len() < 32 + 32 + 64 {
        return None;
    }
    let identity = Identity::new_from_slices(&payload[..32], &payload[32..64]);
    let signature = ed25519_dalek::Signature::from_slice(&payload[64..128]).ok()?;
    let mut signed = Vec::with_capacity(16 + 64);
    signed.extend_from_slice(link_id.as_slice());
    signed.extend_from_slice(&payload[..64]);
    identity.verify(&signed, &signature).ok()?;
    Some(identity)
}

fn handle_control_request(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
    payload: &[u8],
    remote_identity: Option<&Identity>,
) -> ControlResponse {
    const ERROR_NO_IDENTITY: u8 = 0xF0;
    const ERROR_NO_ACCESS: u8 = 0xF1;
    const ERROR_INVALID_DATA: u8 = 0xF4;
    const ERROR_NOT_FOUND: u8 = 0xFD;

    if remote_identity.is_none() {
        daemon.record_unpeered_propagation_attempt(payload.len());
        return ControlResponse::Code(ERROR_NO_IDENTITY);
    }
    let remote_identity = remote_identity.expect("checked above");
    let remote_hash = hex::encode(remote_identity.address_hash.as_slice());
    if !control_identity_allowed(control, &remote_hash) {
        daemon.record_unpeered_propagation_attempt(payload.len());
        return ControlResponse::Code(ERROR_NO_ACCESS);
    }

    let Some((path_hash, data)) = parse_control_request_payload(payload) else {
        return ControlResponse::Code(ERROR_INVALID_DATA);
    };
    if path_hash == control_path_hash("/pn/get/stats") {
        return ControlResponse::Value(compose_python_status(daemon, control));
    }

    let Some(peer_hex) = data.and_then(|value| match value {
        rmpv::Value::Binary(bytes) if bytes.len() == 16 => Some(hex::encode(bytes)),
        _ => None,
    }) else {
        return ControlResponse::Code(ERROR_INVALID_DATA);
    };
    let peer_exists = daemon
        .handle_rpc(RpcRequest { id: 0, method: "list_peers".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .and_then(|value| value.get("peers").cloned())
        .and_then(|value| value.as_array().cloned())
        .map(|rows| {
            rows.iter()
                .any(|row| row.get("peer").and_then(Value::as_str) == Some(peer_hex.as_str()))
        })
        .unwrap_or(false);
    if !peer_exists {
        return ControlResponse::Code(ERROR_NOT_FOUND);
    }

    if path_hash == control_path_hash("/pn/peer/sync") {
        let _ = daemon.handle_rpc(RpcRequest {
            id: 0,
            method: "peer_sync".to_string(),
            params: Some(json!({ "peer": peer_hex })),
        });
        return ControlResponse::Bool(true);
    }
    if path_hash == control_path_hash("/pn/peer/unpeer") {
        let _ = daemon.handle_rpc(RpcRequest {
            id: 0,
            method: "peer_unpeer".to_string(),
            params: Some(json!({ "peer": peer_hex })),
        });
        return ControlResponse::Bool(true);
    }

    ControlResponse::Code(ERROR_INVALID_DATA)
}

fn control_identity_allowed(control: &PropagationControlContext, remote_hash: &str) -> bool {
    if control.allowed_control_identities.is_empty() {
        return true;
    }
    control
        .allowed_control_identities
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(remote_hash))
}

fn parse_control_request_payload(payload: &[u8]) -> Option<([u8; 16], Option<rmpv::Value>)> {
    let value = rmp_serde::from_slice::<rmpv::Value>(payload).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 3 {
        return None;
    }
    let path_bytes = match entries.get(1)? {
        rmpv::Value::Binary(bytes) if bytes.len() == 16 => bytes,
        _ => return None,
    };
    let mut path_hash = [0u8; 16];
    path_hash.copy_from_slice(path_bytes.as_slice());
    Some((path_hash, entries.get(2).cloned()))
}

fn control_path_hash(path: &str) -> [u8; 16] {
    let hash = rns_transport::hash::address_hash(path.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(hash.as_slice());
    out
}

fn compose_python_status(daemon: &RpcDaemon, control: &PropagationControlContext) -> Value {
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
    let stamp_policy = status.get("stamp_policy").cloned().unwrap_or_else(|| json!({}));
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
                            "stamp_cost_flexibility": stamp_policy.get("flexibility").and_then(Value::as_u64).unwrap_or(3),
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
        "stamp_cost_flexibility": stamp_policy.get("flexibility").and_then(Value::as_u64).unwrap_or(3),
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

enum ControlResponse {
    Code(u8),
    Bool(bool),
    Value(Value),
}

async fn send_control_response(
    transport: &Transport,
    link_id: &AddressHash,
    request_id: [u8; 16],
    response: ControlResponse,
) -> Result<(), std::io::Error> {
    let Some(link) = transport.find_in_link(link_id).await else {
        return Err(std::io::Error::other("control link not found"));
    };
    let response_value = match response {
        ControlResponse::Code(code) => rmpv::Value::from(code),
        ControlResponse::Bool(value) => rmpv::Value::Boolean(value),
        ControlResponse::Value(value) => json_to_rmpv(&value),
    };
    let frame = rmpv::Value::Array(vec![rmpv::Value::Binary(request_id.to_vec()), response_value]);
    let payload = rmp_serde::to_vec(&frame).map_err(std::io::Error::other)?;
    let (packet, ingress_iface) = build_link_response_packet(&link, payload.as_slice()).await?;
    let Some(ingress_iface) = ingress_iface else {
        return Err(std::io::Error::other("control link ingress interface unavailable"));
    };
    match packet {
        LinkResponsePacket::Direct(packet) => {
            transport.send_direct(ingress_iface, *packet).await;
            Ok(())
        }
        LinkResponsePacket::Resource(payload) => transport
            .send_resource_direct(link_id, ingress_iface, payload, None)
            .await
            .map(|_| ())
            .map_err(|err| std::io::Error::other(format!("{err:?}"))),
    }
}

enum LinkResponsePacket {
    Direct(Box<Packet>),
    Resource(Vec<u8>),
}

async fn build_link_response_packet(
    link: &Arc<tokio::sync::Mutex<Link>>,
    payload: &[u8],
) -> Result<(LinkResponsePacket, Option<AddressHash>), std::io::Error> {
    let guard = link.lock().await;
    let ingress_iface = guard.ingress_iface();
    let mut packet_data = PacketDataBuffer::new();
    let cipher_len = match guard.encrypt(payload, packet_data.accuire_buf_max()) {
        Ok(ciphertext) => ciphertext.len(),
        Err(_) => return Ok((LinkResponsePacket::Resource(payload.to_vec()), ingress_iface)),
    };
    packet_data.resize(cipher_len);
    Ok((
        LinkResponsePacket::Direct(Box::new(Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination: *guard.id(),
            transport: None,
            context: PacketContext::Response,
            data: packet_data,
        })),
        ingress_iface,
    ))
}

fn json_to_rmpv(value: &Value) -> rmpv::Value {
    match value {
        Value::Null => rmpv::Value::Nil,
        Value::Bool(value) => rmpv::Value::Boolean(*value),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_u64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_f64() {
                rmpv::Value::F64(value)
            } else {
                rmpv::Value::Nil
            }
        }
        Value::String(value) => rmpv::Value::from(value.as_str()),
        Value::Array(values) => rmpv::Value::Array(values.iter().map(json_to_rmpv).collect()),
        Value::Object(map) => rmpv::Value::Map(
            map.iter()
                .map(|(key, value)| (rmpv::Value::from(key.as_str()), json_to_rmpv(value)))
                .collect(),
        ),
    }
}

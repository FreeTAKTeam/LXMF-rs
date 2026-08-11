use super::control;
use super::events;
use super::model::{address_hex, Cli, ControlRequest, SharedState, CHANNEL_MESSAGE_TYPE};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rns_transport::destination::link::{Link, LinkStatus};
use rns_transport::destination::{DestinationName, ProofStrategy, SingleInputDestination};
use rns_transport::hash::{AddressHash, Hash};
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::{IfaceRole, InterfaceMode};
use rns_transport::packet::{Packet, PacketContext, PacketDataBuffer};
use rns_transport::transport::{Transport, TransportConfig};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

pub async fn run(cli: Cli) -> Result<(), String> {
    if cli.listen.is_empty() && cli.connect.is_empty() {
        return Err("at least one --listen or --connect interface is required".to_string());
    }
    let listen_policies =
        interface_policies(cli.listen.len(), &cli.listen_mode, &cli.listen_gravity, "listen")?;
    let connect_policies =
        interface_policies(cli.connect.len(), &cli.connect_mode, &cli.connect_gravity, "connect")?;
    let seed = cli.identity_seed.as_deref().unwrap_or(&cli.name);
    let identity = PrivateIdentity::new_from_name(seed);
    let mut config = TransportConfig::new(cli.name.clone(), &identity, true);
    config.set_transport_enabled(cli.transport);
    config.set_path_request_timeout_secs(5);
    config.set_link_proof_timeout_secs(30);
    config.set_resource_retry_interval_secs(1);
    let mut transport = Transport::new(config);
    let destination =
        transport.add_destination(identity.clone(), DestinationName::new("interop", "probe")).await;
    destination.lock().await.set_proof_strategy(ProofStrategy::All);
    let destination_hash = destination.lock().await.desc.address_hash;
    let state = SharedState::new(cli.name, *identity.address_hash(), destination_hash);
    transport.set_receipt_handler(events::receipt_handler(state.clone())).await;
    let transport = Arc::new(transport);

    events::spawn(transport.clone(), state.clone());
    let manager = transport.iface_manager();
    for (listen, (mode, gravity)) in cli.listen.into_iter().zip(listen_policies) {
        let mut guard = manager.lock().await;
        let address = guard.spawn_as_with_mode(
            TcpServer::new(listen, manager.clone()),
            TcpServer::spawn,
            IfaceRole::default(),
            mode,
        );
        if !guard.set_gravity(address, gravity) {
            return Err("failed to set listening interface gravity".to_string());
        }
    }
    for (target, (mode, gravity)) in cli.connect.into_iter().zip(connect_policies) {
        let mut guard = manager.lock().await;
        let address = guard.spawn_as_with_mode(
            TcpClient::new(target),
            TcpClient::spawn,
            IfaceRole::default(),
            mode,
        );
        if !guard.set_gravity(address, gravity) {
            return Err("failed to set connecting interface gravity".to_string());
        }
    }
    control::serve(&cli.control, transport, destination, state).await
}

fn interface_policies(
    count: usize,
    modes: &[String],
    gravities: &[i64],
    label: &str,
) -> Result<Vec<(InterfaceMode, i64)>, String> {
    for (values, name) in [(modes.len(), "mode"), (gravities.len(), "gravity")] {
        if values != 0 && values != 1 && values != count {
            return Err(format!(
                "--{label}-{name} must be omitted, supplied once, or supplied once per interface"
            ));
        }
    }
    (0..count)
        .map(|index| {
            let mode = modes.get(if modes.len() == 1 { 0 } else { index }).map_or(
                Ok(InterfaceMode::default()),
                |value| {
                    InterfaceMode::parse(value)
                        .map_err(str::to_owned)
                        .map(|parsed| parsed.unwrap_or_default())
                },
            )?;
            let gravity = gravities
                .get(if gravities.len() == 1 { 0 } else { index })
                .copied()
                .unwrap_or_default();
            Ok((mode, gravity))
        })
        .collect()
}

pub async fn handle_request(
    transport: &Transport,
    destination: &Arc<Mutex<SingleInputDestination>>,
    state: &SharedState,
    request: &ControlRequest,
) -> Result<Value, String> {
    match request.method.as_str() {
        "status" => status(transport, state).await,
        "interfaces" => interfaces(transport).await,
        "set_interface_policy" => set_interface_policy(transport, &request.params).await,
        "stop_interface" => stop_interface(transport, &request.params).await,
        "announce" => announce(transport, destination, &request.params).await,
        "events" => events(state, &request.params).await,
        "has_path" => has_path(transport, &request.params).await,
        "expire_path" => expire_path(transport, &request.params).await,
        "request_path" => request_path(transport, &request.params).await,
        "send" => send_packet(transport, &request.params).await,
        "link" => create_link(transport, state, &request.params).await,
        "links" => {
            Ok(json!({"links": state.links.read().await.values().cloned().collect::<Vec<_>>() }))
        }
        "link_send" => link_send(transport, &request.params).await,
        "request" => send_request(transport, &request.params).await,
        "respond" => send_response(transport, &request.params).await,
        "channel" => channel_send(transport, &request.params).await,
        "channel_state" => channel_state(transport, &request.params).await,
        "resource" => send_resource(transport, &request.params).await,
        "prepare_resource" => super::performance::prepare_resource(state, &request.params).await,
        "send_prepared_resource" => {
            super::performance::send_prepared_resource(transport, state, &request.params).await
        }
        "cancel_resource" => cancel_resource(transport, &request.params).await,
        "close_link" => close_link(transport, &request.params).await,
        "shutdown" => Ok(json!({"shutdown": true})),
        other => Err(format!("unknown control method {other}")),
    }
}

async fn status(transport: &Transport, state: &SharedState) -> Result<Value, String> {
    Ok(json!({
        "name": state.name,
        "identity_hash": address_hex(&state.identity_hash),
        "destination_hash": address_hex(&state.destination_hash),
        "known_destinations": state.known_destinations.read().await.len(),
        "link_count": transport.link_count().await,
    }))
}

async fn interfaces(transport: &Transport) -> Result<Value, String> {
    let manager = transport.iface_manager();
    let guard = manager.lock().await;
    let mut rows = guard
        .interface_hashes()
        .into_iter()
        .map(|address| {
            json!({
                "address": address_hex(&address),
                "mode": guard.mode(&address).map(InterfaceMode::as_str),
                "gravity": guard.gravity(&address),
                "outgoing": guard.outgoing(&address),
                "role": guard.role(&address).map(|role| format!("{role:?}")),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left["address"].as_str().cmp(&right["address"].as_str()));
    Ok(json!({"interfaces": rows}))
}

async fn set_interface_policy(transport: &Transport, params: &Value) -> Result<Value, String> {
    let address = address_param(params, "interface")?;
    let mode = params.get("mode").and_then(Value::as_str);
    let gravity = params.get("gravity").and_then(Value::as_i64);
    if mode.is_none() && gravity.is_none() {
        return Err("set_interface_policy requires mode or gravity".to_string());
    }
    let manager = transport.iface_manager();
    let mut guard = manager.lock().await;
    if let Some(value) = mode {
        let parsed = InterfaceMode::parse(value)
            .map_err(str::to_owned)?
            .ok_or_else(|| "mode cannot be empty".to_string())?;
        if !guard.set_mode(address, parsed) {
            return Err(format!("unknown interface {}", address_hex(&address)));
        }
    }
    if let Some(value) = gravity {
        if !guard.set_gravity(address, value) {
            return Err(format!("unknown interface {}", address_hex(&address)));
        }
    }
    Ok(json!({
        "interface": address_hex(&address),
        "mode": guard.mode(&address).map(InterfaceMode::as_str),
        "gravity": guard.gravity(&address),
    }))
}

async fn stop_interface(transport: &Transport, params: &Value) -> Result<Value, String> {
    let address = address_param(params, "interface")?;
    let stopped = transport.iface_manager().lock().await.stop_interface(address);
    Ok(json!({"interface": address_hex(&address), "stopped": stopped}))
}

async fn expire_path(transport: &Transport, params: &Value) -> Result<Value, String> {
    let destination_hash = address_param(params, "destination_hash")?;
    Ok(json!({"expired": transport.expire_path(&destination_hash).await}))
}

async fn announce(
    transport: &Transport,
    destination: &Arc<Mutex<SingleInputDestination>>,
    params: &Value,
) -> Result<Value, String> {
    let app_data = optional_bytes(params, "app_data")?;
    transport.send_announce(destination, app_data.as_deref()).await;
    Ok(json!({"announced": true}))
}

async fn events(state: &SharedState, params: &Value) -> Result<Value, String> {
    let clear = params.get("clear").and_then(Value::as_bool).unwrap_or(false);
    let mut events = state.events.lock().await;
    let snapshot = events.clone();
    if clear {
        events.clear();
    }
    Ok(json!({"events": snapshot}))
}

async fn has_path(transport: &Transport, params: &Value) -> Result<Value, String> {
    let destination = address_param(params, "destination_hash")?;
    let status = transport.path_status(&destination).await;
    Ok(json!({
        "path_found": status.path_found,
        "hops": status.hops,
        "next_hop": status.next_hop.map(|value| address_hex(&value)),
        "interface": status.interface.map(|value| address_hex(&value)),
    }))
}

async fn request_path(transport: &Transport, params: &Value) -> Result<Value, String> {
    let destination = address_param(params, "destination_hash")?;
    let dispatch = transport.request_path(&destination, None, None).await;
    Ok(json!({"dispatch": format!("{dispatch:?}")}))
}

async fn send_packet(transport: &Transport, params: &Value) -> Result<Value, String> {
    let destination = address_param(params, "destination_hash")?;
    let data = required_bytes(params, "data")?;
    let packet =
        Packet { destination, data: PacketDataBuffer::new_from_slice(&data), ..Packet::default() };
    let trace = transport.send_packet_with_trace(packet).await;
    Ok(json!({
        "outcome": format!("{:?}", trace.outcome),
        "packet_hash": trace.packet_hash.map(|value| hex::encode(value.as_slice())),
    }))
}

async fn create_link(
    transport: &Transport,
    state: &SharedState,
    params: &Value,
) -> Result<Value, String> {
    let destination_hash = address_param(params, "destination_hash")?;
    let destination = state
        .known_destinations
        .read()
        .await
        .get(&destination_hash)
        .copied()
        .ok_or_else(|| format!("destination {destination_hash} has not been announced"))?;
    let link = transport.link(destination).await;
    let link = link.lock().await;
    Ok(json!({"link_id": address_hex(link.id()), "state": link_status(link.status())}))
}

async fn link_send(transport: &Transport, params: &Value) -> Result<Value, String> {
    let link_id = address_param(params, "link_id")?;
    let data = required_bytes(params, "data")?;
    let context = params.get("context").and_then(Value::as_u64).unwrap_or(0);
    let link = find_link(transport, &link_id).await?;
    let packet = match context {
        0 => link.lock().await.data_packet(&data),
        11 => link.lock().await.request_packet(&data),
        12 => link.lock().await.response_packet(&data),
        value => return Err(format!("unsupported link packet context {value}")),
    }
    .map_err(|error| format!("build link packet: {error:?}"))?;
    let outcome = transport.send_link_packet_on_bound_iface(&link, packet).await;
    Ok(json!({"outcome": format!("{outcome:?}")}))
}

async fn send_request(transport: &Transport, params: &Value) -> Result<Value, String> {
    let link_id = address_param(params, "link_id")?;
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing string parameter path".to_string())?;
    let application_data = required_bytes(params, "data")?;
    let data_value = decode_msgpack_value(&application_data)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system time before epoch: {error}"))?
        .as_secs_f64();
    let path_hash = Sha256::digest(path.as_bytes());
    let envelope = encode_msgpack_value(&rmpv::Value::Array(vec![
        rmpv::Value::F64(timestamp),
        rmpv::Value::Binary(path_hash[..16].to_vec()),
        data_value,
    ]))?;
    let link = find_link(transport, &link_id).await?;
    let packet = link
        .lock()
        .await
        .request_packet(&envelope)
        .map_err(|error| format!("build request packet: {error:?}"))?;
    let packet_hash = packet.hash();
    let request_id = hex::encode(&packet_hash.as_slice()[..16]);
    let outcome = transport.send_link_packet_on_bound_iface(&link, packet).await;
    Ok(json!({"outcome": format!("{outcome:?}"), "request_id": request_id}))
}

async fn send_response(transport: &Transport, params: &Value) -> Result<Value, String> {
    let link_id = address_param(params, "link_id")?;
    let request_id = required_hex_bytes(params, "request_id", 16)?;
    let application_data = required_bytes(params, "data")?;
    let data_value = decode_msgpack_value(&application_data)?;
    let envelope = encode_msgpack_value(&rmpv::Value::Array(vec![
        rmpv::Value::Binary(request_id),
        data_value,
    ]))?;
    let link = find_link(transport, &link_id).await?;
    let packet = link
        .lock()
        .await
        .response_packet(&envelope)
        .map_err(|error| format!("build response packet: {error:?}"))?;
    let outcome = transport.send_link_packet_on_bound_iface(&link, packet).await;
    Ok(json!({"outcome": format!("{outcome:?}")}))
}

async fn channel_send(transport: &Transport, params: &Value) -> Result<Value, String> {
    let link_id = address_param(params, "link_id")?;
    let payload = required_bytes(params, "payload")?;
    let sequence = transport
        .channel(link_id)
        .send(CHANNEL_MESSAGE_TYPE, payload)
        .await
        .map_err(|error| format!("send channel message: {error:?}"))?;
    Ok(json!({"sequence": sequence, "message_type": CHANNEL_MESSAGE_TYPE}))
}

async fn channel_state(transport: &Transport, params: &Value) -> Result<Value, String> {
    let link_id = address_param(params, "link_id")?;
    let sequence = params
        .get("sequence")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| "sequence must be a u16".to_string())?;
    let state = transport
        .channel_message_state(&link_id, sequence)
        .await
        .map_err(|error| format!("read channel message state: {error:?}"))?;
    Ok(json!({"sequence": sequence, "state": format!("{state:?}").to_lowercase()}))
}

async fn send_resource(transport: &Transport, params: &Value) -> Result<Value, String> {
    let link_id = address_param(params, "link_id")?;
    let data = required_bytes(params, "data")?;
    let metadata = optional_bytes(params, "metadata")?;
    let hash = transport
        .send_resource(&link_id, data, metadata)
        .await
        .map_err(|error| format!("send resource: {error:?}"))?;
    Ok(json!({"resource_hash": hex::encode(hash.as_slice())}))
}

async fn cancel_resource(transport: &Transport, params: &Value) -> Result<Value, String> {
    let link_id = address_param(params, "link_id")?;
    let resource_hash = required_hex_bytes(params, "resource_hash", 32)?;
    let resource_hash = Hash::new_from_slice(&resource_hash);
    let cancelled = transport
        .cancel_resource(&link_id, resource_hash)
        .await
        .map_err(|error| format!("cancel resource: {error:?}"))?;
    Ok(json!({"cancelled": cancelled}))
}

async fn close_link(transport: &Transport, params: &Value) -> Result<Value, String> {
    let link_id = address_param(params, "link_id")?;
    let link = find_link(transport, &link_id).await?;
    let packet = link.lock().await.teardown();
    let outcome = if let Some(packet) = packet {
        format!("{:?}", transport.send_link_packet_on_bound_iface(&link, packet).await)
    } else {
        "already_closed".to_string()
    };
    Ok(json!({"outcome": outcome}))
}

async fn find_link(
    transport: &Transport,
    link_id: &AddressHash,
) -> Result<Arc<Mutex<Link>>, String> {
    if let Some(link) = transport.find_out_link(link_id).await {
        return Ok(link);
    }
    transport.find_in_link(link_id).await.ok_or_else(|| format!("unknown link {link_id}"))
}

fn address_param(params: &Value, name: &str) -> Result<AddressHash, String> {
    let value = params
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string parameter {name}"))?;
    AddressHash::new_from_hex_string(value.trim_matches('/'))
        .map_err(|error| format!("invalid {name}: {error:?}"))
}

fn required_hex_bytes(params: &Value, name: &str, expected: usize) -> Result<Vec<u8>, String> {
    let value = params
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string parameter {name}"))?;
    let decoded = hex::decode(value).map_err(|error| format!("invalid {name}: {error}"))?;
    if decoded.len() != expected {
        return Err(format!("{name} must decode to {expected} bytes"));
    }
    Ok(decoded)
}

fn decode_msgpack_value(data: &[u8]) -> Result<rmpv::Value, String> {
    let mut cursor = Cursor::new(data);
    let value = rmpv::decode::read_value(&mut cursor)
        .map_err(|error| format!("decode MessagePack application data: {error}"))?;
    if cursor.position() != data.len() as u64 {
        return Err("MessagePack application data has trailing bytes".into());
    }
    Ok(value)
}

fn encode_msgpack_value(value: &rmpv::Value) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    rmpv::encode::write_value(&mut data, value)
        .map_err(|error| format!("encode MessagePack envelope: {error}"))?;
    Ok(data)
}

pub(super) fn request_envelope_details(
    context: Option<PacketContext>,
    data: &[u8],
) -> Option<(Option<String>, Option<String>)> {
    if !matches!(context, Some(PacketContext::Request | PacketContext::Response)) {
        return None;
    }
    let value = decode_msgpack_value(data).ok()?;
    let values = value.as_array()?;
    let (path_hash, application) = if context == Some(PacketContext::Request) {
        (values.get(1)?.as_slice().map(hex::encode), values.get(2)?)
    } else {
        (None, values.get(1)?)
    };
    let application = match application {
        rmpv::Value::Binary(value) => Some(value.clone()),
        value => encode_msgpack_value(value).ok(),
    }
    .map(|value| BASE64.encode(value));
    Some((path_hash, application))
}

fn required_bytes(params: &Value, name: &str) -> Result<Vec<u8>, String> {
    let value = params
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing base64 parameter {name}"))?;
    BASE64.decode(value).map_err(|error| format!("invalid base64 {name}: {error}"))
}

fn optional_bytes(params: &Value, name: &str) -> Result<Option<Vec<u8>>, String> {
    params.get(name).map(|_| required_bytes(params, name)).transpose()
}

fn link_status(status: LinkStatus) -> &'static str {
    match status {
        LinkStatus::Pending => "pending",
        LinkStatus::Handshake => "handshake",
        LinkStatus::Active => "active",
        LinkStatus::Stale => "stale",
        LinkStatus::Closed => "closed",
    }
}

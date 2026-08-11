use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rns_ctl::args::Args;
use rns_ctl::cmd::http::{prepare_embedded_with_state, HttpRunOptions};
use rns_ctl::{api::NodeHandle, server};
use rns_net::link_manager::RequestResponse;
use rns_net::DestHash;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

type PreparedResources = Arc<Mutex<HashMap<String, Vec<u8>>>>;

fn main() {
    if let Err(error) = run() {
        eprintln!("rns-rs interop control: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut raw_args = std::env::args().collect::<Vec<_>>();
    let control_index = raw_args
        .iter()
        .position(|value| value == "--interop-control")
        .ok_or_else(|| "missing --interop-control HOST:PORT".to_string())?;
    if control_index + 1 >= raw_args.len() {
        return Err("missing --interop-control value".into());
    }
    let control_addr = raw_args.remove(control_index + 1);
    raw_args.remove(control_index);

    let mut args = Args::parse_control_from(raw_args);
    if args.positional.first().map(String::as_str) == Some("http") {
        args.positional.remove(0);
    }
    let prepared = prepare_embedded_with_state(
        args,
        HttpRunOptions { init_logging: true, install_signal_handler: false },
        None,
    )?;
    register_handlers(&prepared.ctx.node)?;

    let http_addr = prepared.addr;
    let http_ctx = prepared.ctx.clone();
    thread::spawn(move || {
        if let Err(error) = server::run_server(http_addr, http_ctx) {
            eprintln!("rns-rs HTTP controller stopped: {error}");
        }
    });

    let control_addr: SocketAddr = control_addr
        .parse()
        .map_err(|error| format!("invalid interop control address: {error}"))?;
    let listener = TcpListener::bind(control_addr)
        .map_err(|error| format!("bind interop control {control_addr}: {error}"))?;
    let prepared_resources = Arc::new(Mutex::new(HashMap::new()));
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                handle_control(stream, &prepared.ctx.node, &prepared.ctx.state, &prepared_resources)
            }
            Err(error) => eprintln!("rns-rs interop control accept failed: {error}"),
        }
    }
    Ok(())
}

fn register_handlers(node: &NodeHandle) -> Result<(), String> {
    let guard = node.lock().map_err(|_| "rns-rs node lock poisoned".to_string())?;
    let node = guard.as_ref().ok_or_else(|| "rns-rs node unavailable".to_string())?;
    node.register_request_handler("/interop/echo", None, |_, _, data, _| Some(data.to_vec()))
        .map_err(|_| "register rns-rs echo handler".to_string())?;
    node.register_request_handler_response("/interop/resource", None, |_, _, data, _| {
        let size = rns_core::msgpack::unpack_exact(data)
            .ok()
            .and_then(|value| value.as_uint())
            .and_then(|value| usize::try_from(value).ok())?;
        Some(RequestResponse::Resource {
            data: deterministic_payload(size),
            metadata: None,
            auto_compress: true,
        })
    })
    .map_err(|_| "register rns-rs Resource response handler".to_string())?;
    Ok(())
}

fn deterministic_payload(size: usize) -> Vec<u8> {
    (0..size).map(|index| (index % 251) as u8).collect()
}

fn handle_control(
    mut stream: TcpStream,
    node: &NodeHandle,
    state: &rns_ctl::state::SharedState,
    prepared: &PreparedResources,
) {
    let request = {
        let mut line = String::new();
        let mut reader = BufReader::new(&mut stream);
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => serde_json::from_str::<Value>(line.trim())
                .map_err(|error| format!("decode control request: {error}")),
            Err(error) => Err(format!("read control request: {error}")),
        }
    };
    let response = request.and_then(|request| dispatch(&request, node, state, prepared));
    let body = match response {
        Ok(value) => json!({"ok": true, "result": value}),
        Err(error) => json!({"ok": false, "error": error}),
    };
    let _ = writeln!(stream, "{body}");
}

fn dispatch(
    request: &Value,
    node: &NodeHandle,
    state: &rns_ctl::state::SharedState,
    prepared: &PreparedResources,
) -> Result<Value, String> {
    match request.get("method").and_then(Value::as_str) {
        Some("health") => Ok(json!({"status": "ok"})),
        Some("request") => send_request(request.get("params").unwrap_or(&Value::Null), node),
        Some("set_resource_strategy") => {
            set_resource_strategy(request.get("params").unwrap_or(&Value::Null), node)
        }
        Some("prepare_resource") => {
            prepare_resource(request.get("params").unwrap_or(&Value::Null), prepared)
        }
        Some("send_prepared_resource") => {
            send_prepared_resource(request.get("params").unwrap_or(&Value::Null), node, prepared)
        }
        Some("clear_resource_events") => clear_resource_events(state),
        Some("received_resource_digest") => {
            received_resource_digest(request.get("params").unwrap_or(&Value::Null), state)
        }
        Some("create_links") => create_links(request.get("params").unwrap_or(&Value::Null), node),
        Some("teardown_links") => {
            teardown_links(request.get("params").unwrap_or(&Value::Null), node)
        }
        Some("link_event_counts") => link_event_counts(state),
        Some(method) => Err(format!("unknown control method {method}")),
        None => Err("control request missing method".into()),
    }
}

fn create_links(params: &Value, node: &NodeHandle) -> Result<Value, String> {
    let destination = decode_array::<16>(params, "destination_hash")?;
    let count = params
        .get("count")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "count must be positive".to_string())?;
    let guard = node.lock().map_err(|_| "rns-rs node lock poisoned".to_string())?;
    let node = guard.as_ref().ok_or_else(|| "rns-rs node unavailable".to_string())?;
    let recalled = node
        .recall_identity(&DestHash(destination))
        .map_err(|_| "rns-rs identity query failed".to_string())?
        .ok_or_else(|| "destination identity is not recalled".to_string())?;
    let mut signing_key = [0u8; 32];
    signing_key.copy_from_slice(&recalled.public_key[32..64]);
    let links = (0..count)
        .map(|_| {
            node.create_link(destination, signing_key)
                .map(hex::encode)
                .map_err(|_| "rns-rs Link creation failed".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"count": links.len(), "link_ids": links}))
}

fn teardown_links(params: &Value, node: &NodeHandle) -> Result<Value, String> {
    let links = params
        .get("link_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| "link_ids must be an array".to_string())?;
    let guard = node.lock().map_err(|_| "rns-rs node lock poisoned".to_string())?;
    let node = guard.as_ref().ok_or_else(|| "rns-rs node unavailable".to_string())?;
    for link in links {
        let encoded = link.as_str().ok_or_else(|| "link_ids must contain strings".to_string())?;
        let params = json!({"link_id": encoded});
        node.teardown_link(decode_array::<16>(&params, "link_id")?)
            .map_err(|_| "rns-rs Link teardown failed".to_string())?;
    }
    Ok(json!({"count": links.len()}))
}

fn link_event_counts(state: &rns_ctl::state::SharedState) -> Result<Value, String> {
    let guard = state.read().map_err(|_| "rns-rs state lock poisoned".to_string())?;
    let established =
        guard.link_events.iter().filter(|event| event.event_type == "established").count();
    let closed = guard.link_events.iter().filter(|event| event.event_type == "closed").count();
    Ok(json!({"established": established, "closed": closed, "retained": guard.link_events.len()}))
}

fn prepare_resource(params: &Value, prepared: &PreparedResources) -> Result<Value, String> {
    let size = params
        .get("size")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| "size must be a usize".to_string())?;
    let seed = params.get("seed").and_then(Value::as_u64).unwrap_or(0x4c584d46);
    let key = format!("{seed:016x}-{size}");
    let payload = deterministic_benchmark_payload(size, seed);
    let digest = hex::encode(Sha256::digest(&payload));
    prepared
        .lock()
        .map_err(|_| "prepared resource lock poisoned".to_string())?
        .insert(key.clone(), payload);
    Ok(json!({"key": key, "bytes": size, "sha256": digest}))
}

fn send_prepared_resource(
    params: &Value,
    node: &NodeHandle,
    prepared: &PreparedResources,
) -> Result<Value, String> {
    let link_id = decode_array::<16>(params, "link_id")?;
    let key = params
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| "request missing key".to_string())?;
    let payload = prepared
        .lock()
        .map_err(|_| "prepared resource lock poisoned".to_string())?
        .remove(key)
        .ok_or_else(|| format!("unknown prepared resource {key}"))?;
    let guard = node.lock().map_err(|_| "rns-rs node lock poisoned".to_string())?;
    let node = guard.as_ref().ok_or_else(|| "rns-rs node unavailable".to_string())?;
    node.send_resource(link_id, payload, None)
        .map_err(|_| "rns-rs prepared Resource dispatch failed".to_string())?;
    Ok(json!({"dispatched": true, "key": key}))
}

fn clear_resource_events(state: &rns_ctl::state::SharedState) -> Result<Value, String> {
    state.write().map_err(|_| "rns-rs state lock poisoned".to_string())?.resource_events.clear();
    Ok(json!({"cleared": true}))
}

fn received_resource_digest(
    params: &Value,
    state: &rns_ctl::state::SharedState,
) -> Result<Value, String> {
    let link_id = params
        .get("link_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "request missing link_id".to_string())?;
    let guard = state.read().map_err(|_| "rns-rs state lock poisoned".to_string())?;
    let event = guard
        .resource_events
        .iter()
        .rev()
        .find(|event| event.link_id == link_id && event.event_type == "received")
        .ok_or_else(|| "received resource unavailable".to_string())?;
    let data = event
        .data_base64
        .as_deref()
        .ok_or_else(|| "received resource has no data".to_string())
        .and_then(|value| BASE64.decode(value).map_err(|error| error.to_string()))?;
    Ok(json!({"bytes": data.len(), "sha256": hex::encode(Sha256::digest(&data))}))
}

fn deterministic_benchmark_payload(size: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.max(1);
    (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

fn send_request(params: &Value, node: &NodeHandle) -> Result<Value, String> {
    let link_id = decode_array::<16>(params, "link_id")?;
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "request missing path".to_string())?;
    let data = params
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| "request missing data".to_string())
        .and_then(|value| BASE64.decode(value).map_err(|error| format!("invalid data: {error}")))?;
    let maximum = params
        .get("max_response_size")
        .and_then(Value::as_u64)
        .map(usize::try_from)
        .transpose()
        .map_err(|error| format!("invalid max_response_size: {error}"))?;
    let guard = node.lock().map_err(|_| "rns-rs node lock poisoned".to_string())?;
    let node = guard.as_ref().ok_or_else(|| "rns-rs node unavailable".to_string())?;
    node.send_request_with_max_response_size(link_id, path, &data, maximum)
        .map_err(|_| "rns-rs request dispatch failed".to_string())?;
    Ok(json!({"dispatched": true, "path": path}))
}

fn set_resource_strategy(params: &Value, node: &NodeHandle) -> Result<Value, String> {
    let link_id = decode_array::<16>(params, "link_id")?;
    let strategy = params
        .get("strategy")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .filter(|value| *value <= 2)
        .ok_or_else(|| "strategy must be 0, 1 or 2".to_string())?;
    let guard = node.lock().map_err(|_| "rns-rs node lock poisoned".to_string())?;
    let node = guard.as_ref().ok_or_else(|| "rns-rs node unavailable".to_string())?;
    node.set_resource_strategy(link_id, strategy)
        .map_err(|_| "rns-rs resource strategy dispatch failed".to_string())?;
    Ok(json!({"link_id": params["link_id"], "strategy": strategy}))
}

fn decode_array<const N: usize>(params: &Value, key: &str) -> Result<[u8; N], String> {
    let value =
        params.get(key).and_then(Value::as_str).ok_or_else(|| format!("request missing {key}"))?;
    if value.len() != N * 2 {
        return Err(format!("{key} must contain {} hexadecimal characters", N * 2));
    }
    let mut bytes = [0u8; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|error| format!("invalid {key}: {error}"))?;
    }
    Ok(bytes)
}

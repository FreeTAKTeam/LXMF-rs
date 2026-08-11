use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rns_core::transport::types::InterfaceId;
use rns_core::types::{DestHash, IdentityHash, PacketHash, ProofStrategy};
use rns_crypto::identity::Identity;
use rns_net::shared_client::SharedClientConfig;
use rns_net::{AnnouncedIdentity, Callbacks, Destination, RnsNode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct State {
    online: bool,
    reconnects: usize,
    received: Vec<Value>,
    proofs: Vec<String>,
}

struct ClientCallbacks {
    state: Arc<Mutex<State>>,
    identity: Identity,
}

impl Callbacks for ClientCallbacks {
    fn on_announce(&mut self, _: AnnouncedIdentity) {}
    fn on_path_updated(&mut self, _: DestHash, _: u8) {}

    fn on_local_delivery(&mut self, dest_hash: DestHash, data: Vec<u8>, packet_hash: PacketHash) {
        let Ok(packet) = rns_core::packet::RawPacket::unpack(&data) else {
            return;
        };
        let Ok(plaintext) = self.identity.decrypt(&packet.data) else {
            return;
        };
        self.state.lock().expect("shared client state").received.push(json!({
            "destination_hash": hex::encode(dest_hash.0),
            "data": BASE64.encode(&plaintext),
            "sha256": hex::encode(Sha256::digest(&plaintext)),
            "packet_hash": hex::encode(packet_hash.0),
        }));
    }

    fn on_interface_up(&mut self, _: InterfaceId) {
        let mut state = self.state.lock().expect("shared client state");
        state.online = true;
        state.reconnects += 1;
    }

    fn on_interface_down(&mut self, _: InterfaceId) {
        self.state.lock().expect("shared client state").online = false;
    }

    fn on_proof(&mut self, _: DestHash, packet_hash: PacketHash, _: f64) {
        self.state.lock().expect("shared client state").proofs.push(hex::encode(packet_hash.0));
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rns-rs shared client: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let shared_port = args
        .next()
        .ok_or_else(|| "usage: shared_client SHARED_PORT CONTROL_ADDR".to_string())?
        .parse::<u16>()
        .map_err(|error| format!("invalid shared port: {error}"))?;
    let control: SocketAddr = args
        .next()
        .ok_or_else(|| "missing control address".to_string())?
        .parse()
        .map_err(|error| format!("invalid control address: {error}"))?;
    let state = Arc::new(Mutex::new(State::default()));
    let private_key = [0x4d; 64];
    let node = Arc::new(
        RnsNode::connect_shared(
            SharedClientConfig {
                instance_name: "independent-lxmf-rs".into(),
                port: shared_port,
                rpc_port: 0,
            },
            Box::new(ClientCallbacks {
                state: state.clone(),
                identity: Identity::from_private_key(&private_key),
            }),
        )
        .map_err(|error| format!("connect shared instance: {error}"))?,
    );
    let identity = Identity::from_private_key(&private_key);
    let identity_hash = IdentityHash(*identity.hash());
    let destination = Destination::single_in("interop", &["probe"], identity_hash)
        .set_proof_strategy(ProofStrategy::ProveAll);
    node.register_destination_with_proof(&destination, Some(private_key))
        .map_err(|_| "register shared client destination".to_string())?;
    let listener = TcpListener::bind(control)
        .map_err(|error| format!("bind shared client control {control}: {error}"))?;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle(stream, &node, &identity, &destination, &state),
            Err(error) => eprintln!("shared client control accept: {error}"),
        }
    }
    Ok(())
}

fn handle(
    mut stream: TcpStream,
    node: &RnsNode,
    identity: &Identity,
    destination: &Destination,
    state: &Arc<Mutex<State>>,
) {
    let result = (|| -> Result<Value, String> {
        let mut line = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut line)
            .map_err(|error| format!("read control request: {error}"))?;
        let request: Value =
            serde_json::from_str(&line).map_err(|error| format!("decode request: {error}"))?;
        dispatch(&request, node, identity, destination, state)
    })();
    let response = match result {
        Ok(value) => json!({"ok": true, "result": value}),
        Err(error) => json!({"ok": false, "error": error}),
    };
    let _ = writeln!(stream, "{response}");
}

fn dispatch(
    request: &Value,
    node: &RnsNode,
    identity: &Identity,
    destination: &Destination,
    state: &Arc<Mutex<State>>,
) -> Result<Value, String> {
    let params = request.get("params").unwrap_or(&Value::Null);
    match request.get("method").and_then(Value::as_str) {
        Some("status") => {
            let state = state.lock().map_err(|_| "shared client state poisoned")?;
            Ok(json!({
                "connected": state.online,
                "reconnects": state.reconnects,
                "destination_hash": hex::encode(destination.hash.0),
                "received": state.received,
                "proofs": state.proofs,
            }))
        }
        Some("announce") => {
            let app_data = decode(params, "app_data")?;
            node.announce(destination, identity, Some(&app_data))
                .map_err(|_| "announce shared destination".to_string())?;
            Ok(json!({"announced": true}))
        }
        Some("known") => {
            let hash = decode_hash(params, "destination_hash")?;
            let known = node
                .recall_identity(&DestHash(hash))
                .map_err(|_| "recall destination identity".to_string())?
                .is_some();
            Ok(json!({"known": known}))
        }
        Some("clear") => {
            let mut state = state.lock().map_err(|_| "shared client state poisoned")?;
            state.received.clear();
            state.proofs.clear();
            Ok(json!({"cleared": true}))
        }
        Some("send") => {
            let hash = decode_hash(params, "destination_hash")?;
            let recalled = node
                .recall_identity(&DestHash(hash))
                .map_err(|_| "recall destination identity".to_string())?
                .ok_or_else(|| "destination identity is not recalled".to_string())?;
            let outbound = Destination::single_out("interop", &["probe"], &recalled);
            let packet_hash = node
                .send_packet(&outbound, &decode(params, "data")?)
                .map_err(|_| "send shared client packet".to_string())?;
            Ok(
                json!({"token": hex::encode(packet_hash.0), "packet_hash": hex::encode(packet_hash.0)}),
            )
        }
        Some(method) => Err(format!("unknown method {method}")),
        None => Err("missing method".to_string()),
    }
}

fn decode(params: &Value, key: &str) -> Result<Vec<u8>, String> {
    BASE64
        .decode(params.get(key).and_then(Value::as_str).unwrap_or_default())
        .map_err(|error| format!("decode {key}: {error}"))
}

fn decode_hash(params: &Value, key: &str) -> Result<[u8; 16], String> {
    let bytes = hex::decode(
        params.get(key).and_then(Value::as_str).ok_or_else(|| format!("missing {key}"))?,
    )
    .map_err(|error| format!("decode {key}: {error}"))?;
    bytes.try_into().map_err(|_| format!("{key} must be 16 bytes"))
}

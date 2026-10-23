use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, InterfaceMutationOwner};
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::{
    encode_packet_ifac, IfacState, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::time::{sleep, timeout, Duration};

#[path = "bootstrap_i2p_shutdown.rs"]
mod shutdown_tests;

const INITIAL: &str = "i2p-family-initial-credential";
const ROTATED: &str = "i2p-family-rotated-credential";
const FINAL: &str = "i2p-family-final-credential";
type SamPeer = BufReader<TcpStream>;

fn private_destination() -> String {
    // A structurally valid zero-certificate destination, used only by fake SAM.
    "A".repeat(516)
}

fn assert_redacted(value: &Value) {
    let text = value.to_string();
    for credential in [INITIAL, ROTATED, FINAL, private_destination().as_str()] {
        assert!(!text.contains(credential), "runtime diagnostics cannot expose private values");
    }
}

fn config(port: u16, other: u16, incoming: bool, credential: &str, mtu: usize) -> String {
    let peers = if incoming { "[]" } else { "['peer.b32.i2p']" };
    format!("interfaces=[{{type='i2p',enabled=true,name='i2p-family',sam_host='127.0.0.1',sam_port={port},peers={peers},connectable={incoming},reconnect_backoff_ms=50,mtu={mtu},network_name='i2p-family-network',passphrase='{credential}'}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other}}}]\n[reticulum]\nshare_instance=false")
}

async fn start(
    text: &str,
    root: &Path,
) -> (Arc<Transport>, Arc<rns_rpc::RpcDaemon>, InterfaceMutationOwner) {
    let config = DaemonConfig::from_toml(text).expect("configured I2P");
    let startup = start_fixture(&Args::parse_from(["reticulumd"]), &config, root).await;
    assert!(startup.interfaces.startup_failures.is_empty());
    let transport = startup.transport.expect("native transport");
    let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
    daemon.replace_interfaces(startup.configured_interfaces.clone());
    let owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        transport.clone(),
        startup.interfaces.into_owned_roots(&startup.configured_interfaces),
        Arc::downgrade(&daemon),
        None,
        startup.configured_startup,
    );
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    (transport, daemon, owner)
}

async fn shutdown(transport: &Arc<Transport>, owner: &mut InterfaceMutationOwner) {
    timeout(Duration::from_secs(10), owner.shutdown())
        .await
        .expect("existing native cleanup deadline")
        .expect("native and reconnect workers joined");
    assert!(transport.interface_hashes().await.is_empty());
}

async fn accept(listener: &TcpListener) -> TcpStream {
    timeout(Duration::from_secs(3), listener.accept())
        .await
        .expect("native connection deadline")
        .expect("native connection")
        .0
}

async fn closed(peer: &mut SamPeer) {
    timeout(Duration::from_secs(2), peer.read_to_end(&mut Vec::new()))
        .await
        .expect("retired SAM socket EOF deadline")
        .expect("retired SAM socket EOF");
}

async fn line(peer: &mut SamPeer) -> String {
    let mut line = String::new();
    timeout(Duration::from_secs(3), peer.read_line(&mut line))
        .await
        .expect("SAM command barrier deadline")
        .expect("SAM command");
    assert!(line.len() <= 8192 && line.ends_with('\n'), "complete bounded SAM command");
    line.trim_end().to_string()
}

async fn reply(peer: &mut SamPeer, response: &str) {
    peer.get_mut().write_all(response.as_bytes()).await.expect("fake SAM response");
}

async fn hello(listener: &TcpListener) -> SamPeer {
    let mut peer = BufReader::new(accept(listener).await);
    assert_eq!(line(&mut peer).await, "HELLO VERSION MIN=3.0 MAX=3.3");
    reply(&mut peer, "HELLO REPLY RESULT=OK VERSION=3.3\n").await;
    peer
}

async fn generate(listener: &TcpListener, malformed: bool) {
    let mut peer = hello(listener).await;
    assert_eq!(line(&mut peer).await, "DEST GENERATE SIGNATURE_TYPE=7");
    let prefix = if malformed { "DEST REPLY" } else { "DEST REPLY PUB=owned-public-destination" };
    reply(&mut peer, &format!("{prefix} PRIV={}\n", private_destination())).await;
    closed(&mut peer).await;
}

async fn session_request(listener: &TcpListener, incoming: bool) -> (SamPeer, String) {
    let mut peer = hello(listener).await;
    let command = line(&mut peer).await;
    let prefix = "SESSION CREATE STYLE=STREAM ID=";
    let fields = command.strip_prefix(prefix).expect("native SESSION CREATE");
    let (id, destination) = fields.split_once(" DESTINATION=").expect("native session fields");
    assert_eq!(destination, if incoming { private_destination() } else { "TRANSIENT".into() });
    assert_eq!(id.ends_with("-accept"), incoming);
    (peer, id.to_string())
}

async fn session(listener: &TcpListener, incoming: bool) -> (SamPeer, String) {
    let (mut peer, id) = session_request(listener, incoming).await;
    reply(&mut peer, "SESSION STATUS RESULT=OK DESTINATION=owned-public-destination\n").await;
    (peer, id)
}

async fn outgoing(listener: &TcpListener) -> (SamPeer, SamPeer) {
    let (control, id) = session(listener, false).await;
    let mut lookup = hello(listener).await;
    assert_eq!(line(&mut lookup).await, "NAMING LOOKUP NAME=peer.b32.i2p");
    reply(&mut lookup, "NAMING REPLY RESULT=OK NAME=peer.b32.i2p VALUE=resolved-destination\n")
        .await;
    closed(&mut lookup).await;
    let mut stream = hello(listener).await;
    assert_eq!(
        line(&mut stream).await,
        format!("STREAM CONNECT ID={id} DESTINATION=resolved-destination SILENT=false")
    );
    reply(&mut stream, "STREAM STATUS RESULT=OK\n").await;
    (control, stream)
}

async fn accept_request(listener: &TcpListener, id: &str) -> SamPeer {
    let mut peer = hello(listener).await;
    assert_eq!(line(&mut peer).await, format!("STREAM ACCEPT ID={id} SILENT=false"));
    reply(&mut peer, "STREAM STATUS RESULT=OK\n").await;
    peer
}

async fn incoming(listener: &TcpListener, id: &str, name: &str) -> SamPeer {
    let mut peer = accept_request(listener, id).await;
    reply(&mut peer, &format!("{name}\n")).await;
    peer
}

async fn published(daemon: &Arc<rns_rpc::RpcDaemon>, ready: impl Fn(&Value) -> bool) -> Value {
    timeout(Duration::from_secs(3), async {
        loop {
            let runtime = daemon.interface_records()[0].settings.as_ref().expect("settings")
                ["_runtime"]
                .clone();
            let status = &runtime["i2p"]["tunnel_status"];
            if ready(status) {
                assert_redacted(&runtime);
                break status.clone();
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("published native I2P state deadline")
}

fn outgoing_state(status: &Value, state: &str) -> bool {
    status["peers"].as_array().is_some_and(|peers| {
        peers.iter().any(|peer| peer["direction"] == "outbound" && peer["state"] == state)
    })
}

async fn child(
    transport: &Arc<Transport>,
    parent: AddressHash,
    existing: &[AddressHash],
) -> AddressHash {
    timeout(Duration::from_secs(3), async {
        loop {
            if let Some(child) =
                transport.iface_manager().lock().await.traffic_snapshots().into_iter().find(
                    |entry| entry.parent == Some(parent) && !existing.contains(&entry.address),
                )
            {
                break child.address;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native I2P virtual child registration")
}

async fn policy(transport: &Arc<Transport>, address: AddressHash, credential: &str, mtu: usize) {
    let manager = transport.iface_manager();
    let manager = manager.lock().await;
    assert_eq!(manager.mtu(&address), Some(mtu));
    let policy = manager.shared_config(&address).expect("native policy");
    assert_eq!(policy.passphrase.as_deref(), Some(credential));
    assert_eq!(policy.ifac_size, None, "native carrier owns its sixteen-byte default");
}

async fn rotate(transport: &Arc<Transport>, address: AddressHash) {
    let manager = transport.iface_manager();
    let mut manager = manager.lock().await;
    let mut policy = manager.shared_config(&address).expect("native parent policy").clone();
    policy.passphrase = Some(ROTATED.into());
    assert!(manager.try_set_shared_config(address, policy).expect("native parent rotation"));
}

fn packet(marker: u8) -> Packet {
    Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0x7e, 0x7d, 0xc0, 0xdb]),
        ..Default::default()
    }
}

fn auth(credential: &str) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            network_name: Some("i2p-family-network".into()),
            passphrase: Some(credential.into()),
            ..Default::default()
        }
        .ifac_context_with_default_size(16)
        .expect("independent sixteen-byte IFAC peer"),
    ))
}

fn frame(credential: &str, packet: &Packet) -> Vec<u8> {
    let payload = encode_packet_ifac(&auth(credential), packet).expect("IFAC packet");
    assert_eq!(payload.len(), packet.to_bytes().expect("plain packet").len() + 16);
    Hdlc::frame(&payload).expect("HDLC frame")
}

async fn authenticate(
    transport: &Arc<Transport>,
    address: AddressHash,
    peer: &mut SamPeer,
    credential: &str,
    rejected: &str,
    marker: u8,
) -> Vec<u8> {
    let manager = transport.iface_manager();
    let mut ingress = transport.iface_rx();
    let violations = || async {
        manager
            .lock()
            .await
            .traffic_snapshots()
            .into_iter()
            .find(|entry| entry.address == address)
            .expect("native child")
            .ifac_violations
    };
    let before = violations().await;
    let inbound = packet(marker);
    let mut negative = frame(rejected, &inbound);
    negative.extend(Hdlc::frame(&inbound.to_bytes().expect("plaintext packet")).expect("HDLC"));
    peer.get_mut().write_all(&negative).await.expect("negative native frames");
    timeout(Duration::from_secs(2), async {
        while violations().await != before + 2 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("both wrong-key/plaintext frames counted before admission");
    while let Ok(event) = ingress.try_recv() {
        assert_ne!(event.packet.destination, inbound.destination, "negative native ingress");
    }
    peer.get_mut()
        .write_all(&frame(credential, &inbound))
        .await
        .expect("native authenticated ingress");
    let received = timeout(Duration::from_secs(2), ingress.recv())
        .await
        .expect("native ingress deadline")
        .expect("native ingress");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, inbound.destination);
    assert_eq!(received.packet.data, inbound.data);
    assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));
    let outbound = packet(marker + 1);
    let expected = frame(credential, &outbound);
    assert_eq!(
        manager
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: outbound })
            .await
            .sent_ifaces,
        1
    );
    timeout(Duration::from_secs(2), async {
        let mut wire = Vec::new();
        let mut chunk = [0; 2048];
        loop {
            let count = peer.read(&mut chunk).await.expect("native framed egress");
            assert_ne!(count, 0, "native peer closed before authenticated output");
            wire.extend_from_slice(&chunk[..count]);
            assert!(wire.len() <= 16_384, "unexpected native output");
            if wire.windows(expected.len()).any(|bytes| bytes == expected) {
                break;
            }
        }
    })
    .await
    .expect("exact authenticated native egress deadline");
    expected
}

async fn isolated(peer: &mut SamPeer, forbidden: &[u8]) {
    let result = timeout(Duration::from_secs(2), async {
        let mut wire = Vec::new();
        let mut chunk = [0; 2048];
        loop {
            let count = peer.read(&mut chunk).await.expect("other peer observation");
            assert_ne!(count, 0, "unrelated accepted peer must remain live");
            wire.extend_from_slice(&chunk[..count]);
            assert!(wire.len() <= 16_384);
            assert!(
                !wire.windows(forbidden.len()).any(|bytes| bytes == forbidden),
                "direct I2P traffic leaked to another peer"
            );
        }
    })
    .await;
    assert!(result.is_err(), "observe the full peer-isolation interval");
}

fn reserve() -> TcpSocket {
    let socket = TcpSocket::new_v4().expect("owned SAM socket");
    socket
        .bind("127.0.0.1:0".parse().expect("loopback"))
        .expect("reserve unavailable SAM endpoint");
    socket
}

#[tokio::test]
async fn configured_i2p_outgoing_ifac_refusal_rotation_reconnect_and_restart() {
    let root = tempfile::tempdir().expect("state");
    let reserved = reserve();
    let port = reserved.local_addr().expect("SAM endpoint").port();
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("other endpoint").port();
    let text = config(port, other_port, false, INITIAL, 512);
    let (transport, daemon, mut owner) = start(&text, root.path()).await;
    let parent = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    let failed = published(&daemon, |s| outgoing_state(s, "reconnecting")).await;
    assert!(failed["peers"][0]["reconnect_attempts"].as_u64().expect("attempts") >= 1);
    assert!(failed["peers"][0]["last_error"].is_string());
    let address = child(&transport, parent, &[]).await;
    policy(&transport, parent, INITIAL, 512).await;
    policy(&transport, address, INITIAL, 512).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x51).await;
    let listener = reserved.listen(8).expect("same endpoint becomes available");
    let (mut control, mut peer) = outgoing(&listener).await;
    published(&daemon, |s| outgoing_state(s, "connected")).await;
    authenticate(&transport, address, &mut peer, INITIAL, ROTATED, 0x54).await;
    rotate(&transport, parent).await;
    policy(&transport, address, ROTATED, 512).await;
    authenticate(&transport, address, &mut peer, ROTATED, INITIAL, 0x57).await;
    drop(peer);
    let (mut next_control, mut reconnected) = outgoing(&listener).await;
    closed(&mut control).await;
    assert_eq!(child(&transport, parent, &[]).await, address);
    authenticate(&transport, address, &mut reconnected, ROTATED, INITIAL, 0x5a).await;
    assert_eq!(native_address(&daemon.interface_records()[0]), parent);
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x5d).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut reconnected).await;
    closed(&mut next_control).await;
    let mut unaffected = BufReader::new(unaffected);
    closed(&mut unaffected).await;
    let previous_identity = transport.internal_identity().await;

    let text = config(port, other_port, false, FINAL, 1024);
    let (transport, daemon, mut owner) = start(&text, root.path()).await;
    assert_ne!(transport.internal_identity().await, previous_identity);
    let parent = native_address(&daemon.interface_records()[0]);
    let address = child(&transport, parent, &[]).await;
    let (mut control, mut peer) = outgoing(&listener).await;
    policy(&transport, address, FINAL, 1024).await;
    authenticate(&transport, address, &mut peer, FINAL, ROTATED, 0x60).await;
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x63).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut peer).await;
    closed(&mut control).await;
    closed(&mut BufReader::new(unaffected)).await;
}

#[tokio::test]
async fn configured_i2p_incoming_ifac_failure_child_rotation_session_recovery_and_restart() {
    let root = tempfile::tempdir().expect("state");
    let reserved = reserve();
    let port = reserved.local_addr().expect("SAM endpoint").port();
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("other endpoint").port();
    let text = config(port, other_port, true, INITIAL, 512);
    let (transport, daemon, mut owner) = start(&text, root.path()).await;
    let parent = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    let failed = published(&daemon, |s| s["accept_state"] == "reconnecting").await;
    let attempts = failed["accept_reconnect_attempts"].as_u64().expect("attempts");
    let initial = daemon.interface_records()[0].settings.as_ref().expect("settings")["_runtime"]
        ["i2p"]
        .clone();
    assert!(initial["destination_prepare_error"].is_string());
    assert_eq!(initial["private_key_persisted"], false);
    let listener = reserved.listen(8).expect("same endpoint becomes available");
    generate(&listener, true).await;
    published(&daemon, |s| {
        s["accept_reconnect_attempts"].as_u64().is_some_and(|n| n > attempts)
            && s["last_accept_error"].as_str().is_some_and(|error| error.contains("missing PUB"))
    })
    .await;
    generate(&listener, false).await;
    let (mut control, id) = session(&listener, true).await;
    let key_path =
        initial["private_key_path"].as_str().expect("configured private destination path");
    assert!(Path::new(key_path).starts_with(root.path()));
    assert_eq!(
        std::fs::read_to_string(key_path).expect("native persisted destination"),
        private_destination()
    );
    let mut first = incoming(&listener, &id, "owned-peer-a").await;
    let first_address = child(&transport, parent, &[]).await;
    policy(&transport, first_address, INITIAL, 512).await;
    authenticate(&transport, first_address, &mut first, INITIAL, ROTATED, 0x71).await;
    rotate(&transport, parent).await;
    policy(&transport, first_address, ROTATED, 512).await;
    authenticate(&transport, first_address, &mut first, ROTATED, INITIAL, 0x74).await;
    let mut second = incoming(&listener, &id, "owned-peer-b").await;
    let second_address = child(&transport, parent, &[first_address]).await;
    policy(&transport, second_address, ROTATED, 512).await;
    let expected =
        authenticate(&transport, second_address, &mut second, ROTATED, INITIAL, 0x77).await;
    isolated(&mut first, &expected).await;
    let expected =
        authenticate(&transport, first_address, &mut first, ROTATED, INITIAL, 0x7a).await;
    isolated(&mut second, &expected).await;

    let mut expired = accept_request(&listener, &id).await;
    reply(
        &mut expired,
        "STREAM STATUS RESULT=I2P_ERROR MESSAGE=\"specified ID is not a STREAM session\"\n",
    )
    .await;
    closed(&mut expired).await;
    let (mut replacement_control, replacement_id) = session(&listener, true).await;
    assert_eq!(replacement_id, id);
    closed(&mut control).await;
    published(&daemon, |s| {
        s["accept_state"] == "listening"
            && s["accept_reconnect_attempts"].as_u64().is_some_and(|n| n > attempts + 1)
    })
    .await;
    let mut pending = accept_request(&listener, &replacement_id).await;
    assert_eq!(child(&transport, parent, &[]).await, first_address);
    authenticate(&transport, first_address, &mut first, ROTATED, INITIAL, 0x7d).await;
    authenticate(&transport, second_address, &mut second, ROTATED, INITIAL, 0x80).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x83).await;
    shutdown(&transport, &mut owner).await;
    for peer in [&mut first, &mut second, &mut replacement_control, &mut pending] {
        closed(peer).await;
    }
    closed(&mut BufReader::new(unaffected)).await;
    let previous_identity = transport.internal_identity().await;

    let text = config(port, other_port, true, FINAL, 1024);
    let state_path = root.path().to_owned();
    let restarting = tokio::spawn(async move { start(&text, &state_path).await });
    generate(&listener, false).await;
    let (transport, daemon, mut owner) = restarting.await.expect("configured restart");
    assert_ne!(transport.internal_identity().await, previous_identity);
    let metadata = daemon.interface_records()[0].settings.as_ref().expect("settings")["_runtime"]
        ["i2p"]
        .clone();
    assert_eq!(metadata["private_key_persisted"], true);
    assert!(metadata["reachable_endpoint"]
        .as_str()
        .expect("reachable endpoint")
        .ends_with(".b32.i2p"));
    assert_ne!(metadata["private_key_path"], initial["private_key_path"]);
    assert_redacted(&metadata);
    let parent = native_address(&daemon.interface_records()[0]);
    let (mut control, id) = session(&listener, true).await;
    let mut peer = incoming(&listener, &id, "owned-peer-after-restart").await;
    let address = child(&transport, parent, &[]).await;
    policy(&transport, address, FINAL, 1024).await;
    authenticate(&transport, address, &mut peer, FINAL, ROTATED, 0x86).await;
    let mut pending = accept_request(&listener, &id).await;
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x89).await;
    shutdown(&transport, &mut owner).await;
    for peer in [&mut peer, &mut control, &mut pending] {
        closed(peer).await;
    }
    closed(&mut BufReader::new(unaffected)).await;
}

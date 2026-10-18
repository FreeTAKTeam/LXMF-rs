use super::pty_peer::PtyPeer;
use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, InterfaceMutationOwner};
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::{
    encode_packet_ifac, IfacState, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::time::{sleep, timeout, Duration};

const INITIAL: &str = "serial-family-initial-credential";
const ROTATED: &str = "serial-family-rotated-credential";
const FINAL: &str = "serial-family-final-credential";
const AX25_HEADER: [u8; 16] = [
    0x82, 0xa0, 0xb4, 0xa4, 0x9c, 0xa6, 0x60, // APZRNS-0
    0x9c, 0x60, 0x86, 0x82, 0x98, 0x98, 0x61, // N0CALL-0
    0x03, 0xf0,
];
const KISS_INITIAL: [u8; 20] = [
    0xc0, 0x01, 20, 0xc0, // preamble 200 ms
    0xc0, 0x04, 3, 0xc0, // tail 30 ms
    0xc0, 0x02, 55, 0xc0, // persistence
    0xc0, 0x03, 1, 0xc0, // slot 10 ms
    0xc0, 0x0f, 1, 0xc0,
];

fn assert_redacted(value: &impl serde::Serialize) {
    let text = serde_json::to_string(value).expect("diagnostic JSON");
    for credential in [INITIAL, ROTATED, FINAL] {
        assert!(!text.contains(credential), "runtime diagnostics cannot expose credentials");
    }
}

fn config(kind: &str, connection: &str, other_port: u16, credential: &str, mtu: usize) -> String {
    let kiss = if kind == "serial" {
        ""
    } else {
        ",preamble_ms=200,tx_tail_ms=30,persistence=55,slot_time_ms=10,kiss_flow_control=false,callsign='N0CALL',ssid=0"
    };
    format!("interfaces=[{{type='{kind}',enabled=true,name='serial-family',{connection},mtu={mtu},reconnect_backoff_ms=50,max_reconnect_backoff_ms=100,network_name='serial-family-network',passphrase='{credential}'{kiss}}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other_port}}}]\n[reticulum]\nshare_instance=false")
}

async fn start(
    text: &str,
    root: &Path,
) -> (Arc<Transport>, Arc<rns_rpc::RpcDaemon>, InterfaceMutationOwner) {
    let config = DaemonConfig::from_toml(text).expect("configured serial/KISS");
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
        .expect("existing native cleanup bound")
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

async fn closed(peer: &mut TcpStream) {
    timeout(Duration::from_secs(2), peer.read_to_end(&mut Vec::new()))
        .await
        .expect("retired peer EOF deadline")
        .expect("retired peer EOF");
}

async fn status(daemon: &Arc<rns_rpc::RpcDaemon>, kind: &str, state: &str, errors: u64) -> Value {
    let (key, counter) = match kind {
        "serial" => ("serial", "open_errors"),
        "kiss_tcp_client" => ("kiss_tcp", "connect_errors"),
        _ => ("kiss", "open_errors"),
    };
    timeout(Duration::from_secs(3), async {
        loop {
            let record = &daemon.interface_records()[0];
            let runtime = &record.settings.as_ref().expect("settings")["_runtime"];
            let status = &runtime[key]["status"];
            if status["link_state"] == state
                && status[counter].as_u64().is_some_and(|n| n >= errors)
            {
                assert_redacted(runtime);
                break status.clone();
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("published native serial/KISS state deadline")
}

async fn policy(transport: &Arc<Transport>, address: AddressHash, key: &str, mtu: usize) {
    let manager = transport.iface_manager();
    let manager = manager.lock().await;
    assert_eq!(manager.mtu(&address), Some(mtu));
    let policy = manager.shared_config(&address).expect("native policy");
    assert_eq!(policy.passphrase.as_deref(), Some(key));
    assert_eq!(policy.ifac_size, None, "the native carrier owns its default");
}

async fn rotate(transport: &Arc<Transport>, address: AddressHash) {
    let manager = transport.iface_manager();
    let mut manager = manager.lock().await;
    let mut policy = manager.shared_config(&address).expect("native policy").clone();
    policy.passphrase = Some(ROTATED.into());
    assert!(manager.try_set_shared_config(address, policy).expect("native live IFAC rotation"));
}

fn auth(credential: &str) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            network_name: Some("serial-family-network".into()),
            passphrase: Some(credential.into()),
            ..Default::default()
        }
        .ifac_context_with_default_size(8)
        .expect("independent peer with eight-byte default"),
    ))
}

fn frame(kind: &str, payload: &[u8]) -> Vec<u8> {
    match kind {
        "serial" => Hdlc::frame(payload).expect("serial HDLC"),
        "ax25_kiss" => {
            let mut body = AX25_HEADER.to_vec();
            body.extend_from_slice(payload);
            rns_transport::kiss::encode_data_frame(&body)
        }
        "kiss" | "kiss_tcp_client" => rns_transport::kiss::encode_data_frame(payload),
        _ => panic!("undeclared native framing"),
    }
}

async fn initial_frames<R: AsyncRead + Unpin>(kind: &str, peer: &mut R) {
    if kind == "serial" {
        return;
    }
    let mut wire = [0; KISS_INITIAL.len()];
    timeout(Duration::from_secs(2), peer.read_exact(&mut wire))
        .await
        .expect("complete KISS initialization deadline")
        .expect("complete KISS initialization");
    assert_eq!(wire, KISS_INITIAL, "independently specified native initialization bytes");
}

async fn authenticate<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    transport: &Arc<Transport>,
    address: AddressHash,
    (mut reader, mut writer): (R, W),
    kind: &str,
    (credential, rejected): (&str, &str),
    marker: u8,
) {
    let packet = Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0x7e, 0x7d, 0xc0, 0xdb]),
        ..Default::default()
    };
    let manager = transport.iface_manager();
    let mut ingress = transport.iface_rx();
    let violations = || async {
        manager
            .lock()
            .await
            .traffic_snapshots()
            .into_iter()
            .find(|entry| entry.address == address)
            .expect("native root")
            .ifac_violations
    };
    let before = violations().await;
    let mut negative =
        frame(kind, &encode_packet_ifac(&auth(rejected), &packet).expect("wrong key"));
    negative.extend(frame(kind, &packet.to_bytes().expect("plaintext packet")));
    writer.write_all(&negative).await.expect("negative native frames");
    timeout(Duration::from_secs(2), async {
        while violations().await != before + 2 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong-key/plaintext counted before native admission");
    while let Ok(event) = ingress.try_recv() {
        assert_ne!(event.packet.destination, packet.destination, "negative native ingress");
    }
    let state = auth(credential);
    let payload = encode_packet_ifac(&state, &packet).expect("authenticated packet");
    assert_eq!(payload.len(), packet.to_bytes().expect("plain bytes").len() + 8);
    writer.write_all(&frame(kind, &payload)).await.expect("authenticated native ingress");
    let received = timeout(Duration::from_secs(2), ingress.recv())
        .await
        .expect("authenticated native ingress deadline")
        .expect("native ingress");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, packet.destination);
    assert_eq!(received.packet.data, packet.data);
    assert_eq!(received.packet.ifac.map(|ifac| ifac.length), Some(0));
    let outbound = Packet { destination: AddressHash::new([marker + 1; 16]), ..packet };
    let expected =
        frame(kind, &encode_packet_ifac(&state, &outbound).expect("authenticated outbound"));
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
            let count = reader.read(&mut chunk).await.expect("native framed egress");
            assert_ne!(count, 0, "native peer closed before exact authenticated output");
            wire.extend_from_slice(&chunk[..count]);
            assert!(wire.len() <= 16_384, "unexpected native output before target frame");
            if wire.windows(expected.len()).any(|bytes| bytes == expected) {
                break;
            }
        }
    })
    .await
    .expect("exact authenticated framed egress deadline");
}

async fn pty_lifecycle(kind: &str) {
    let root = tempfile::tempdir().expect("owned state");
    let alias = root.path().join("owned-device");
    let connection =
        format!("device={},baud_rate=115200", json!(alias.to_str().expect("device path")));
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("unrelated endpoint").port();
    let (transport, daemon, mut owner) =
        start(&config(kind, &connection, other_port, INITIAL, 512), root.path()).await;
    let initial = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    let failed = status(&daemon, kind, "open_failed", 1).await;
    assert!(failed["last_error"].is_string());
    assert_eq!(transport.iface_manager().lock().await.online(&initial), Some(false));
    policy(&transport, initial, INITIAL, 512).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x11).await;
    let mut first = PtyPeer::start().await;
    first.publish_alias(&alias);
    let running = status(&daemon, kind, "running", 1).await;
    assert_eq!(running["mtu"], 512);
    if kind != "serial" {
        assert_eq!(running["ax25"], kind == "ax25_kiss");
    }
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    initial_frames(kind, &mut first.output).await;
    authenticate(
        &transport,
        initial,
        (&mut first.output, &mut first.input),
        kind,
        (INITIAL, ROTATED),
        0x14,
    )
    .await;
    rotate(&transport, initial).await;
    policy(&transport, initial, ROTATED, 512).await;
    authenticate(
        &transport,
        initial,
        (&mut first.output, &mut first.input),
        kind,
        (ROTATED, INITIAL),
        0x17,
    )
    .await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x1a).await;
    let errors_before = running["open_errors"].as_u64().expect("open error count");
    // A retired slave name can be reused by another concurrent test's PTY.
    std::fs::remove_file(&alias).expect("remove only the owned alias while its descriptor is open");
    first.stop().await;
    let failed = status(&daemon, kind, "open_failed", errors_before + 1).await;
    assert!(
        failed["read_errors"].as_u64().expect("read errors") > 0
            || failed["eof_count"].as_u64().expect("EOF count") > 0
    );
    policy(&transport, initial, ROTATED, 512).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x1d).await;
    let mut next = PtyPeer::start().await;
    next.publish_alias(&alias);
    status(&daemon, kind, "running", errors_before + 1).await;
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    initial_frames(kind, &mut next.output).await;
    authenticate(
        &transport,
        initial,
        (&mut next.output, &mut next.input),
        kind,
        (ROTATED, INITIAL),
        0x20,
    )
    .await;
    policy(&transport, initial, ROTATED, 512).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x23).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut unaffected).await;
    let old_identity = transport.internal_identity().await;

    let (transport, daemon, mut owner) =
        start(&config(kind, &connection, other_port, FINAL, 1024), root.path()).await;
    assert_ne!(transport.internal_identity().await, old_identity);
    let current = native_address(&daemon.interface_records()[0]);
    status(&daemon, kind, "running", 0).await;
    initial_frames(kind, &mut next.output).await;
    authenticate(
        &transport,
        current,
        (&mut next.output, &mut next.input),
        kind,
        (FINAL, ROTATED),
        0x26,
    )
    .await;
    policy(&transport, current, FINAL, 1024).await;
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x29).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut unaffected).await;
    next.stop().await;
}

#[tokio::test]
async fn configured_serial_ifac_open_failure_rotation_reopen_and_restart() {
    pty_lifecycle("serial").await;
}

#[tokio::test]
async fn configured_kiss_ifac_open_failure_rotation_reopen_and_restart() {
    pty_lifecycle("kiss").await;
}

#[tokio::test]
async fn configured_ax25_kiss_ifac_open_failure_rotation_reopen_and_restart() {
    pty_lifecycle("ax25_kiss").await;
}

#[tokio::test]
async fn configured_kiss_tcp_ifac_refusal_rotation_reconnect_and_restart() {
    let kind = "kiss_tcp_client";
    let root = tempfile::tempdir().expect("owned state");
    let reserved = TcpSocket::new_v4().expect("owned native socket");
    reserved.bind("127.0.0.1:0".parse().expect("loopback")).expect("reserve refused endpoint");
    let port = reserved.local_addr().expect("reserved endpoint").port();
    let connection = format!("host='127.0.0.1',port={port}");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("unrelated endpoint").port();
    let (transport, daemon, mut owner) =
        start(&config(kind, &connection, other_port, INITIAL, 512), root.path()).await;
    let initial = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    let failed = status(&daemon, kind, "connect_failed", 1).await;
    assert!(failed["last_error"].is_string());
    assert_eq!(transport.iface_manager().lock().await.online(&initial), Some(false));
    policy(&transport, initial, INITIAL, 512).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x41).await;
    let listener = reserved.listen(8).expect("same reserved socket begins listening");
    let mut peer = accept(&listener).await;
    status(&daemon, kind, "running", 1).await;
    initial_frames(kind, &mut peer).await;
    authenticate(&transport, initial, peer.split(), kind, (INITIAL, ROTATED), 0x44).await;
    rotate(&transport, initial).await;
    authenticate(&transport, initial, peer.split(), kind, (ROTATED, INITIAL), 0x47).await;
    policy(&transport, initial, ROTATED, 512).await;
    peer.shutdown().await.expect("close only current native KISS connection");
    closed(&mut peer).await;
    let mut reconnected = accept(&listener).await;
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    initial_frames(kind, &mut reconnected).await;
    authenticate(&transport, initial, reconnected.split(), kind, (ROTATED, INITIAL), 0x4a).await;
    policy(&transport, initial, ROTATED, 512).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x4d).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut reconnected).await;
    closed(&mut unaffected).await;
    let old_identity = transport.internal_identity().await;

    let (transport, daemon, mut owner) =
        start(&config(kind, &connection, other_port, FINAL, 1024), root.path()).await;
    assert_ne!(transport.internal_identity().await, old_identity);
    let current = native_address(&daemon.interface_records()[0]);
    let mut peer = accept(&listener).await;
    status(&daemon, kind, "running", 0).await;
    initial_frames(kind, &mut peer).await;
    authenticate(&transport, current, peer.split(), kind, (FINAL, ROTATED), 0x50).await;
    policy(&transport, current, FINAL, 1024).await;
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x53).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut peer).await;
    closed(&mut unaffected).await;
}

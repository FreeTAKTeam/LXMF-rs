use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, InterfaceMutationOwner};
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::{
    encode_packet_ifac, IfacState, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::json;
use std::net::Ipv6Addr;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::time::{sleep, timeout, Duration};

const INITIAL: &str = "tcp-family-initial-credential";
const ROTATED: &str = "tcp-family-rotated-credential";
const ATTEMPTED: &str = "tcp-family-attempted-credential";
const FINAL: &str = "tcp-family-final-credential";

fn assert_redacted(value: &impl serde::Serialize) {
    let text = serde_json::to_string(value).expect("diagnostic JSON");
    for credential in [INITIAL, ROTATED, ATTEMPTED, FINAL] {
        assert!(!text.contains(credential), "runtime diagnostics cannot expose credentials");
    }
}

async fn start(
    config: &DaemonConfig,
    root: &Path,
) -> (Arc<Transport>, Arc<rns_rpc::RpcDaemon>, InterfaceMutationOwner) {
    let args = Args::parse_from(["reticulumd"]);
    let startup = start_fixture(&args, config, root).await;
    assert!(
        startup.interfaces.startup_failures.is_empty(),
        "unexpected configured startup failure"
    );
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
        .expect("all native and reconnect workers joined");
    assert!(transport.interface_hashes().await.is_empty());
}

fn config(kind: &str, host: &str, port: u16, other_port: u16, credential: &str) -> String {
    // Listener-only strictness avoids unrelated client preflight probe connections.
    let strict_listener = matches!(kind, "tcp_server" | "backbone");
    format!("interfaces=[{{type='{kind}',enabled=true,name='family-root',host='{host}',port={port},ifac_size=64,network_name='tcp-family-network',passphrase='{credential}',mtu=512,connect_timeout=1,max_reconnect_tries=4,prefer_ipv6=true}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other_port}}}]\n[reticulum]\nshare_instance=false\npanic_on_interface_error={strict_listener}")
}

async fn accept(listener: &TcpListener, bound: u64) -> TcpStream {
    timeout(Duration::from_secs(bound), listener.accept())
        .await
        .expect("native connection deadline")
        .expect("native connection")
        .0
}

async fn connect_ipv6(port: u16) -> TcpStream {
    let stream = timeout(Duration::from_secs(2), TcpStream::connect((Ipv6Addr::LOCALHOST, port)))
        .await
        .expect("IPv6 connection deadline")
        .expect("preferred IPv6 listener must be reachable");
    assert!(stream.peer_addr().expect("native endpoint").is_ipv6());
    stream
}

async fn closed(stream: &mut TcpStream) {
    timeout(Duration::from_secs(2), stream.read_to_end(&mut Vec::new()))
        .await
        .expect("retired peer EOF deadline")
        .expect("retired peer EOF");
}

async fn child(
    transport: &Arc<Transport>,
    parent: AddressHash,
    existing: &[AddressHash],
) -> AddressHash {
    timeout(Duration::from_secs(2), async {
        loop {
            if let Some(snapshot) =
                transport.iface_manager().lock().await.traffic_snapshots().into_iter().find(
                    |entry| entry.parent == Some(parent) && !existing.contains(&entry.address),
                )
            {
                break snapshot.address;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native accepted child registration")
}

fn auth(credential: &str) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            ifac_size: Some(64),
            network_name: Some("tcp-family-network".into()),
            passphrase: Some(credential.into()),
            ..Default::default()
        }
        .ifac_context()
        .expect("independent IFAC peer"),
    ))
}

async fn authenticate(
    transport: &Arc<Transport>,
    address: AddressHash,
    stream: &mut TcpStream,
    credential: &str,
    rejected: &str,
    marker: u8,
) {
    let packet = Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0x7e, 0x7d, 0xc0, 0xdb]),
        ..Default::default()
    };
    let manager = transport.iface_manager();
    let mut packets = transport.iface_rx();
    let violations = || async {
        manager
            .lock()
            .await
            .traffic_snapshots()
            .into_iter()
            .find(|entry| entry.address == address)
            .expect("native root/child")
            .ifac_violations
    };
    let before = violations().await;
    let mut negative =
        Hdlc::frame(&encode_packet_ifac(&auth(rejected), &packet).expect("wrong key"))
            .expect("HDLC wrong-key frame");
    negative.extend(
        Hdlc::frame(&packet.to_bytes().expect("plaintext packet")).expect("HDLC plaintext"),
    );
    stream.write_all(&negative).await.expect("negative native wire");
    timeout(Duration::from_secs(2), async {
        while violations().await != before + 2 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("both negative frames rejected on exact native interface");
    while let Ok(event) = packets.try_recv() {
        assert_ne!(
            event.packet.destination, packet.destination,
            "rejected frame cannot enter ingress"
        );
    }
    let state = auth(credential);
    stream
        .write_all(
            &Hdlc::frame(&encode_packet_ifac(&state, &packet).expect("authenticated packet"))
                .expect("HDLC authenticated frame"),
        )
        .await
        .expect("authenticated native ingress");
    let received = timeout(Duration::from_secs(2), packets.recv())
        .await
        .expect("authenticated ingress deadline")
        .expect("native ingress event");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.destination, packet.destination);
    assert_eq!(received.packet.data, packet.data);
    let outbound = Packet { destination: AddressHash::new([marker + 1; 16]), ..packet };
    let expected =
        Hdlc::frame(&encode_packet_ifac(&state, &outbound).expect("authenticated outbound"))
            .expect("outbound HDLC");
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
        let mut buffer = [0; 2048];
        loop {
            let count = stream.read(&mut buffer).await.expect("native egress");
            assert_ne!(count, 0, "stream closed before exact authenticated egress");
            wire.extend_from_slice(&buffer[..count]);
            assert!(wire.len() <= 16_384, "unexpected output before target frame");
            if wire.windows(expected.len()).any(|candidate| candidate == expected) {
                break;
            }
        }
    })
    .await
    .expect("exact authenticated egress deadline");
}

async fn rotate(transport: &Arc<Transport>, address: AddressHash) {
    let manager = transport.iface_manager();
    let mut manager = manager.lock().await;
    let mut policy = manager.shared_config(&address).expect("configured policy").clone();
    policy.passphrase = Some(ROTATED.into());
    assert!(manager.try_set_shared_config(address, policy).expect("atomic live key rotation"));
}

async fn manage(daemon: &Arc<rns_rpc::RpcDaemon>, operation: &'static str) -> rns_rpc::RpcResponse {
    let daemon = daemon.clone();
    let response = tokio::task::spawn_blocking(move || {
        daemon.handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "manage_interface".into(),
            params: Some(json!({"name":"family-root", "operation": operation})),
        })
    })
    .await
    .expect("management worker")
    .expect("management RPC");
    if let Some(error) = &response.error {
        assert_redacted(error);
    }
    response
}

async fn apply(daemon: &Arc<rns_rpc::RpcDaemon>, records: Vec<InterfaceRecord>) {
    let mut events = daemon.subscribe_events();
    let response = daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 2,
            method: "set_interfaces".into(),
            params: Some(json!({"interfaces": records})),
        })
        .expect("public Apply");
    assert!(response.error.is_none(), "{response:?}");
    timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("native completion");
            if event.event_type == "interfaces_updated" && event.payload["complete"] == true {
                break;
            }
        }
    })
    .await
    .expect("actual Apply completion");
}

async fn listener_options(daemon: &Arc<rns_rpc::RpcDaemon>, backbone: bool) {
    let record = timeout(Duration::from_secs(3), async {
        loop {
            let record = daemon.interface_records().remove(0);
            if record.settings.as_ref().expect("settings")["_runtime"]["tcp"]["listener_status"]
                ["listener_state"]
                == "listening"
            {
                break record;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native listener status publication");
    let runtime = &record.settings.as_ref().expect("runtime")["_runtime"];
    let status = &runtime["tcp"]["listener_status"];
    assert_eq!(status["listener_state"], "listening");
    assert_eq!(status["client_mtu"], 512);
    assert_eq!(status["prefer_ipv6"], true);
    assert_eq!(status["client_liveness_enabled"], backbone);
    assert_redacted(runtime);
}

#[tokio::test]
async fn tcp_named_listener_ifac_rotation_rollback_reload_and_reattach_preserve_ipv6() {
    let root = tempfile::tempdir().expect("state");
    let mut dual_hostname = None;
    for hostname in ["localhost", "localhost.localdomain"] {
        let candidates = tokio::net::lookup_host((hostname, 0))
            .await
            .expect("loopback resolution")
            .collect::<Vec<_>>();
        if candidates.iter().all(|address| address.ip().is_loopback())
            && candidates.iter().any(|address| address.is_ipv4())
            && candidates.iter().any(|address| address.ip().is_loopback() && address.is_ipv6())
        {
            dual_hostname = Some(hostname);
            break;
        }
    }
    let hostname = dual_hostname.expect("dual-loopback resolver prerequisite");
    eprintln!("TCP family native dual-loopback resolver fixture: {hostname}");
    let reserved = TcpListener::bind((Ipv6Addr::LOCALHOST, 0))
        .await
        .expect("independent IPv6 availability prerequisite");
    let port = reserved.local_addr().expect("reserved endpoint").port();
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("unrelated endpoint").port();
    let path = root.path().join("reticulum.toml");
    let text = config("tcp_server", hostname, port, other_port, INITIAL);
    std::fs::write(&path, &text).expect("configuration file");
    let configured = DaemonConfig::from_toml(&text).expect("config");
    drop(reserved);
    let (transport, daemon, owner) = start(&configured, root.path()).await;
    let mut owner = owner.with_management_config(Some(path.clone()), true);
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    let initial = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other, 2).await;
    listener_options(&daemon, false).await;
    let mut first = connect_ipv6(port).await;
    let first_child = child(&transport, initial, &[]).await;
    authenticate(&transport, first_child, &mut first, INITIAL, ROTATED, 0x21).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x24).await;

    std::fs::write(&path, text.replace("prefer_ipv6=true", "prefer_ipv6=true,i2p_tunneled=true"))
        .expect("unsupported public listener configuration");
    assert!(manage(&daemon, "reload").await.error.is_some());
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    authenticate(&transport, first_child, &mut first, INITIAL, ROTATED, 0x18).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x1b).await;

    let mut records = daemon.interface_records();
    records[0].settings.as_mut().expect("settings")["passphrase"] = json!(ROTATED);
    apply(&daemon, records).await;
    assert_eq!(
        native_address(&daemon.interface_records()[0]),
        initial,
        "live key changes retain the native listener"
    );
    authenticate(&transport, first_child, &mut first, ROTATED, INITIAL, 0x27).await;
    let mut second = connect_ipv6(port).await;
    let second_child = child(&transport, initial, &[first_child]).await;
    authenticate(&transport, second_child, &mut second, ROTATED, INITIAL, 0x2a).await;

    let occupied = TcpListener::bind("127.0.0.1:0").await.expect("occupied replacement");
    std::fs::write(
        &path,
        config(
            "tcp_server",
            "127.0.0.1",
            occupied.local_addr().expect("occupied endpoint").port(),
            other_port,
            ATTEMPTED,
        ),
    )
    .expect("unavailable replacement config");
    let response = manage(&daemon, "reload").await;
    assert!(response
        .error
        .as_ref()
        .expect("bind failure")
        .message
        .contains("previous interface restored"));
    closed(&mut first).await;
    closed(&mut second).await;
    for old in [initial, first_child, second_child] {
        assert_eq!(transport.iface_manager().lock().await.online(&old), None);
    }
    let rollback = native_address(&daemon.interface_records()[0]);
    assert_ne!(rollback, initial);
    let mut restored = connect_ipv6(port).await;
    listener_options(&daemon, false).await;
    let rollback_child = child(&transport, rollback, &[]).await;
    authenticate(&transport, rollback_child, &mut restored, ROTATED, ATTEMPTED, 0x2d).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x30).await;

    let blocked_ipv6 = TcpListener::bind((Ipv6Addr::LOCALHOST, 0))
        .await
        .expect("occupied IPv6 fallback candidate");
    let fallback_port = blocked_ipv6.local_addr().expect("fallback endpoint").port();
    std::fs::write(&path, config("tcp_server", hostname, fallback_port, other_port, ROTATED))
        .expect("native address fallback config");
    let response = manage(&daemon, "reload").await;
    assert!(response.error.is_none(), "IPv4 fallback remains available: {response:?}");
    closed(&mut restored).await;
    for old in [rollback, rollback_child] {
        assert_eq!(transport.iface_manager().lock().await.online(&old), None);
    }
    let fallback = native_address(&daemon.interface_records()[0]);
    let mut fallback_peer = TcpStream::connect(("127.0.0.1", fallback_port))
        .await
        .expect("native IPv4 fallback after occupied IPv6 candidate");
    assert!(fallback_peer.peer_addr().expect("fallback address").is_ipv4());
    listener_options(&daemon, false).await;
    let fallback_child = child(&transport, fallback, &[]).await;
    authenticate(&transport, fallback_child, &mut fallback_peer, ROTATED, ATTEMPTED, 0x42).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x45).await;

    std::fs::write(&path, config("tcp_server", hostname, port, other_port, FINAL))
        .expect("changed-key replacement config");
    let response = manage(&daemon, "reload").await;
    assert!(response.error.is_none(), "{response:?}");
    closed(&mut fallback_peer).await;
    for old in [fallback, fallback_child] {
        assert_eq!(transport.iface_manager().lock().await.online(&old), None);
    }
    let replacement = native_address(&daemon.interface_records()[0]);
    assert_ne!(replacement, fallback);
    listener_options(&daemon, false).await;
    let mut replaced = connect_ipv6(port).await;
    let replacement_child = child(&transport, replacement, &[]).await;
    authenticate(&transport, replacement_child, &mut replaced, FINAL, ROTATED, 0x33).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x36).await;

    assert!(manage(&daemon, "detach").await.error.is_none());
    closed(&mut replaced).await;
    for old in [replacement, replacement_child] {
        assert_eq!(transport.iface_manager().lock().await.online(&old), None);
    }
    assert!(!daemon.interface_records()[0].enabled);
    assert!(TcpStream::connect((Ipv6Addr::LOCALHOST, port)).await.is_err());
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x39).await;
    assert!(manage(&daemon, "attach").await.error.is_none());
    let attached = native_address(&daemon.interface_records()[0]);
    assert_ne!(attached, replacement);
    listener_options(&daemon, false).await;
    let mut reattached = connect_ipv6(port).await;
    let attached_child = child(&transport, attached, &[]).await;
    authenticate(&transport, attached_child, &mut reattached, FINAL, ROTATED, 0x3c).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x3f).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut reattached).await;
    closed(&mut unaffected).await;
}

async fn configured_client_lifecycle(kind: &str) {
    let root = tempfile::tempdir().expect("state");
    let reserved = TcpSocket::new_v4().expect("native socket");
    reserved.bind("127.0.0.1:0".parse().expect("loopback")).expect("reserve refused endpoint");
    let port = reserved.local_addr().expect("reserved endpoint").port();
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("unrelated endpoint").port();
    let configured = DaemonConfig::from_toml(&config(kind, "127.0.0.1", port, other_port, INITIAL))
        .expect("client config");
    let (transport, daemon, mut owner) = start(&configured, root.path()).await;
    let address = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other, 2).await;
    let failed = timeout(Duration::from_secs(3), async {
        loop {
            let runtime = daemon.interface_records()[0].settings.as_ref().expect("settings")
                ["_runtime"]
                .clone();
            let status = &runtime["tcp"]["stream_status"];
            if status["stream_state"] == "reconnecting" && status["last_error"].is_string() {
                assert_redacted(&runtime);
                break status.clone();
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native refused-connect status publication");
    assert!(failed["reconnect_attempts"].as_u64().expect("attempt count") >= 1);
    assert_eq!(failed["mtu"], 512);
    assert_eq!(failed["liveness_enabled"], kind == "backbone_client");
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x51).await;

    let listener = reserved.listen(8).expect("same reserved socket becomes live");
    let mut first = accept(&listener, 8).await;
    authenticate(&transport, address, &mut first, INITIAL, ROTATED, 0x54).await;
    rotate(&transport, address).await;
    authenticate(&transport, address, &mut first, ROTATED, INITIAL, 0x57).await;
    drop(first);
    let mut reconnected = accept(&listener, 8).await;
    assert_eq!(native_address(&daemon.interface_records()[0]), address);
    authenticate(&transport, address, &mut reconnected, ROTATED, INITIAL, 0x5a).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x5d).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut reconnected).await;
    closed(&mut unaffected).await;
    let previous_identity = transport.internal_identity().await;

    let restarted = DaemonConfig::from_toml(&config(kind, "127.0.0.1", port, other_port, FINAL))
        .expect("changed-key restart config");
    let (transport, daemon, mut owner) = start(&restarted, root.path()).await;
    let current = native_address(&daemon.interface_records()[0]);
    assert_ne!(transport.internal_identity().await, previous_identity);
    let mut peer = accept(&listener, 2).await;
    authenticate(&transport, current, &mut peer, FINAL, ROTATED, 0x60).await;
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other, 2).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x63).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut peer).await;
    closed(&mut unaffected).await;
}

#[tokio::test]
async fn configured_tcp_client_ifac_refusal_live_rotation_reconnect_and_restart() {
    configured_client_lifecycle("tcp_client").await;
}

#[tokio::test]
async fn configured_backbone_client_ifac_refusal_live_rotation_reconnect_and_restart() {
    configured_client_lifecycle("backbone_client").await;
}

#[tokio::test]
async fn configured_backbone_listener_ifac_failure_child_rotation_and_restart() {
    let root = tempfile::tempdir().expect("state");
    let occupied = TcpListener::bind("127.0.0.1:0").await.expect("occupied endpoint");
    let port = occupied.local_addr().expect("endpoint").port();
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("unrelated endpoint").port();
    let configured =
        DaemonConfig::from_toml(&config("backbone", "127.0.0.1", port, other_port, INITIAL))
            .expect("Backbone config");
    let startup = start_fixture(&Args::parse_from(["reticulumd"]), &configured, root.path()).await;
    assert_eq!(startup.interfaces.startup_failures.len(), 1);
    assert_eq!(startup.interfaces.startup_failures[0].kind, "backbone");
    assert!(!startup.interfaces.startup_failures[0].error.is_empty());
    for refresh in &startup.interfaces.tcp_runtime_refreshes {
        if let TcpRuntimeStatusSource::Listener(status) = &refresh.status {
            let status = status.to_json();
            assert_eq!(status["listener_state"], "bind_error");
            assert_redacted(&status);
        }
    }
    let mut old_unrelated = accept(&other, 2).await;
    retire_fixture(startup).await;
    closed(&mut old_unrelated).await;
    drop(occupied);
    let (transport, daemon, mut owner) = start(&configured, root.path()).await;
    let parent = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other, 2).await;
    listener_options(&daemon, true).await;
    let mut first = TcpStream::connect(("127.0.0.1", port)).await.expect("accepted Backbone peer");
    let first_child = child(&transport, parent, &[]).await;
    authenticate(&transport, first_child, &mut first, INITIAL, ROTATED, 0x71).await;
    rotate(&transport, parent).await;
    authenticate(&transport, first_child, &mut first, ROTATED, INITIAL, 0x74).await;
    let mut second =
        TcpStream::connect(("127.0.0.1", port)).await.expect("new child after rotation");
    let second_child = child(&transport, parent, &[first_child]).await;
    authenticate(&transport, second_child, &mut second, ROTATED, INITIAL, 0x77).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x7a).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut first).await;
    closed(&mut second).await;
    closed(&mut unaffected).await;
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    let previous_identity = transport.internal_identity().await;

    let restarted =
        DaemonConfig::from_toml(&config("backbone", "127.0.0.1", port, other_port, FINAL))
            .expect("changed-key restart config");
    let (transport, daemon, mut owner) = start(&restarted, root.path()).await;
    let current = native_address(&daemon.interface_records()[0]);
    assert_ne!(transport.internal_identity().await, previous_identity);
    listener_options(&daemon, true).await;
    let mut peer = TcpStream::connect(("127.0.0.1", port)).await.expect("restarted listener");
    let current_child = child(&transport, current, &[]).await;
    authenticate(&transport, current_child, &mut peer, FINAL, ROTATED, 0x7d).await;
    let mut unaffected = accept(&other, 2).await;
    verify_unrelated_tcp(
        &transport,
        native_address(&daemon.interface_records()[1]),
        &mut unaffected,
        0x80,
    )
    .await;
    shutdown(&transport, &mut owner).await;
    closed(&mut peer).await;
    closed(&mut unaffected).await;
}

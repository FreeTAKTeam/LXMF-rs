use super::restore_tests::{native_address, verify_unrelated_tcp};
use super::*;
use crate::interface_hot_apply::{InterfaceHotApplyBridge, InterfaceMutationOwner};
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::iface::{
    encode_packet_ifac, IfacState, IfaceRole, InterfaceSharedConfig, TxMessage, TxMessageType,
};
use rns_transport::packet::{Packet, PacketDataBuffer};
use serde_json::json;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::{sleep, sleep_until, timeout, Duration, Instant};

const INITIAL: &str = "udp-family-initial-credential";
const ROTATED: &str = "udp-family-rotated-credential";
const ATTEMPTED: &str = "udp-family-attempted-credential";
const FINAL: &str = "udp-family-final-credential";
const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 0, 100);

fn assert_redacted(value: &impl serde::Serialize) {
    let text = serde_json::to_string(value).expect("diagnostic JSON");
    for credential in [INITIAL, ROTATED, ATTEMPTED, FINAL] {
        assert!(!text.contains(credential), "runtime diagnostics cannot expose credentials");
    }
}

fn config(port: u16, target: SocketAddr, other_port: u16, credential: &str) -> String {
    assert!(target.ip().is_loopback() || target.ip() == GROUP);
    format!("interfaces=[{{type='udp',enabled=true,name='udp-family',host='127.0.0.1',port={port},target_host='{}',target_port={},ifac_size=64,network_name='udp-family-network',passphrase='{credential}'}},{{type='tcp_client',enabled=true,name='unrelated',host='127.0.0.1',port={other_port}}}]\n[reticulum]\nshare_instance=false", target.ip(), target.port())
}

async fn start(
    text: &str,
    root: &Path,
    path: &Path,
) -> (Arc<Transport>, Arc<rns_rpc::RpcDaemon>, InterfaceMutationOwner) {
    std::fs::write(path, text).expect("configured UDP file");
    let config = DaemonConfig::from_toml(text).expect("configured UDP");
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
    )
    .with_management_config(Some(path.to_owned()), true);
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    bound(&daemon).await;
    (transport, daemon, owner)
}

async fn shutdown(transport: &Arc<Transport>, owner: &mut InterfaceMutationOwner) {
    timeout(Duration::from_secs(10), owner.shutdown())
        .await
        .expect("native cleanup bound")
        .expect("native and reconnect workers joined");
    assert!(transport.interface_hashes().await.is_empty());
}

async fn bound(daemon: &Arc<rns_rpc::RpcDaemon>) {
    timeout(Duration::from_secs(3), async {
        loop {
            let record = &daemon.interface_records()[0];
            let runtime = &record.settings.as_ref().expect("settings")["_runtime"];
            if runtime["udp"]["status"]["link_state"] == "bound" {
                assert_redacted(runtime);
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("published native UDP readiness");
}

async fn accept(listener: &TcpListener) -> TcpStream {
    timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("unrelated native connection deadline")
        .expect("unrelated native connection")
        .0
}

async fn closed(peer: &mut TcpStream) {
    timeout(Duration::from_secs(2), peer.read_to_end(&mut Vec::new()))
        .await
        .expect("unrelated EOF deadline")
        .expect("unrelated EOF");
}

fn auth(credential: &str) -> IfacState {
    Arc::new(std::sync::RwLock::new(
        InterfaceSharedConfig {
            ifac_size: Some(64),
            network_name: Some("udp-family-network".into()),
            passphrase: Some(credential.into()),
            ..Default::default()
        }
        .ifac_context()
        .expect("independent IFAC peer"),
    ))
}

fn packet(marker: u8) -> Packet {
    Packet {
        destination: AddressHash::new([marker; 16]),
        data: PacketDataBuffer::new_from_slice(&[marker, 0xc0, 0xdb, 0x7e, 0x7d]),
        ..Default::default()
    }
}

async fn reject(
    transport: &Arc<Transport>,
    root: AddressHash,
    peer: &UdpSocket,
    worker: SocketAddr,
    rejected: &str,
    packet: &Packet,
) {
    let manager = transport.iface_manager();
    let mut ingress = transport.iface_rx();
    let violations = || async {
        manager
            .lock()
            .await
            .traffic_snapshots()
            .into_iter()
            .find(|entry| entry.address == root)
            .expect("native UDP root")
            .ifac_violations
    };
    let before = violations().await;
    for wire in [
        encode_packet_ifac(&auth(rejected), packet).expect("wrong-key packet"),
        packet.to_bytes().expect("plaintext packet"),
    ] {
        assert_eq!(peer.send_to(&wire, worker).await.expect("negative UDP wire"), wire.len());
    }
    timeout(Duration::from_secs(2), async {
        while violations().await != before + 2 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("wrong key and plaintext counted on native UDP root");
    while let Ok(event) = ingress.try_recv() {
        assert_ne!(event.packet.destination, packet.destination, "rejected native ingress");
    }
}

async fn exact_datagram(peer: &UdpSocket, expected: &[u8], worker: SocketAddr) {
    timeout(Duration::from_secs(2), async {
        let mut received = [0; 4096];
        loop {
            let (length, source) = peer.recv_from(&mut received).await.expect("native UDP egress");
            assert_eq!(source, worker, "reply must use the native root socket");
            if &received[..length] == expected {
                break;
            }
        }
    })
    .await
    .expect("exact authenticated UDP datagram deadline");
}

async fn peer_isolated(other: &UdpSocket, credential: &str, marker: u8) {
    let expected = encode_packet_ifac(&auth(credential), &packet(marker + 1))
        .expect("other peer must not receive this direct reply");
    let observation = timeout(Duration::from_secs(2), async {
        let mut wire = [0; 4096];
        loop {
            let (length, _) =
                other.recv_from(&mut wire).await.expect("independent peer observation");
            assert_ne!(&wire[..length], expected, "direct reply leaked to another peer");
        }
    })
    .await;
    assert!(observation.is_err(), "complete bounded isolation observation");
}

async fn authenticate(
    transport: &Arc<Transport>,
    (root, route): (AddressHash, AddressHash),
    peer: &UdpSocket,
    worker: SocketAddr,
    (credential, rejected): (&str, &str),
    marker: u8,
) {
    let inbound = packet(marker);
    reject(transport, root, peer, worker, rejected, &inbound).await;
    let mut ingress = transport.iface_rx();
    let state = auth(credential);
    let wire = encode_packet_ifac(&state, &inbound).expect("authenticated ingress");
    assert_eq!(peer.send_to(&wire, worker).await.expect("native UDP ingress"), wire.len());
    let received = timeout(Duration::from_secs(2), async {
        loop {
            let event = ingress.recv().await.expect("native ingress event");
            if event.packet.destination == inbound.destination {
                break event;
            }
        }
    })
    .await
    .expect("authenticated UDP ingress deadline");
    assert_eq!(received.address, route, "exact native peer attribution");
    assert_eq!(received.packet.data, inbound.data);
    assert_eq!(
        received.source,
        rns_transport::iface::IfaceSource::Udp(peer.local_addr().expect("peer"))
    );
    let outbound = packet(marker + 1);
    let expected = encode_packet_ifac(&state, &outbound).expect("authenticated egress");
    assert_eq!(
        transport
            .iface_manager()
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(route), packet: outbound })
            .await
            .sent_ifaces,
        1
    );
    exact_datagram(peer, &expected, worker).await;
}

async fn apply_rotation(daemon: &Arc<rns_rpc::RpcDaemon>) {
    let mut records = daemon.interface_records();
    records[0].settings.as_mut().expect("settings")["passphrase"] = json!(ROTATED);
    let mut events = daemon.subscribe_events();
    let reply = daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(json!({"interfaces": records})),
        })
        .expect("public Apply");
    assert!(reply.error.is_none(), "{reply:?}");
    timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("mutation completion");
            if event.event_type == "interfaces_updated" && event.payload["complete"] == true {
                break;
            }
        }
    })
    .await
    .expect("actual Apply completion");
}

async fn manage(daemon: &Arc<rns_rpc::RpcDaemon>, operation: &'static str) -> rns_rpc::RpcResponse {
    let daemon = daemon.clone();
    let response = tokio::task::spawn_blocking(move || {
        daemon.handle_rpc(rns_rpc::RpcRequest {
            id: 2,
            method: "manage_interface".into(),
            params: Some(json!({"name":"udp-family", "operation":operation})),
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

async fn retired(transport: &Arc<Transport>, addresses: &[AddressHash]) {
    for address in addresses {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        assert_eq!(manager.online(address), None, "old root/child must be removed");
        assert_eq!(
            manager
                .send(TxMessage { tx_type: TxMessageType::Direct(*address), packet: packet(0xef) })
                .await
                .sent_ifaces,
            0,
            "retired hash has no dispatch authority"
        );
    }
}

#[tokio::test]
async fn udp_named_ifac_rotation_failed_reload_rollback_detach_and_reattach() {
    let root = tempfile::tempdir().expect("state");
    let path = root.path().join("reticulum.toml");
    let reserved = UdpSocket::bind("127.0.0.1:0").await.expect("reserve UDP root");
    let worker = reserved.local_addr().expect("root endpoint");
    let peer = UdpSocket::bind("127.0.0.1:0").await.expect("native peer");
    let target = peer.local_addr().expect("peer endpoint");
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("unrelated endpoint").port();
    drop(reserved);
    let (transport, daemon, mut owner) =
        start(&config(worker.port(), target, other_port, INITIAL), root.path(), &path).await;
    let initial = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    assert_eq!(transport.iface_manager().lock().await.role(&initial), Some(IfaceRole::Unicast));
    authenticate(&transport, (initial, initial), &peer, worker, (INITIAL, ROTATED), 0x11).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x13).await;
    apply_rotation(&daemon).await;
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    authenticate(&transport, (initial, initial), &peer, worker, (ROTATED, INITIAL), 0x15).await;

    let occupied = UdpSocket::bind("127.0.0.1:0").await.expect("occupied replacement");
    let occupied_port = occupied.local_addr().expect("occupied endpoint").port();
    std::fs::write(&path, config(occupied_port, target, other_port, ATTEMPTED))
        .expect("unavailable replacement configuration");
    let pending = {
        let daemon = daemon.clone();
        tokio::spawn(async move { manage(&daemon, "reload").await })
    };
    let refresh = timeout(Duration::from_secs(2), async {
        loop {
            let refresh = owner
                .bridge
                .udp_refreshes
                .lock()
                .expect("existing UDP refresh observer")
                .get("udp-family")
                .cloned();
            if let Some(refresh) = refresh.filter(|entry| entry.record.port == Some(occupied_port))
            {
                break refresh;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("replacement native status handle");
    timeout(Duration::from_secs(2), async {
        while refresh.status.snapshot().link_state != "bind_failed" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual native bind failure");
    let failed = refresh.status.snapshot();
    assert_eq!(failed.socket_errors, 1);
    assert_eq!(failed.packets_rx, 0);
    assert_redacted(&failed.to_json());
    timeout(Duration::from_secs(2), async {
        while transport.iface_manager().lock().await.online(&refresh.runtime_iface).is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("failed worker has entered owned retirement");
    retired(&transport, &[initial, refresh.runtime_iface]).await;
    assert!(!pending.is_finished(), "management must await failed worker retirement");
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x18).await;
    assert!(!pending.is_finished(), "unrelated traffic completes during failed worker cleanup");
    let response = timeout(Duration::from_secs(20), pending)
        .await
        .expect("existing management reply deadline")
        .expect("management task");
    let error = &response.error.expect("occupied replacement must fail").message;
    assert!(error.contains("state=bind_failed"), "{error}");
    assert!(error.contains("previous interface restored"), "{error}");
    assert_eq!(refresh.status.snapshot().link_state, "bind_failed");
    let rollback = native_address(&daemon.interface_records()[0]);
    assert_ne!(rollback, initial);
    bound(&daemon).await;
    authenticate(&transport, (rollback, rollback), &peer, worker, (ROTATED, ATTEMPTED), 0x1b).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x1e).await;

    std::fs::write(&path, config(worker.port(), target, other_port, FINAL))
        .expect("changed-key replacement");
    assert!(manage(&daemon, "reload").await.error.is_none());
    retired(&transport, &[rollback]).await;
    let replacement = native_address(&daemon.interface_records()[0]);
    bound(&daemon).await;
    authenticate(&transport, (replacement, replacement), &peer, worker, (FINAL, ROTATED), 0x21)
        .await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x24).await;
    assert!(manage(&daemon, "detach").await.error.is_none());
    assert!(!daemon.interface_records()[0].enabled);
    retired(&transport, &[replacement]).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x27).await;
    assert!(manage(&daemon, "attach").await.error.is_none());
    let attached = native_address(&daemon.interface_records()[0]);
    assert_ne!(attached, replacement);
    bound(&daemon).await;
    authenticate(&transport, (attached, attached), &peer, worker, (FINAL, ROTATED), 0x2a).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x2d).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut unaffected).await;
}

fn multicast_receiver() -> UdpSocket {
    let socket =
        std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("owned group receiver");
    socket.join_multicast_v4(&GROUP, &Ipv4Addr::LOCALHOST).expect("join on loopback");
    socket.set_nonblocking(true).expect("nonblocking group socket");
    UdpSocket::from_std(socket).expect("native async group receiver")
}

async fn group_egress(
    transport: &Arc<Transport>,
    unrelated: AddressHash,
    worker: SocketAddr,
    group: &UdpSocket,
    credential: &str,
    marker: u8,
) {
    let packet = packet(marker);
    let expected = encode_packet_ifac(&auth(credential), &packet).expect("group IFAC egress");
    assert_eq!(
        transport
            .iface_manager()
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Broadcast(Some(unrelated)), packet })
            .await
            .sent_ifaces,
        1,
        "only physical UDP root carries the broadcast"
    );
    exact_datagram(group, &expected, worker).await;
}

async fn learn_peer(
    transport: &Arc<Transport>,
    root: AddressHash,
    peer: &UdpSocket,
    worker: SocketAddr,
    (credential, rejected): (&str, &str),
    next_announce: &mut Instant,
    destination: &mut SingleInputDestination,
) -> (AddressHash, AddressHash) {
    // Respect the native 3.5 Hz new-interface ingress guard without changing policy.
    sleep_until(*next_announce).await;
    let announce = destination.announce(rand_core::OsRng, None).expect("fresh signed announce");
    let children_before = transport.interface_hashes().await;
    let previous_route = transport.path_status(&announce.destination).await.interface;
    reject(transport, root, peer, worker, rejected, &announce).await;
    assert_eq!(transport.path_status(&announce.destination).await.interface, previous_route);
    assert_eq!(transport.interface_hashes().await, children_before, "no unauthenticated peer");
    let wire = encode_packet_ifac(&auth(credential), &announce).expect("authenticated announce");
    assert_eq!(peer.send_to(&wire, worker).await.expect("signed native ingress"), wire.len());
    *next_announce = Instant::now() + Duration::from_secs(1);
    let route = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(route) = transport.path_status(&announce.destination).await.interface {
                let current = transport
                    .iface_manager()
                    .lock()
                    .await
                    .traffic_snapshots()
                    .into_iter()
                    .any(|entry| entry.address == route && entry.parent == Some(root));
                if current {
                    break route;
                }
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("signed announce creates native peer route");
    let manager = transport.iface_manager();
    let mut manager = manager.lock().await;
    assert_eq!(manager.role(&route), Some(IfaceRole::VirtualUnicast));
    assert_eq!(
        manager
            .traffic_snapshots()
            .into_iter()
            .find(|entry| entry.address == route)
            .expect("native peer")
            .parent,
        Some(root)
    );
    (route, announce.destination)
}

#[tokio::test]
async fn udp_multicast_ifac_peer_rotation_replacement_and_configured_restart() {
    let root = tempfile::tempdir().expect("state");
    let path = root.path().join("reticulum.toml");
    let reserved = UdpSocket::bind("127.0.0.1:0").await.expect("reserve UDP root");
    let worker = reserved.local_addr().expect("root endpoint");
    let group = multicast_receiver();
    let target = SocketAddr::from((GROUP, group.local_addr().expect("group endpoint").port()));
    let first = UdpSocket::bind("127.0.0.1:0").await.expect("first peer");
    let second = UdpSocket::bind("127.0.0.1:0").await.expect("second peer");
    assert_ne!(first.local_addr().expect("peer"), second.local_addr().expect("peer"));
    let other = TcpListener::bind("127.0.0.1:0").await.expect("unrelated peer");
    let other_port = other.local_addr().expect("unrelated endpoint").port();
    drop(reserved);
    let text = config(worker.port(), target, other_port, INITIAL);
    let (transport, daemon, mut owner) = start(&text, root.path(), &path).await;
    let initial = native_address(&daemon.interface_records()[0]);
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    let mut next_announce = Instant::now();
    let identity = || {
        SingleInputDestination::new(
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng),
            DestinationName::new("lxmf", "delivery"),
        )
    };
    let mut first_identity = identity();
    let mut second_identity = identity();
    assert_eq!(transport.iface_manager().lock().await.role(&initial), Some(IfaceRole::Multicast));
    group_egress(&transport, unrelated, worker, &group, INITIAL, 0x31).await;
    let (first_route, first_destination) = learn_peer(
        &transport,
        initial,
        &first,
        worker,
        (INITIAL, ROTATED),
        &mut next_announce,
        &mut first_identity,
    )
    .await;
    authenticate(&transport, (initial, first_route), &first, worker, (INITIAL, ROTATED), 0x33)
        .await;
    peer_isolated(&second, INITIAL, 0x33).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x36).await;
    apply_rotation(&daemon).await;
    assert_eq!(native_address(&daemon.interface_records()[0]), initial);
    authenticate(&transport, (initial, first_route), &first, worker, (ROTATED, INITIAL), 0x39)
        .await;
    peer_isolated(&second, ROTATED, 0x39).await;
    let (second_route, second_destination) = learn_peer(
        &transport,
        initial,
        &second,
        worker,
        (ROTATED, INITIAL),
        &mut next_announce,
        &mut second_identity,
    )
    .await;
    assert_ne!(second_route, first_route, "distinct peer source ports have independent routes");
    authenticate(&transport, (initial, second_route), &second, worker, (ROTATED, INITIAL), 0x3c)
        .await;
    peer_isolated(&first, ROTATED, 0x3c).await;
    group_egress(&transport, unrelated, worker, &group, ROTATED, 0x3f).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x42).await;

    let changed = config(worker.port(), target, other_port, FINAL);
    std::fs::write(&path, &changed).expect("changed-key multicast replacement");
    assert!(manage(&daemon, "reload").await.error.is_none());
    retired(&transport, &[initial, first_route, second_route]).await;
    let replacement = native_address(&daemon.interface_records()[0]);
    assert_ne!(replacement, initial);
    bound(&daemon).await;
    let (current_first, refreshed_first) = learn_peer(
        &transport,
        replacement,
        &first,
        worker,
        (FINAL, ROTATED),
        &mut next_announce,
        &mut first_identity,
    )
    .await;
    let (current_second, refreshed_second) = learn_peer(
        &transport,
        replacement,
        &second,
        worker,
        (FINAL, ROTATED),
        &mut next_announce,
        &mut second_identity,
    )
    .await;
    assert_ne!(current_first, current_second);
    assert_eq!((refreshed_first, refreshed_second), (first_destination, second_destination));
    assert_ne!(current_first, first_route);
    assert_ne!(current_second, second_route);
    authenticate(&transport, (replacement, current_first), &first, worker, (FINAL, ROTATED), 0x45)
        .await;
    peer_isolated(&second, FINAL, 0x45).await;
    authenticate(
        &transport,
        (replacement, current_second),
        &second,
        worker,
        (FINAL, ROTATED),
        0x48,
    )
    .await;
    peer_isolated(&first, FINAL, 0x48).await;
    group_egress(&transport, unrelated, worker, &group, FINAL, 0x4b).await;
    assert_eq!(native_address(&daemon.interface_records()[1]), unrelated);
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x4e).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut unaffected).await;
    let previous_identity = transport.internal_identity().await;

    let restarted = config(worker.port(), target, other_port, ATTEMPTED);
    let (transport, daemon, mut owner) = start(&restarted, root.path(), &path).await;
    assert_ne!(transport.internal_identity().await, previous_identity);
    let parent = native_address(&daemon.interface_records()[0]);
    let (current_first, refreshed_first) = learn_peer(
        &transport,
        parent,
        &first,
        worker,
        (ATTEMPTED, FINAL),
        &mut next_announce,
        &mut first_identity,
    )
    .await;
    let (current_second, refreshed_second) = learn_peer(
        &transport,
        parent,
        &second,
        worker,
        (ATTEMPTED, FINAL),
        &mut next_announce,
        &mut second_identity,
    )
    .await;
    assert_ne!(current_first, current_second);
    assert_eq!((refreshed_first, refreshed_second), (first_destination, second_destination));
    authenticate(&transport, (parent, current_first), &first, worker, (ATTEMPTED, FINAL), 0x51)
        .await;
    peer_isolated(&second, ATTEMPTED, 0x51).await;
    authenticate(&transport, (parent, current_second), &second, worker, (ATTEMPTED, FINAL), 0x54)
        .await;
    peer_isolated(&first, ATTEMPTED, 0x54).await;
    let unrelated = native_address(&daemon.interface_records()[1]);
    let mut unaffected = accept(&other).await;
    group_egress(&transport, unrelated, worker, &group, ATTEMPTED, 0x57).await;
    verify_unrelated_tcp(&transport, unrelated, &mut unaffected, 0x5a).await;
    shutdown(&transport, &mut owner).await;
    closed(&mut unaffected).await;
}

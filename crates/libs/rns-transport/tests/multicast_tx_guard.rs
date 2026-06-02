//! Wire-level integration tests for the multicast UDP iface.
//!
//! Two properties we want to guarantee end-to-end, against real UDP
//! sockets (no mocks):
//!
//!   1. tx-guard — `TxMessageType::Direct` targeting the multicast
//!      iface's own `AddressHash` never reaches the wire. Otherwise a
//!      Link keepalive / Data packet routed "directly at the multicast
//!      iface" would flood the whole group, which is the bug that
//!      originally brought us here.
//!
//!   2. per-peer unicast routing — `TxMessageType::Direct` targeting a
//!      *virtual* iface hash registered in the host multicast iface's
//!      `PeerRouting` map goes out as a plain UDP unicast from the
//!      same physical socket that carries multicast broadcasts. The
//!      source port is the multicast port (4242-alike), which is
//!      exactly what we want so peers attribute unicast replies to the
//!      same virtual iface they attributed the original announce to.
//!
//! The tests run against IPv4 `224.0.0.100`, link-local scope, which
//! works on stock Linux/macOS loopback without any routing setup.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use rns_transport::iface::udp::spawn_multicast_udp;
use rns_transport::iface::{IfaceRole, InterfaceManager, TxMessage, TxMessageType};
use rns_transport::packet::Packet;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::timeout;

const MCAST_GROUP: &str = "224.0.0.100";

fn bind_listener(port: u16, join_mcast: bool) -> UdpSocket {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .expect("create listener socket");
    socket.set_reuse_address(true).expect("SO_REUSEADDR");
    #[cfg(unix)]
    socket.set_reuse_port(true).expect("SO_REUSEPORT");
    let bind_any: SocketAddr = (Ipv4Addr::UNSPECIFIED, port).into();
    socket.bind(&bind_any.into()).expect("bind listener");
    if join_mcast {
        let group: Ipv4Addr = MCAST_GROUP.parse().expect("MCAST_GROUP parses");
        socket.join_multicast_v4(&group, &Ipv4Addr::LOCALHOST).expect("join mcast group");
    }
    socket.set_nonblocking(true).expect("nonblocking");
    let std_sock: std::net::UdpSocket = socket.into();
    UdpSocket::from_std(std_sock).expect("into tokio")
}

/// Pick an unused UDP port by briefly binding + closing.
fn pick_free_port() -> u16 {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").expect("probe port");
    let port = probe.local_addr().expect("probe local_addr").port();
    drop(probe);
    port
}

#[tokio::test]
async fn broadcast_tx_reaches_multicast_listeners() {
    let port = pick_free_port();
    let group_addr = format!("{}:{}", MCAST_GROUP, port);
    let bind_addr = format!("127.0.0.1:{}", port);
    let listener = bind_listener(port, true);

    let mut mgr = InterfaceManager::new(64);
    let (_iface_hash, _routing) =
        spawn_multicast_udp(&mut mgr, bind_addr.clone(), Some(group_addr.clone()));
    let mgr = Arc::new(Mutex::new(mgr));

    // Give the tx task a moment to bind and join the group.
    tokio::time::sleep(Duration::from_millis(150)).await;

    mgr.lock()
        .await
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
        .await;

    let mut rx_buf = [0u8; 2048];
    let result = timeout(Duration::from_millis(500), listener.recv_from(&mut rx_buf)).await;
    assert!(result.is_ok(), "multicast listener expected to see the Broadcast tx within 500ms");
}

#[tokio::test]
async fn direct_tx_targeting_multicast_iface_is_dropped() {
    let port = pick_free_port();
    let group_addr = format!("{}:{}", MCAST_GROUP, port);
    let bind_addr = format!("127.0.0.1:{}", port);
    let listener = bind_listener(port, true);

    let mut mgr = InterfaceManager::new(64);
    let (iface_hash, _routing) =
        spawn_multicast_udp(&mut mgr, bind_addr.clone(), Some(group_addr.clone()));
    let mgr = Arc::new(Mutex::new(mgr));

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Drain any stray packets that may have been queued while the
    // socket was still binding / joining the group.
    let mut rx_buf = [0u8; 2048];
    let _ = timeout(Duration::from_millis(150), listener.recv_from(&mut rx_buf)).await;

    // Direct tx targeting the multicast iface's own AddressHash is
    // nonsensical (would send the packet to the multicast group the
    // iface is joined to), so the tx-guard drops it.
    mgr.lock()
        .await
        .send(TxMessage { tx_type: TxMessageType::Direct(iface_hash), packet: Packet::default() })
        .await;

    let result = timeout(Duration::from_millis(400), listener.recv_from(&mut rx_buf)).await;
    assert!(
        result.is_err(),
        "multicast listener must NOT see the Direct tx targeting the multicast iface itself; \
         saw {:?}",
        result
    );
}

#[tokio::test]
async fn direct_tx_to_registered_virtual_iface_is_sent_unicast() {
    // Two sockets, two ports:
    //   - multicast iface in the manager, using `mcast_port`
    //   - unicast listener at `peer_port`, NOT joined to the group
    //
    // We register the unicast listener's SocketAddr in the iface's
    // PeerRouting under a virtual iface hash, then send a Direct tx
    // to that virtual hash. The listener should receive it as a
    // plain unicast UDP packet.
    let mcast_port = pick_free_port();
    let peer_port = pick_free_port();
    assert_ne!(mcast_port, peer_port);

    let group_addr = format!("{}:{}", MCAST_GROUP, mcast_port);
    let bind_addr = format!("127.0.0.1:{}", mcast_port);
    let peer_listener = bind_listener(peer_port, false);
    let peer_addr: SocketAddr = format!("127.0.0.1:{}", peer_port).parse().unwrap();

    let mut mgr = InterfaceManager::new(64);
    let (host_hash, routing) =
        spawn_multicast_udp(&mut mgr, bind_addr.clone(), Some(group_addr.clone()));

    // Register a virtual iface pinned to the peer.
    let virtual_hash = mgr
        .register_virtual_iface(host_hash, IfaceRole::VirtualUnicast)
        .expect("register virtual iface");
    routing.lock().await.insert(peer_addr, virtual_hash);

    let mgr = Arc::new(Mutex::new(mgr));
    tokio::time::sleep(Duration::from_millis(150)).await;

    mgr.lock()
        .await
        .send(TxMessage { tx_type: TxMessageType::Direct(virtual_hash), packet: Packet::default() })
        .await;

    let mut rx_buf = [0u8; 2048];
    let (n, from) = timeout(Duration::from_millis(500), peer_listener.recv_from(&mut rx_buf))
        .await
        .expect("peer listener expected to see the unicast tx")
        .expect("recv_from succeeded");
    assert!(n > 0, "unicast tx should carry a non-empty serialized packet");
    // Source port should be the multicast port — that's the whole
    // point of routing unicast sends through the multicast socket.
    assert_eq!(
        from.port(),
        mcast_port,
        "unicast source port should equal the multicast port so peers \
         attribute the reply to the same iface they attributed the announce to"
    );
}

#[tokio::test]
async fn direct_tx_to_unknown_virtual_iface_is_dropped() {
    use rns_transport::hash::{AddressHash, Hash};

    let port = pick_free_port();
    let group_addr = format!("{}:{}", MCAST_GROUP, port);
    let bind_addr = format!("127.0.0.1:{}", port);
    let listener = bind_listener(port, true);

    let mut mgr = InterfaceManager::new(64);
    let (_host_hash, _routing) =
        spawn_multicast_udp(&mut mgr, bind_addr.clone(), Some(group_addr.clone()));
    let mgr = Arc::new(Mutex::new(mgr));

    tokio::time::sleep(Duration::from_millis(150)).await;

    // An AddressHash the iface has never heard of.
    let bogus: AddressHash = AddressHash::new_from_hash(&Hash::new_from_slice(&[0xAAu8; 32]));

    // InterfaceManager::send only enqueues to ifaces whose AddressHash
    // matches — an unregistered hash hits no ifaces and no tx task
    // runs. (The separate unit tests in iface/udp.rs cover the case
    // where the hash *does* reach the tx task but isn't in
    // PeerRouting; ensuring matching + routing both drop is
    // belt-and-suspenders.)
    mgr.lock()
        .await
        .send(TxMessage { tx_type: TxMessageType::Direct(bogus), packet: Packet::default() })
        .await;

    let mut rx_buf = [0u8; 2048];
    let result = timeout(Duration::from_millis(300), listener.recv_from(&mut rx_buf)).await;
    assert!(result.is_err(), "nothing should be sent for an unknown iface hash");
}

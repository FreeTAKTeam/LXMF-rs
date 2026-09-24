#[path = "support/python_channel_events.rs"]
#[allow(dead_code)]
mod python_channel_events;
#[path = "support/python_channel_process.rs"]
#[allow(dead_code)]
mod python_channel_process;

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use python_channel_events::wait_for_in_link_active_with_announces;
use python_channel_process::{
    python_channel_interop_paths, python_interop_guard,
    write_python_client_config_with_ifac_credentials, ChildGuard, IFAC_NETWORK_NAME,
    IFAC_PASSPHRASE, IFAC_SIZE_BITS,
};
use rand_core::OsRng;
use rns_core::identity::PrivateIdentity;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::hash::AddressHash;
use rns_transport::identity_bridge::to_transport_private_identity;
use rns_transport::iface::tcp_server::{TcpListenerRuntimeStatusHandle, TcpServer};
use rns_transport::iface::{InterfaceManager, InterfaceSharedConfig};
use rns_transport::transport::{Transport, TransportConfig};

const ROTATED_NETWORK: &str = "lxmf-rs-issue-608-rotated";
const ROTATED_PASSPHRASE: &str = "lxmf-rs-issue-608-rotated-secret";
const WRONG_PASSPHRASE: &str = "lxmf-rs-issue-608-wrong-secret";

fn reserve_loopback_listener_port() -> (u16, std::net::TcpListener) {
    // Stay below the common OS ephemeral-source-port range so unrelated
    // outbound test traffic cannot claim the port after this reservation is
    // released and before TcpServer binds it.
    for port in 20_000..30_000 {
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => return (port, listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(error) => panic!("reserve loopback TCP port {port}: {error}"),
        }
    }
    panic!("no available loopback TCP port in the test range 20000..30000");
}

fn config_for(network_name: &str, passphrase: &str) -> InterfaceSharedConfig {
    InterfaceSharedConfig {
        ifac_size: Some(IFAC_SIZE_BITS),
        network_name: Some(network_name.to_string()),
        passphrase: Some(passphrase.to_string()),
        ..InterfaceSharedConfig::default()
    }
}

fn write_client_config(dir: &std::path::Path, port: u16, network: &str, key: &str) {
    fs::create_dir_all(dir).expect("create Python client config directory");
    write_python_client_config_with_ifac_credentials(dir, port, network, key);
}

async fn start_rust_tcp_server(
    identity: &rns_transport::identity::PrivateIdentity,
    port: u16,
    shared: InterfaceSharedConfig,
) -> (
    Transport,
    AddressHash,
    Arc<tokio::sync::Mutex<InterfaceManager>>,
    TcpListenerRuntimeStatusHandle,
) {
    let transport = Transport::new(TransportConfig::new("issue-608-ifac-tcp", identity, true));
    let manager = transport.iface_manager();
    let (address, status) = {
        let mut guard = manager.lock().await;
        let interface = TcpServer::new(format!("127.0.0.1:{port}"), manager.clone());
        let status = interface.runtime_status_handle();
        let address = guard.spawn(interface, TcpServer::spawn);
        assert!(guard.try_set_shared_config(address, shared).expect("configure TCP listener IFAC"));
        (address, status)
    };
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let status = status.to_json();
            if status["listener_state"] == "listening" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("Rust TCP IFAC listener did not reach listening state: {}", status.to_json())
    });
    (transport, address, manager, status)
}

async fn wait_for_loopback_port_release(port: u16) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) {
                drop(listener);
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("TCP port {port} was not released after stopping its interface"));
}

async fn create_destination(
    transport: &Transport,
    identity: &rns_transport::identity::PrivateIdentity,
) -> (Arc<tokio::sync::Mutex<SingleInputDestination>>, String) {
    let destination =
        transport.add_destination(identity.clone(), DestinationName::new("test", "channel")).await;
    let hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };
    (destination, hash)
}

async fn wait_for_inbound_link(
    transport: &Transport,
    destination: &Arc<tokio::sync::Mutex<SingleInputDestination>>,
) -> AddressHash {
    let mut events = transport.in_link_events();
    wait_for_in_link_active_with_announces(
        transport,
        destination,
        &mut events,
        Duration::from_secs(8),
    )
    .await
}

async fn assert_no_admitted_traffic(
    transport: &Transport,
    listener_status: &TcpListenerRuntimeStatusHandle,
    baseline_links: usize,
    before: &serde_json::Value,
    duration: Duration,
) {
    tokio::time::sleep(duration).await;
    assert_eq!(
        transport.link_count().await,
        baseline_links,
        "unauthenticated TCP peer routed a Link"
    );
    let status = listener_status.to_json();
    assert!(
        status["accepted_connections"].as_u64().unwrap_or_default()
            > before["accepted_connections"].as_u64().unwrap_or_default(),
        "wrong-key peer did not reach TCP listener: {status}"
    );
    assert!(
        status["latest_stream_status"]["bytes_rx"].as_u64().unwrap_or_default() > 0,
        "wrong-key peer sent no frame to listener: {status}"
    );
}

#[tokio::test]
#[ignore = "requires pinned local Python Reticulum checkout"]
async fn python_tcp_ifac_accepted_child_rotation_and_restart_fail_closed() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let temp = tempfile::tempdir().expect("temporary fixture directory");
    let (server_port, port_reservation) = reserve_loopback_listener_port();
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let identity = to_transport_private_identity(&identity);
    let old = config_for(IFAC_NETWORK_NAME, IFAC_PASSPHRASE);
    let rotated = config_for(ROTATED_NETWORK, ROTATED_PASSPHRASE);

    // The correct Python peer's initial announce is the first real frame on
    // an accepted TCP child. Link activation proves that child inherited IFAC
    // before its worker admitted and routed that frame.
    drop(port_reservation);
    let (transport, server, manager, listener_status) =
        start_rust_tcp_server(&identity, server_port, old.clone()).await;
    let (destination, destination_hash) = create_destination(&transport, &identity).await;
    let good_dir = temp.path().join("old-key-client");
    write_client_config(&good_dir, server_port, IFAC_NETWORK_NAME, IFAC_PASSPHRASE);
    let good = ChildGuard {
        child: Some(paths.spawn_channel_client(&good_dir, &destination_hash, "channel")),
    };
    let good_link = wait_for_inbound_link(&transport, &destination).await;
    assert!(listener_status.to_json()["accepted_connections"].as_u64().unwrap_or_default() >= 1);

    // Wrong-key peers must be rejected before even an announce reaches the
    // transport routing tables.
    let baseline_links = transport.link_count().await;
    let before_wrong = listener_status.to_json();
    let wrong_dir = temp.path().join("wrong-key-client");
    write_client_config(&wrong_dir, server_port, IFAC_NETWORK_NAME, WRONG_PASSPHRASE);
    let wrong = ChildGuard {
        child: Some(paths.spawn_channel_client(&wrong_dir, &destination_hash, "channel")),
    };
    assert_no_admitted_traffic(
        &transport,
        &listener_status,
        baseline_links,
        &before_wrong,
        Duration::from_secs(2),
    )
    .await;
    drop(wrong);

    // Rotate through the production InterfaceManager operation. Existing
    // accepted children inherit the new context; old-key reconnect attempts
    // stay rejected while a new-key peer is admitted.
    assert!(manager
        .lock()
        .await
        .try_set_shared_config(server, rotated.clone())
        .expect("rotate listener IFAC credentials"));
    let rotated_wrong_dir = temp.path().join("rotated-old-key-client");
    write_client_config(&rotated_wrong_dir, server_port, IFAC_NETWORK_NAME, IFAC_PASSPHRASE);
    let rotated_wrong = ChildGuard {
        child: Some(paths.spawn_channel_client(&rotated_wrong_dir, &destination_hash, "channel")),
    };
    let rotated_baseline = transport.link_count().await;
    let before_rotated_wrong = listener_status.to_json();
    assert_no_admitted_traffic(
        &transport,
        &listener_status,
        rotated_baseline,
        &before_rotated_wrong,
        Duration::from_secs(2),
    )
    .await;
    drop(rotated_wrong);

    let new_dir = temp.path().join("rotated-key-client");
    write_client_config(&new_dir, server_port, ROTATED_NETWORK, ROTATED_PASSPHRASE);
    let new_peer = ChildGuard {
        child: Some(paths.spawn_channel_client(&new_dir, &destination_hash, "channel")),
    };
    let new_link = wait_for_inbound_link(&transport, &destination).await;
    assert_ne!(new_link, good_link, "rotated peer did not establish a fresh Link");

    // Restart the listener with rotated configuration and repeat both sides
    // of the admission boundary.
    drop(new_peer);
    drop(good);
    assert!(transport.stop_interface(server).await, "stop original IFAC listener");
    drop(transport);
    wait_for_loopback_port_release(server_port).await;
    let (restarted, restarted_server, _restarted_manager, restarted_status) =
        start_rust_tcp_server(&identity, server_port, rotated).await;
    let (restarted_destination, restarted_hash) = create_destination(&restarted, &identity).await;
    let before_restart_wrong = restarted_status.to_json();
    let restart_wrong_dir = temp.path().join("restart-old-key-client");
    write_client_config(&restart_wrong_dir, server_port, IFAC_NETWORK_NAME, IFAC_PASSPHRASE);
    let restart_wrong = ChildGuard {
        child: Some(paths.spawn_channel_client(&restart_wrong_dir, &restarted_hash, "channel")),
    };
    assert_no_admitted_traffic(
        &restarted,
        &restarted_status,
        0,
        &before_restart_wrong,
        Duration::from_secs(2),
    )
    .await;
    drop(restart_wrong);

    let restart_good_dir = temp.path().join("restart-rotated-key-client");
    write_client_config(&restart_good_dir, server_port, ROTATED_NETWORK, ROTATED_PASSPHRASE);
    let restart_good = ChildGuard {
        child: Some(paths.spawn_channel_client(&restart_good_dir, &restarted_hash, "channel")),
    };
    wait_for_inbound_link(&restarted, &restarted_destination).await;
    let restart_snapshot = restarted
        .interface_traffic_snapshots()
        .await
        .into_iter()
        .find(|snapshot| snapshot.address == restarted_server)
        .expect("listener snapshot after restart peer");
    assert!(restart_snapshot.rx_bytes > 0);
    assert_eq!(restart_snapshot.ifac_violations, 0);
    drop(restart_good);
    assert!(restarted.stop_interface(restarted_server).await, "stop restarted IFAC listener");
}

use super::announce_worker::spawn_announce_worker;
use super::bridge::{PeerCrypto, TransportBridge};
use super::inbound_worker::spawn_inbound_worker;
use super::receipt_worker::spawn_receipt_worker;
use super::Args;
use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::iface::tcp_client::TcpClient;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::rpc::{AnnounceBridge, InterfaceRecord, OutboundBridge, RpcDaemon};
use reticulum::storage::messages::MessagesStore;
use reticulum::transport::{Transport, TransportConfig};
use reticulum_daemon::announce_names::{
    encode_delivery_display_name_app_data, normalize_display_name,
};
use reticulum_daemon::config::DaemonConfig;
use reticulum_daemon::identity_store::load_or_create_identity;
use reticulum_daemon::receipt_bridge::ReceiptBridge;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::unbounded_channel;

pub(super) struct BootstrapContext {
    pub(super) rpc_addr: SocketAddr,
    pub(super) daemon: Rc<RpcDaemon>,
}

pub(super) async fn bootstrap(args: Args) -> BootstrapContext {
    let rpc_addr: SocketAddr = args.rpc.parse().expect("invalid rpc address");
    let store = MessagesStore::open(&args.db).expect("open sqlite");

    let identity_path = args.identity.clone().unwrap_or_else(|| {
        let mut path = args.db.clone();
        path.set_extension("identity");
        path
    });
    let identity = load_or_create_identity(&identity_path).expect("load identity");
    let identity_hash = hex::encode(identity.address_hash().as_slice());
    let local_display_name =
        std::env::var("LXMF_DISPLAY_NAME").ok().and_then(|value| normalize_display_name(&value));
    let daemon_config = args.config.as_ref().and_then(|path| match DaemonConfig::from_path(path) {
        Ok(config) => Some(config),
        Err(err) => {
            eprintln!("[daemon] failed to load config {}: {}", path.display(), err);
            None
        }
    });
    let mut configured_interfaces = daemon_config
        .as_ref()
        .map(|config| {
            config
                .interfaces
                .iter()
                .map(|iface| InterfaceRecord {
                    kind: iface.kind.clone(),
                    enabled: iface.enabled.unwrap_or(false),
                    host: iface.host.clone(),
                    port: iface.port,
                    name: iface.name.clone(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut transport: Option<Arc<Transport>> = None;
    let peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> = None;
    let mut delivery_destination_hash_hex: Option<String> = None;
    let mut delivery_source_hash = [0u8; 16];
    let receipt_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let (receipt_tx, receipt_rx) = unbounded_channel();

    if let Some(addr) = args.transport.clone() {
        let config = TransportConfig::new("daemon", &identity, true);
        let mut transport_instance = Transport::new(config);
        transport_instance
            .set_receipt_handler(Box::new(ReceiptBridge::new(
                receipt_map.clone(),
                receipt_tx.clone(),
            )))
            .await;
        let iface_manager = transport_instance.iface_manager();
        let server_iface = iface_manager
            .lock()
            .await
            .spawn(TcpServer::new(addr.clone(), iface_manager.clone()), TcpServer::spawn);
        eprintln!("[daemon] tcp_server enabled iface={} bind={}", server_iface, addr);
        if let Some(config) = daemon_config.as_ref() {
            for (host, port) in config.tcp_client_endpoints() {
                let endpoint = format!("{}:{}", host, port);
                let client_iface =
                    iface_manager.lock().await.spawn(TcpClient::new(endpoint), TcpClient::spawn);
                eprintln!(
                    "[daemon] tcp_client enabled iface={} name={} host={} port={}",
                    client_iface, host, host, port
                );
            }
        }
        eprintln!("[daemon] transport enabled");
        if let Some((host, port)) = addr.rsplit_once(':') {
            configured_interfaces.push(InterfaceRecord {
                kind: "tcp_server".into(),
                enabled: true,
                host: Some(host.to_string()),
                port: port.parse::<u16>().ok(),
                name: Some("daemon-transport".into()),
            });
        }

        let destination = transport_instance
            .add_destination(identity.clone(), DestinationName::new("lxmf", "delivery"))
            .await;
        {
            let dest = destination.lock().await;
            delivery_source_hash.copy_from_slice(dest.desc.address_hash.as_slice());
            delivery_destination_hash_hex = Some(hex::encode(dest.desc.address_hash.as_slice()));
            println!(
                "[daemon] delivery destination hash={}",
                hex::encode(dest.desc.address_hash.as_slice())
            );
        }
        announce_destination = Some(destination);
        transport = Some(Arc::new(transport_instance));
    }

    let bridge: Option<Arc<TransportBridge>> =
        transport.as_ref().zip(announce_destination.as_ref()).map(|(transport, destination)| {
            Arc::new(TransportBridge::new(
                transport.clone(),
                identity.clone(),
                delivery_source_hash,
                destination.clone(),
                local_display_name
                    .as_ref()
                    .and_then(|display_name| encode_delivery_display_name_app_data(display_name)),
                peer_crypto.clone(),
                receipt_map.clone(),
                receipt_tx.clone(),
            ))
        });

    let outbound_bridge: Option<Arc<dyn OutboundBridge>> =
        bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn OutboundBridge>);
    let announce_bridge: Option<Arc<dyn AnnounceBridge>> =
        bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn AnnounceBridge>);

    let daemon = Rc::new(RpcDaemon::with_store_and_bridges(
        store,
        identity_hash,
        outbound_bridge,
        announce_bridge,
    ));
    daemon.set_delivery_destination_hash(delivery_destination_hash_hex);
    daemon.replace_interfaces(configured_interfaces);
    daemon.set_propagation_state(transport.is_some(), None, 0);

    // Make the local delivery destination visible on startup.
    if let Some(bridge) = bridge.as_ref() {
        let _ = bridge.announce_now();
    }

    if transport.is_some() {
        spawn_receipt_worker(daemon.clone(), receipt_rx);
    }

    if args.announce_interval_secs > 0 {
        let _handle = daemon.clone().start_announce_scheduler(args.announce_interval_secs);
    }

    if let Some(transport) = transport {
        spawn_inbound_worker(daemon.clone(), transport.clone());
        spawn_announce_worker(daemon.clone(), transport, peer_crypto);
    }

    BootstrapContext { rpc_addr, daemon }
}

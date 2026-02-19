use super::bridge::{PeerCrypto, TransportBridge};
use super::bridge_helpers::{diagnostics_enabled, log_delivery_trace, payload_preview};
use super::Args;
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::iface::tcp_client::TcpClient;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::rpc::{AnnounceBridge, InterfaceRecord, OutboundBridge, RpcDaemon};
use reticulum::storage::messages::MessagesStore;
use reticulum::time::now_epoch_secs_i64;
use reticulum::transport::{Transport, TransportConfig};
use reticulum_daemon::announce_names::{
    encode_delivery_display_name_app_data, normalize_display_name, parse_peer_name_from_app_data,
};
use reticulum_daemon::config::DaemonConfig;
use reticulum_daemon::identity_store::load_or_create_identity;
use reticulum_daemon::inbound_delivery::{
    decode_inbound_payload, decode_inbound_payload_with_diagnostics,
};
use reticulum_daemon::receipt_bridge::{handle_receipt_event, ReceiptBridge, ReceiptEvent};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

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
        spawn_transport_workers(daemon.clone(), transport, peer_crypto);
    }

    BootstrapContext { rpc_addr, daemon }
}

fn spawn_receipt_worker(daemon: Rc<RpcDaemon>, mut receipt_rx: UnboundedReceiver<ReceiptEvent>) {
    let daemon_receipts = daemon;
    tokio::task::spawn_local(async move {
        while let Some(event) = receipt_rx.recv().await {
            let message_id = event.message_id.clone();
            let status = event.status.clone();
            let detail = format!("status={status}");
            log_delivery_trace(&message_id, "-", "receipt-update", &detail);
            let result = handle_receipt_event(&daemon_receipts, event);
            if let Err(err) = result {
                let detail = format!("persist-failed err={err}");
                log_delivery_trace(&message_id, "-", "receipt-persist", &detail);
            } else {
                log_delivery_trace(&message_id, "-", "receipt-persist", "ok");
            }
        }
    });
}

fn spawn_transport_workers(
    daemon: Rc<RpcDaemon>,
    transport: Arc<Transport>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
) {
    let daemon_inbound = daemon.clone();
    let inbound_transport = transport.clone();
    tokio::task::spawn_local(async move {
        let mut rx = inbound_transport.received_data_events();
        loop {
            if let Ok(event) = rx.recv().await {
                let data = event.data.as_slice();
                let destination_hex = hex::encode(event.destination.as_slice());
                if diagnostics_enabled() {
                    eprintln!(
                        "[daemon-rx] dst={} len={} ratchet_used={} data_prefix={}",
                        destination_hex,
                        data.len(),
                        event.ratchet_used,
                        payload_preview(data, 16)
                    );
                } else {
                    eprintln!("[daemon] rx data len={} dst={}", data.len(), destination_hex);
                }
                let mut destination = [0u8; 16];
                destination.copy_from_slice(event.destination.as_slice());
                let record = if diagnostics_enabled() {
                    let (record, diagnostics) = decode_inbound_payload_with_diagnostics(
                        destination,
                        data,
                        InboundPayloadMode::DestinationStripped,
                    );
                    if let Some(ref decoded) = record {
                        eprintln!(
                            "[daemon-rx] decoded msg_id={} src={} dst={} title_len={} content_len={}",
                            decoded.id,
                            decoded.source,
                            decoded.destination,
                            decoded.title.len(),
                            decoded.content.len()
                        );
                    } else {
                        eprintln!(
                            "[daemon-rx] decode-failed dst={} attempts={}",
                            destination_hex,
                            diagnostics.summary()
                        );
                    }
                    record
                } else {
                    decode_inbound_payload(
                        destination,
                        data,
                        InboundPayloadMode::DestinationStripped,
                    )
                };
                if let Some(record) = record {
                    let _ = daemon_inbound.accept_inbound(record);
                }
            }
        }
    });

    let daemon_announce = daemon;
    tokio::task::spawn_local(async move {
        let mut rx = transport.recv_announces().await;
        loop {
            if let Ok(event) = rx.recv().await {
                let dest = event.destination.lock().await;
                let peer = hex::encode(dest.desc.address_hash.as_slice());
                let identity = dest.desc.identity;
                let (peer_name, peer_name_source) =
                    parse_peer_name_from_app_data(event.app_data.as_slice())
                        .map(|(name, source)| (Some(name), Some(source.to_string())))
                        .unwrap_or((None, None));
                let _ratchet = event.ratchet;
                peer_crypto.lock().expect("peer map").insert(peer.clone(), PeerCrypto { identity });
                if let Some(name) = peer_name.as_ref() {
                    eprintln!("[daemon] rx announce peer={} name={}", peer, name);
                } else {
                    eprintln!("[daemon] rx announce peer={}", peer);
                }
                let timestamp = now_epoch_secs_i64();
                let _ = daemon_announce.accept_announce_with_details(
                    peer,
                    timestamp,
                    peer_name,
                    peer_name_source,
                );
            }
        }
    });
}

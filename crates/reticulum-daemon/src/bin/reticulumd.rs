#![allow(clippy::items_after_test_module)]

use clap::Parser;
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum::delivery::{send_outcome_status, LinkSendResult};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::LocalSet;

use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::destination_hash::parse_destination_hash_required;
use reticulum::hash::AddressHash;
use reticulum::identity::{Identity, PrivateIdentity};
use reticulum::iface::tcp_client::TcpClient;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use reticulum::rpc::{http, AnnounceBridge, InterfaceRecord, OutboundBridge, RpcDaemon};
use reticulum::storage::messages::MessagesStore;
use reticulum::transport::{SendPacketOutcome, SendPacketTrace, Transport, TransportConfig};
use tokio::sync::mpsc::unbounded_channel;

use reticulum_daemon::announce_names::{
    encode_delivery_display_name_app_data, normalize_display_name, parse_peer_name_from_app_data,
};
use reticulum_daemon::config::DaemonConfig;
use reticulum_daemon::direct_delivery::send_via_link;
use reticulum_daemon::identity_store::load_or_create_identity;
use reticulum_daemon::inbound_delivery::{
    decode_inbound_payload, decode_inbound_payload_with_diagnostics,
};
use reticulum_daemon::lxmf_bridge::build_wire_message;
use reticulum_daemon::receipt_bridge::{
    handle_receipt_event, track_receipt_mapping, ReceiptBridge, ReceiptEvent,
};

#[derive(Parser, Debug)]
#[command(name = "reticulumd")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,
    #[arg(long, default_value = "reticulum.db")]
    db: PathBuf,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    identity: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    announce_interval_secs: u64,
    #[arg(long)]
    transport: Option<String>,
}

struct TransportBridge {
    transport: Arc<Transport>,
    signer: PrivateIdentity,
    delivery_source_hash: [u8; 16],
    announce_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    announce_app_data: Option<Vec<u8>>,
    peer_crypto: Arc<std::sync::Mutex<HashMap<String, PeerCrypto>>>,
    receipt_map: Arc<std::sync::Mutex<HashMap<String, String>>>,
    receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
}

#[derive(Clone, Copy)]
struct PeerCrypto {
    identity: Identity,
}

impl TransportBridge {
    #[allow(clippy::too_many_arguments)]
    fn new(
        transport: Arc<Transport>,
        signer: PrivateIdentity,
        delivery_source_hash: [u8; 16],
        announce_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
        announce_app_data: Option<Vec<u8>>,
        peer_crypto: Arc<std::sync::Mutex<HashMap<String, PeerCrypto>>>,
        receipt_map: Arc<std::sync::Mutex<HashMap<String, String>>>,
        receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self {
            transport,
            signer,
            delivery_source_hash,
            announce_destination,
            announce_app_data,
            peer_crypto,
            receipt_map,
            receipt_tx,
        }
    }
}

impl OutboundBridge for TransportBridge {
    fn deliver(
        &self,
        record: &reticulum::storage::messages::MessageRecord,
        _options: &reticulum::rpc::OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        let destination = parse_destination_hash_required(&record.destination)?;
        let peer_info =
            self.peer_crypto.lock().expect("peer map").get(&record.destination).copied();
        let peer_identity = peer_info.map(|info| info.identity);

        let wire = build_wire_message(
            self.delivery_source_hash,
            destination,
            &record.title,
            &record.content,
            record.fields.clone(),
            &self.signer,
        )
        .map_err(std::io::Error::other)?;

        let payload = wire;

        let destination_hash = AddressHash::new(destination);
        let transport = self.transport.clone();
        let peer_crypto = self.peer_crypto.clone();
        let receipt_map = self.receipt_map.clone();
        let receipt_tx = self.receipt_tx.clone();
        let message_id = record.id.clone();
        let destination_hex = record.destination.clone();
        tokio::spawn(async move {
            log_delivery_trace(&message_id, &destination_hex, "start", "delivery requested");
            let mut identity = peer_identity;
            // Refresh routing for the destination before link setup.
            transport.request_path(&destination_hash, None, None).await;
            log_delivery_trace(&message_id, &destination_hex, "path-request", "requested");

            if identity.is_none() {
                log_delivery_trace(
                    &message_id,
                    &destination_hex,
                    "identity",
                    "waiting for announce",
                );
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(12);
                while tokio::time::Instant::now() < deadline {
                    if let Some(found) = transport.destination_identity(&destination_hash).await {
                        identity = Some(found);
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }

            let Some(identity) = identity else {
                log_delivery_trace(&message_id, &destination_hex, "identity", "not found");
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: "failed: peer not announced".to_string(),
                });
                return;
            };
            log_delivery_trace(&message_id, &destination_hex, "identity", "resolved");

            if let Ok(mut peers) = peer_crypto.lock() {
                peers.insert(destination_hex.clone(), PeerCrypto { identity });
            }

            let destination_desc = reticulum::destination::DestinationDesc {
                identity,
                address_hash: destination_hash,
                name: DestinationName::new("lxmf", "delivery"),
            };

            let result = send_via_link(
                transport.as_ref(),
                destination_desc,
                &payload,
                std::time::Duration::from_secs(20),
            )
            .await;
            if diagnostics_enabled() {
                let payload_starts_with_dst =
                    payload.len() >= 16 && payload[..16] == destination[..];
                let detail = format!(
                    "payload_len={} payload_prefix={} starts_with_dst={}",
                    payload.len(),
                    payload_preview(&payload, 16),
                    payload_starts_with_dst
                );
                log_delivery_trace(&message_id, &destination_hex, "payload", &detail);
            }
            match result {
                Ok(LinkSendResult::Packet(packet)) => {
                    let packet_hash = hex::encode(packet.hash().to_bytes());
                    track_receipt_mapping(&receipt_map, &packet_hash, &message_id);
                    let detail = if diagnostics_enabled() {
                        format!(
                            "packet_hash={} packet_data_len={} packet_data_prefix={}",
                            packet_hash,
                            packet.data.len(),
                            payload_preview(packet.data.as_slice(), 16)
                        )
                    } else {
                        format!("packet_hash={packet_hash}")
                    };
                    log_delivery_trace(&message_id, &destination_hex, "link", &detail);
                    let _ = receipt_tx
                        .send(ReceiptEvent { message_id, status: "sent: link".to_string() });
                }
                Ok(LinkSendResult::Resource(resource_hash)) => {
                    let detail = format!("resource_hash={}", hex::encode(resource_hash.as_slice()));
                    log_delivery_trace(&message_id, &destination_hex, "link", &detail);
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id,
                        status: "sending: link resource".to_string(),
                    });
                }
                Err(err) => {
                    let err_detail = format!("failed err={err}");
                    log_delivery_trace(&message_id, &destination_hex, "link", &err_detail);
                    eprintln!(
                        "[daemon] link delivery failed dst={} msg_id={} err={}; trying opportunistic",
                        destination_hex, message_id, err
                    );
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id: message_id.clone(),
                        status: format!("link failed: {err}; trying opportunistic"),
                    });
                    // Opportunistic SINGLE packets must carry LXMF wire bytes
                    // without the destination prefix. Receivers prepend the
                    // packet destination hash before unpacking.
                    let opportunistic_payload = opportunistic_payload(&payload, &destination);
                    let mut data = PacketDataBuffer::new();
                    if data.write(opportunistic_payload).is_err() {
                        log_delivery_trace(
                            &message_id,
                            &destination_hex,
                            "opportunistic",
                            "payload too large",
                        );
                        let _ = receipt_tx
                            .send(ReceiptEvent { message_id, status: format!("failed: {}", err) });
                        return;
                    }

                    let packet = Packet {
                        header: Header {
                            ifac_flag: IfacFlag::Open,
                            header_type: HeaderType::Type1,
                            context_flag: ContextFlag::Unset,
                            propagation_type: PropagationType::Broadcast,
                            destination_type: DestinationType::Single,
                            packet_type: PacketType::Data,
                            hops: 0,
                        },
                        ifac: None,
                        destination: destination_hash,
                        transport: None,
                        context: PacketContext::None,
                        data,
                    };
                    let packet_hash = hex::encode(packet.hash().to_bytes());
                    track_receipt_mapping(&receipt_map, &packet_hash, &message_id);
                    if diagnostics_enabled() {
                        let detail = format!(
                            "sending packet_hash={} payload_len={} payload_prefix={}",
                            packet_hash,
                            opportunistic_payload.len(),
                            payload_preview(opportunistic_payload, 16)
                        );
                        log_delivery_trace(&message_id, &destination_hex, "opportunistic", &detail);
                    } else {
                        log_delivery_trace(
                            &message_id,
                            &destination_hex,
                            "opportunistic",
                            "sending",
                        );
                    }
                    let trace = transport.send_packet_with_trace(packet).await;
                    let trace_detail = send_trace_detail(trace);
                    log_delivery_trace(
                        &message_id,
                        &destination_hex,
                        "opportunistic",
                        &trace_detail,
                    );
                    let outcome = trace.outcome;
                    if !matches!(
                        outcome,
                        SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast
                    ) {
                        if let Ok(mut map) = receipt_map.lock() {
                            map.remove(&packet_hash);
                        }
                    }
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id,
                        status: send_outcome_status("opportunistic", outcome),
                    });
                }
            }
        });
        Ok(())
    }
}

impl AnnounceBridge for TransportBridge {
    fn announce_now(&self) -> Result<(), std::io::Error> {
        let transport = self.transport.clone();
        let destination = self.announce_destination.clone();
        let app_data = self.announce_app_data.clone();
        tokio::spawn(async move {
            transport.send_announce(&destination, app_data.as_deref()).await;
        });
        Ok(())
    }
}

fn opportunistic_payload<'a>(payload: &'a [u8], destination: &[u8; 16]) -> &'a [u8] {
    if payload.len() > 16 && payload[..16] == destination[..] {
        &payload[16..]
    } else {
        payload
    }
}

fn log_delivery_trace(message_id: &str, destination: &str, stage: &str, detail: &str) {
    eprintln!(
        "[delivery-trace] msg_id={} dst={} stage={} {}",
        message_id, destination, stage, detail
    );
}

fn diagnostics_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("RETICULUMD_DIAGNOSTICS")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on" | "debug"
                )
            })
            .unwrap_or(false)
    })
}

fn payload_preview(bytes: &[u8], limit: usize) -> String {
    let end = bytes.len().min(limit);
    hex::encode(&bytes[..end])
}

fn send_trace_detail(trace: SendPacketTrace) -> String {
    let direct_iface =
        trace.direct_iface.map(|iface| iface.to_string()).unwrap_or_else(|| "-".to_string());
    format!(
        "outcome={:?} direct_iface={} broadcast={} dispatch(matched={},sent={},failed={})",
        trace.outcome,
        direct_iface,
        trace.broadcast,
        trace.dispatch.matched_ifaces,
        trace.dispatch.sent_ifaces,
        trace.dispatch.failed_ifaces
    )
}

#[cfg(test)]
mod tests {
    use super::opportunistic_payload;
    use reticulum::delivery::send_outcome_status;
    use reticulum::destination_hash::parse_destination_hash_required;
    use reticulum::transport::SendPacketOutcome;

    #[test]
    fn opportunistic_payload_strips_destination_prefix() {
        let destination = [0xAA; 16];
        let mut payload = destination.to_vec();
        payload.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(opportunistic_payload(&payload, &destination), &[1, 2, 3, 4]);
    }

    #[test]
    fn opportunistic_payload_keeps_payload_without_prefix() {
        let destination = [0xAA; 16];
        let payload = vec![0xBB; 24];
        assert_eq!(opportunistic_payload(&payload, &destination), payload.as_slice());
    }

    #[test]
    fn send_outcome_status_maps_success() {
        assert_eq!(
            send_outcome_status("opportunistic", SendPacketOutcome::SentDirect),
            "sent: opportunistic"
        );
    }

    #[test]
    fn send_outcome_status_maps_failures() {
        assert_eq!(
            send_outcome_status(
                "opportunistic",
                SendPacketOutcome::DroppedMissingDestinationIdentity
            ),
            "failed: opportunistic missing destination identity"
        );
        assert_eq!(
            send_outcome_status("opportunistic", SendPacketOutcome::DroppedNoRoute),
            "failed: opportunistic no route"
        );
    }

    #[test]
    fn parse_destination_hex_required_rejects_invalid_hashes() {
        let err = parse_destination_hash_required("not-hex").expect_err("invalid hash");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let local = LocalSet::new();
    local
        .run_until(async {
            let args = Args::parse();
            let addr: SocketAddr = args.rpc.parse().expect("invalid rpc address");
            let store = MessagesStore::open(&args.db).expect("open sqlite");

            let identity_path = args.identity.clone().unwrap_or_else(|| {
                let mut path = args.db.clone();
                path.set_extension("identity");
                path
            });
            let identity = load_or_create_identity(&identity_path).expect("load identity");
            let identity_hash = hex::encode(identity.address_hash().as_slice());
            let local_display_name = std::env::var("LXMF_DISPLAY_NAME")
                .ok()
                .and_then(|value| normalize_display_name(&value));
            let daemon_config =
                args.config
                    .as_ref()
                    .and_then(|path| match DaemonConfig::from_path(path) {
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
            let peer_crypto: Arc<std::sync::Mutex<HashMap<String, PeerCrypto>>> =
                Arc::new(std::sync::Mutex::new(HashMap::new()));
            let mut announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> =
                None;
            let mut delivery_destination_hash_hex: Option<String> = None;
            let mut delivery_source_hash = [0u8; 16];
            let receipt_map: Arc<std::sync::Mutex<HashMap<String, String>>> =
                Arc::new(std::sync::Mutex::new(HashMap::new()));
            let (receipt_tx, mut receipt_rx) = unbounded_channel();

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
                let server_iface = iface_manager.lock().await.spawn(
                    TcpServer::new(addr.clone(), iface_manager.clone()),
                    TcpServer::spawn,
                );
                eprintln!("[daemon] tcp_server enabled iface={} bind={}", server_iface, addr);
                if let Some(config) = daemon_config.as_ref() {
                    for (host, port) in config.tcp_client_endpoints() {
                        let addr = format!("{}:{}", host, port);
                        let client_iface = iface_manager
                            .lock()
                            .await
                            .spawn(TcpClient::new(addr), TcpClient::spawn);
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
                    delivery_destination_hash_hex =
                        Some(hex::encode(dest.desc.address_hash.as_slice()));
                    println!(
                        "[daemon] delivery destination hash={}",
                        hex::encode(dest.desc.address_hash.as_slice())
                    );
                }
                announce_destination = Some(destination);
                transport = Some(Arc::new(transport_instance));
            }

            let bridge: Option<Arc<TransportBridge>> = transport
                .as_ref()
                .zip(announce_destination.as_ref())
                .map(|(transport, destination)| {
                    Arc::new(TransportBridge::new(
                        transport.clone(),
                        identity.clone(),
                        delivery_source_hash,
                        destination.clone(),
                        local_display_name.as_ref().and_then(|display_name| {
                            encode_delivery_display_name_app_data(display_name)
                        }),
                        peer_crypto.clone(),
                        receipt_map.clone(),
                        receipt_tx.clone(),
                    ))
                });

            let outbound_bridge: Option<Arc<dyn OutboundBridge>> = bridge
                .as_ref()
                .map(|bridge| bridge.clone() as Arc<dyn OutboundBridge>);
            let announce_bridge: Option<Arc<dyn AnnounceBridge>> = bridge
                .as_ref()
                .map(|bridge| bridge.clone() as Arc<dyn AnnounceBridge>);

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
                let daemon_receipts = daemon.clone();
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

            if args.announce_interval_secs > 0 {
                let _handle = daemon
                    .clone()
                    .start_announce_scheduler(args.announce_interval_secs);
            }

            if let Some(transport) = transport.clone() {
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
                                let (record, diagnostics) =
                                    decode_inbound_payload_with_diagnostics(
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

                let daemon_announce = daemon.clone();
                let peer_crypto = peer_crypto.clone();
                let announce_transport = transport.clone();
                tokio::task::spawn_local(async move {
                    let mut rx = announce_transport.recv_announces().await;
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
                            peer_crypto
                                .lock()
                                .expect("peer map")
                                .insert(peer.clone(), PeerCrypto { identity });
                            if let Some(name) = peer_name.as_ref() {
                                eprintln!("[daemon] rx announce peer={} name={}", peer, name);
                            } else {
                                eprintln!("[daemon] rx announce peer={}", peer);
                            }
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|value| value.as_secs() as i64)
                                .unwrap_or(0);
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

            let listener = TcpListener::bind(addr).await.unwrap();
            println!("reticulumd listening on http://{}", addr);

            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = Vec::new();
                loop {
                    let mut chunk = [0u8; 4096];
                    let read = stream.read(&mut chunk).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    if let Some(header_end) = http::find_header_end(&buffer) {
                        let headers = &buffer[..header_end];
                        if let Some(length) = http::parse_content_length(headers) {
                            let body_start = header_end + 4;
                            if buffer.len() >= body_start + length {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }

                if buffer.is_empty() {
                    continue;
                }

                let response = http::handle_http_request(&daemon, &buffer).unwrap_or_else(|err| {
                    http::build_error_response(&format!("rpc error: {}", err))
                });
                let _ = stream.write_all(&response).await;
                let _ = stream.shutdown().await;
            }
        })
        .await;
}

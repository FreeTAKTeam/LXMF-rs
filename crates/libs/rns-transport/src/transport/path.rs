use super::diag;
use super::*;
use crate::packet::{DestinationType, Header, HeaderType, PacketType, PropagationType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RouteDecision {
    pub packet: Packet,
    pub next_iface: Option<AddressHash>,
}

pub(super) fn route_inbound_packet(
    path_table: &PathTable,
    original_packet: &Packet,
    lookup: Option<AddressHash>,
) -> RouteDecision {
    let lookup = lookup.unwrap_or(original_packet.destination);

    let Some(entry) = path_table.get(&lookup) else {
        return RouteDecision { packet: *original_packet, next_iface: None };
    };

    let is_direct_hop = entry.hops <= 1 && entry.received_from == lookup;
    let packet = if is_direct_hop {
        Packet {
            header: Header {
                ifac_flag: original_packet.header.ifac_flag,
                header_type: HeaderType::Type1,
                context_flag: original_packet.header.context_flag,
                propagation_type: PropagationType::Broadcast,
                destination_type: original_packet.header.destination_type,
                packet_type: original_packet.header.packet_type,
                hops: original_packet.header.hops,
            },
            ifac: None,
            destination: original_packet.destination,
            transport: None,
            context: original_packet.context,
            data: original_packet.data,
        }
    } else {
        Packet {
            header: Header {
                ifac_flag: original_packet.header.ifac_flag,
                header_type: HeaderType::Type2,
                context_flag: original_packet.header.context_flag,
                propagation_type: PropagationType::Transport,
                destination_type: original_packet.header.destination_type,
                packet_type: original_packet.header.packet_type,
                hops: original_packet.header.hops,
            },
            ifac: None,
            destination: original_packet.destination,
            transport: Some(entry.received_from),
            context: original_packet.context,
            data: original_packet.data,
        }
    };

    RouteDecision { packet, next_iface: Some(entry.iface) }
}

pub(super) fn route_outbound_packet(
    path_table: &PathTable,
    original_packet: &Packet,
) -> RouteDecision {
    if original_packet.header.header_type == HeaderType::Type2 {
        return RouteDecision { packet: *original_packet, next_iface: None };
    }

    if original_packet.header.packet_type == PacketType::Announce {
        return RouteDecision { packet: *original_packet, next_iface: None };
    }

    if original_packet.header.destination_type == DestinationType::Plain
        || original_packet.header.destination_type == DestinationType::Group
    {
        return RouteDecision { packet: *original_packet, next_iface: None };
    }

    let Some(entry) = path_table.get(&original_packet.destination) else {
        return RouteDecision { packet: *original_packet, next_iface: None };
    };

    if entry.hops <= 1 && entry.received_from == original_packet.destination {
        return RouteDecision { packet: *original_packet, next_iface: Some(entry.iface) };
    }

    RouteDecision {
        packet: Packet {
            header: Header {
                ifac_flag: original_packet.header.ifac_flag,
                header_type: HeaderType::Type2,
                context_flag: original_packet.header.context_flag,
                propagation_type: PropagationType::Transport,
                destination_type: original_packet.header.destination_type,
                packet_type: original_packet.header.packet_type,
                hops: original_packet.header.hops,
            },
            ifac: original_packet.ifac,
            destination: original_packet.destination,
            transport: Some(entry.received_from),
            context: original_packet.context,
            data: original_packet.data,
        },
        next_iface: Some(entry.iface),
    }
}

pub(super) fn message_to_next_hop<'a>(
    packet: &Packet,
    handler: &MutexGuard<'a, TransportHandler>,
    lookup: Option<AddressHash>,
) -> Option<TxMessage> {
    let decision = route_inbound_packet(&handler.path_table, packet, lookup);
    let packet = decision.packet;
    let maybe_iface = decision.next_iface;

    if let Some(iface) = maybe_iface {
        if diag::enabled() {
            log::info!(
                "[tp-diag] forward_next_hop node={} dst={} lookup={} out={} iface={}",
                handler.config.name,
                packet.destination,
                lookup.map(|value| value.to_string()).unwrap_or_else(|| "-".to_string()),
                packet,
                iface
            );
        }
        Some(TxMessage { tx_type: TxMessageType::Direct(iface), packet })
    } else if diag::enabled() {
        log::info!(
            "[tp-diag] forward_next_hop_miss node={} dst={} lookup={}",
            handler.config.name,
            packet.destination,
            lookup.map(|value| value.to_string()).unwrap_or_else(|| "-".to_string())
        );
        None
    } else {
        None
    }
}

#[allow(clippy::large_enum_variant)]
enum PathRequestAction {
    LocalResponse {
        destination: Arc<Mutex<SingleInputDestination>>,
        app_data: Option<Vec<u8>>,
        tag_bytes: Vec<u8>,
        config_name: String,
    },
    Message(TxMessage),
    None,
}

pub(super) async fn handle_path_request_unlocked(
    packet: &Packet,
    handler_arc: Arc<Mutex<TransportHandler>>,
    iface: AddressHash,
) -> Option<TxMessage> {
    let action = {
        let mut handler = handler_arc.lock().await;
        let request = handler.path_requests.decode(packet.data.as_slice())?;

        if let Some(destination) = handler.single_in_destinations.get(&request.destination).cloned()
        {
            let app_data =
                handler.single_in_destination_app_data.get(&request.destination).cloned();
            if !handler.path_requests.allow_local_response(
                &request.destination,
                request.requesting_transport,
                &request.tag_bytes,
                iface,
            ) {
                log::trace!(
                    "tp({}): suppressing repeated local path response for {} on {}",
                    handler.config.name,
                    request.destination,
                    iface
                );
                return None;
            }

            PathRequestAction::LocalResponse {
                destination,
                app_data,
                tag_bytes: request.tag_bytes,
                config_name: handler.config.name.clone(),
            }
        } else if handler.config.retransmit {
            if let Some(entry) = handler.path_table.get(&request.destination) {
                if let Some(requestor_id) = request.requesting_transport {
                    if requestor_id == entry.received_from {
                        log::trace!(
                            "tp({}): dropping circular path request from {}",
                            handler.config.name,
                            request.destination
                        );
                        return None;
                    }
                }

                let hops = entry.hops;

                handler.announce_table.add_response(request.destination, iface, hops);

                log::trace!(
                    "tp({}): scheduled remote path response to {} ({} hops) over {}",
                    handler.config.name,
                    request.destination,
                    hops,
                    iface
                );

                PathRequestAction::None
            } else if let Some(packet) = handler.path_requests.generate_recursive(
                &request.destination,
                Some(iface),
                Some(request.tag_bytes),
            ) {
                PathRequestAction::Message(TxMessage {
                    tx_type: TxMessageType::Broadcast(Some(iface)),
                    packet,
                })
            } else {
                PathRequestAction::None
            }
        } else {
            PathRequestAction::None
        }
    };

    match action {
        PathRequestAction::LocalResponse { destination, app_data, tag_bytes, config_name } => {
            let response = match destination.try_lock() {
                Ok(mut destination) => destination
                    .path_response_with_tag(OsRng, app_data.as_deref(), Some(tag_bytes.as_slice()))
                    .expect("valid path response"),
                Err(_) => {
                    log::debug!(
                        "tp({}): skipping path response while destination is busy",
                        config_name
                    );
                    return None;
                }
            };

            log::trace!("tp({}): send direct path response over {}", config_name, iface);

            Some(TxMessage { tx_type: TxMessageType::Direct(iface), packet: response })
        }
        PathRequestAction::Message(message) => Some(message),
        PathRequestAction::None => None,
    }
}

pub(super) async fn handle_fixed_destinations_unlocked(
    packet: &Packet,
    handler_arc: Arc<Mutex<TransportHandler>>,
    iface: AddressHash,
) -> (bool, Option<TxMessage>) {
    enum FixedDestination {
        PathRequest,
        TunnelSynthesize,
        None,
    }

    let destination = {
        let handler = handler_arc.lock().await;
        if packet.destination == handler.fixed_dest_path_requests {
            FixedDestination::PathRequest
        } else if packet.destination == handler.fixed_dest_tunnel_synthesize {
            FixedDestination::TunnelSynthesize
        } else {
            FixedDestination::None
        }
    };

    match destination {
        FixedDestination::PathRequest => {
            let message = handle_path_request_unlocked(packet, handler_arc, iface).await;
            (true, message)
        }
        FixedDestination::TunnelSynthesize => {
            let mut handler = handler_arc.lock().await;
            super::tunnels::handle_tunnel_synthesize_packet(packet, &mut handler, iface);
            (true, None)
        }
        FixedDestination::None => (false, None),
    }
}

async fn handle_link_request_as_destination(
    destination: Arc<Mutex<SingleInputDestination>>,
    packet: &Packet,
    iface: AddressHash,
    handler_arc: Arc<Mutex<TransportHandler>>,
    config_name: String,
) {
    let proof_material = match destination.try_lock() {
        Ok(mut destination) => match destination.handle_packet(packet) {
            DestinationHandleStatus::LinkProof => {
                Some((destination.sign_key().clone(), destination.desc))
            }
            DestinationHandleStatus::None => None,
        },
        Err(_) => {
            log::debug!(
                "tp({}): skipping link request while local destination is busy",
                config_name
            );
            None
        }
    };

    let Some((sign_key, destination_desc)) = proof_material else {
        return;
    };

    let link_id = LinkId::from(packet);
    let link_in_event_tx = {
        let handler = handler_arc.lock().await;
        if handler.in_links.contains_key(&link_id) {
            return;
        }
        handler.link_in_event_tx.clone()
    };

    log::trace!("tp({}): send proof to {}", config_name, packet.destination);

    let Ok(mut link) = Link::new_from_request(packet, sign_key, destination_desc, link_in_event_tx)
    else {
        return;
    };

    link.set_ingress_iface(iface);
    log::trace!("[tp] link_proof_tx dst={} link_id={}", packet.destination, link.id());
    // Link-request proofs must go back over the interface that delivered
    // the request so multi-hop requestors can activate the link.
    let proof_message = TxMessage { tx_type: TxMessageType::Direct(iface), packet: link.prove() };
    let stored_link_id = *link.id();
    let destination_hash = link.destination().address_hash;

    let should_send = {
        let mut handler = handler_arc.lock().await;
        if let std::collections::hash_map::Entry::Vacant(entry) =
            handler.in_links.entry(stored_link_id)
        {
            log::debug!(
                "tp({}): save input link {} for destination {}",
                config_name,
                stored_link_id,
                destination_hash
            );
            entry.insert(Arc::new(Mutex::new(link)));
            true
        } else {
            false
        }
    };

    if should_send {
        let _ = TransportHandler::send_message_unlocked(handler_arc, proof_message).await;
    }
}

pub(super) async fn handle_link_request_as_intermediate<'a>(
    received_from: AddressHash,
    next_hop: AddressHash,
    next_hop_iface: AddressHash,
    packet: &Packet,
    handler_arc: Arc<Mutex<TransportHandler>>,
    mut handler: MutexGuard<'a, TransportHandler>,
) {
    if diag::enabled() {
        log::info!(
            "[tp-diag] link_request_intermediate node={} dst={} from_iface={} next_hop={} next_iface={} packet={}",
            handler.config.name,
            packet.destination,
            received_from,
            next_hop,
            next_hop_iface,
            packet
        );
    }
    handler.link_table.add(packet, packet.destination, received_from, next_hop, next_hop_iface);

    let message = message_to_next_hop(packet, &handler, None);
    drop(handler);
    if let Some(message) = message {
        let _ = TransportHandler::send_message_unlocked(handler_arc, message).await;
    }
}

pub(super) async fn handle_link_request_unlocked(
    packet: &Packet,
    iface: AddressHash,
    handler_arc: Arc<Mutex<TransportHandler>>,
) {
    log::trace!(
        "[tp] link_request dst={} ctx={:02x} hops={}",
        packet.destination,
        packet.context as u8,
        packet.header.hops
    );

    enum LinkRequestAction {
        Local { destination: Arc<Mutex<SingleInputDestination>>, config_name: String },
        Intermediate { next_hop: AddressHash, next_iface: AddressHash },
        Unknown { config_name: String },
    }

    let action = {
        let handler = handler_arc.lock().await;
        if let Some(destination) = handler.single_in_destinations.get(&packet.destination).cloned()
        {
            log::trace!(
                "tp({}): handle link request for {}",
                handler.config.name,
                packet.destination
            );
            LinkRequestAction::Local { destination, config_name: handler.config.name.clone() }
        } else if let Some((next_hop, next_iface)) =
            handler.path_table.next_hop_full(&packet.destination)
        {
            log::trace!(
                "tp({}): handle link request for remote destination {}",
                handler.config.name,
                packet.destination
            );
            LinkRequestAction::Intermediate { next_hop, next_iface }
        } else {
            LinkRequestAction::Unknown { config_name: handler.config.name.clone() }
        }
    };

    match action {
        LinkRequestAction::Local { destination, config_name } => {
            handle_link_request_as_destination(
                destination,
                packet,
                iface,
                handler_arc,
                config_name,
            )
            .await;
        }
        LinkRequestAction::Intermediate { next_hop, next_iface } => {
            let handler = handler_arc.lock().await;
            handle_link_request_as_intermediate(
                iface,
                next_hop,
                next_iface,
                packet,
                handler_arc.clone(),
                handler,
            )
            .await;
        }
        LinkRequestAction::Unknown { config_name } => {
            log::trace!(
                "tp({}): dropping link request to unknown destination {}",
                config_name,
                packet.destination
            );
        }
    }
}

include!("path_tests.rs");

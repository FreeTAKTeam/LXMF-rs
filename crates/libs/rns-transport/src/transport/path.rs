use super::*;
use crate::destination::link::clamp_link_request_signalling_mtu;
use crate::iface::InterfaceMode;
use crate::packet::{DestinationType, Header, HeaderType, PacketType, PropagationType};

#[derive(Debug, Clone, PartialEq, Eq)]
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
        return RouteDecision { packet: original_packet.clone(), next_iface: None };
    };

    // Centralized direct-hop criterion (issue #515): see
    // `PathEntry::is_direct` for the full invariant and its dependency on
    // `apply_receive_hop_increment` in transport/jobs.rs.
    let is_direct_hop = entry.is_direct();
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
            data: original_packet.data.clone(),
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
            data: original_packet.data.clone(),
        }
    };

    RouteDecision { packet, next_iface: Some(entry.iface) }
}

pub(super) fn route_outbound_packet(
    path_table: &PathTable,
    original_packet: &Packet,
    connected_to_shared_instance: bool,
) -> RouteDecision {
    if original_packet.header.header_type == HeaderType::Type2 {
        return RouteDecision { packet: original_packet.clone(), next_iface: None };
    }

    if original_packet.header.packet_type == PacketType::Announce {
        return RouteDecision { packet: original_packet.clone(), next_iface: None };
    }

    if original_packet.header.destination_type == DestinationType::Plain
        || original_packet.header.destination_type == DestinationType::Group
    {
        return RouteDecision { packet: original_packet.clone(), next_iface: None };
    }

    let Some(entry) = path_table.get(&original_packet.destination) else {
        return RouteDecision { packet: original_packet.clone(), next_iface: None };
    };

    // Centralized `Transport.outbound()` header rule (issue #515): see
    // `PathEntry::type1_eligible` for the full reference-parity rule and
    // why `received_from` deliberately doesn't factor into it.
    let type1_eligible = entry.type1_eligible(connected_to_shared_instance);

    if type1_eligible {
        return RouteDecision { packet: original_packet.clone(), next_iface: Some(entry.iface) };
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
            data: original_packet.data.clone(),
        },
        next_iface: Some(entry.iface),
    }
}

pub(super) async fn send_to_next_hop<'a>(
    packet: &Packet,
    handler: &MutexGuard<'a, TransportHandler>,
    lookup: Option<AddressHash>,
) -> bool {
    if !handler.config.transport_enabled {
        log::debug!(
            "[tp-diag] forward_next_hop_skip node={} dst={} reason=transport_disabled",
            handler.config.name,
            packet.destination
        );
        return false;
    }

    let decision = route_inbound_packet(&handler.path_table, packet, lookup);
    let packet = decision.packet;
    let maybe_iface = decision.next_iface;

    if let Some(iface) = maybe_iface {
        log::debug!(
            "[tp-diag] forward_next_hop node={} dst={} lookup={} out={} iface={}",
            handler.config.name,
            packet.destination,
            lookup.map(|value| value.to_string()).unwrap_or_else(|| "-".to_string()),
            packet,
            iface
        );
        handler.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
    } else {
        log::debug!(
            "[tp-diag] forward_next_hop_miss node={} dst={} lookup={}",
            handler.config.name,
            packet.destination,
            lookup.map(|value| value.to_string()).unwrap_or_else(|| "-".to_string())
        );
    }

    maybe_iface.is_some()
}

pub(super) async fn handle_path_request<'a>(
    packet: &Packet,
    handler: &mut MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
) {
    if let Some(request) = handler.path_requests.decode(packet.data.as_slice(), iface) {
        if let Some(dest) = handler.single_in_destinations.get(&request.destination).cloned() {
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
                return;
            }

            let response = dest
                .lock()
                .await
                .path_response_with_tag(
                    OsRng,
                    app_data.as_deref(),
                    Some(request.tag_bytes.as_slice()),
                )
                .expect("valid path response");

            handler
                .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet: response })
                .await;

            log::trace!("tp({}): send direct path response over {}", handler.config.name, iface);

            return;
        }

        if handler.config.transport_enabled {
            if let Some(entry) = handler.path_table.get(&request.destination) {
                let next_hop = entry.received_from;
                let learned_iface = entry.iface;
                let hops = entry.hops;

                if let Some(requestor_id) = request.requesting_transport {
                    if requestor_id == next_hop {
                        log::trace!(
                            "tp({}): dropping circular path request from {}",
                            handler.config.name,
                            request.destination
                        );
                        return;
                    }
                }

                let incoming_iface_mode = handler.iface_manager.lock().await.mode(&iface);
                // Reference parity (issue #516): Python Reticulum
                // `Transport.py` (~line 3044) suppresses a known-path
                // response when the request arrived on a MODE_ROAMING
                // interface and the known next hop is attached to that
                // same interface (`attached_interface == received_from`),
                // because roaming peers are expected to move and answering
                // would pin a stale route. Behavior here matches the
                // reference exactly; regression coverage lives in
                // `tests_parts/module_prelude.rs`
                // (`roaming_iface_suppresses_known_path_response_when_next_hop_is_same_iface`,
                // `roaming_iface_delays_known_path_response_when_next_hop_differs`,
                // `roaming_suppression_only_applies_to_roaming_mode`).
                if incoming_iface_mode == Some(InterfaceMode::Roaming) && learned_iface == iface {
                    log::trace!(
                        "tp({}): suppressing roaming same-iface path response for {}",
                        handler.config.name,
                        request.destination
                    );
                    return;
                }

                if incoming_iface_mode == Some(InterfaceMode::Roaming) {
                    handler.announce_table.add_response_with_extra_grace(
                        request.destination,
                        iface,
                        hops,
                        super::announce_table::PATH_RESPONSE_ROAMING_GRACE,
                    );
                } else {
                    handler.announce_table.add_response(request.destination, iface, hops);
                }

                log::trace!(
                    "tp({}): scheduled remote path response to {} ({} hops) over {}",
                    handler.config.name,
                    request.destination,
                    hops,
                    iface
                );

                return;
            }
        }

        if handler.config.transport_enabled {
            let should_search_for_unknown = handler
                .iface_manager
                .lock()
                .await
                .mode(&iface)
                .map(|mode| mode.discovers_unknown_paths() || mode == InterfaceMode::Boundary)
                .unwrap_or(false);
            if !should_search_for_unknown {
                log::trace!(
                    "tp({}): not searching for unknown path {} from non-discovery iface {}",
                    handler.config.name,
                    request.destination,
                    iface
                );
                return;
            }
            if let Some(packet) = handler.path_requests.generate_recursive(
                &request.destination,
                Some(iface),
                Some(request.tag_bytes.clone()),
            ) {
                let boundary_search_modes = (handler.iface_manager.lock().await.mode(&iface)
                    == Some(InterfaceMode::Boundary))
                .then_some([InterfaceMode::Boundary, InterfaceMode::Gateway]);
                handler
                    .send_recursive_path_request_with_modes(
                        TxMessage { tx_type: TxMessageType::Broadcast(Some(iface)), packet },
                        boundary_search_modes.as_ref().map(|modes| modes.as_slice()),
                    )
                    .await;
            }
        }
    }
}

pub(super) async fn handle_fixed_destinations<'a>(
    packet: &Packet,
    handler: &mut MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
) -> bool {
    if packet.destination == handler.fixed_dest_path_requests {
        handle_path_request(packet, handler, iface).await;
        true
    } else if packet.destination == handler.fixed_dest_tunnel_synthesize {
        super::tunnels::handle_tunnel_synthesize_packet(packet, handler, iface).await;
        true
    } else {
        false
    }
}

pub(super) async fn handle_link_request_as_destination<'a>(
    destination: Arc<Mutex<SingleInputDestination>>,
    packet: &Packet,
    iface: AddressHash,
    mut handler: MutexGuard<'a, TransportHandler>,
) {
    let mut destination = destination.lock().await;
    match destination.handle_packet(packet) {
        DestinationHandleStatus::LinkProof => {
            let link_id = LinkId::from(packet);
            if !handler.in_links.contains_key(&link_id) {
                log::trace!("tp({}): send proof to {}", handler.config.name, packet.destination);

                let link = Link::new_from_request(
                    packet,
                    destination.sign_key().clone(),
                    destination.desc,
                    handler.link_in_event_tx.clone(),
                );

                if let Ok(mut link) = link {
                    link.set_ingress_iface(iface);
                    log::trace!(
                        "[tp] link_proof_tx dst={} link_id={}",
                        packet.destination,
                        link.id()
                    );
                    // Link-request proofs must go back over the interface that delivered
                    // the request so multi-hop requestors can activate the link.
                    handler
                        .send(TxMessage {
                            tx_type: TxMessageType::Direct(iface),
                            packet: link.prove(),
                        })
                        .await;

                    log::debug!(
                        "tp({}): save input link {} for destination {}",
                        handler.config.name,
                        link.id(),
                        link.destination().address_hash
                    );

                    handler.in_links.insert(*link.id(), Arc::new(Mutex::new(link)));
                }
            }
        }
        DestinationHandleStatus::None => {}
    }
}

pub(super) async fn handle_link_request_as_intermediate<'a>(
    received_from: AddressHash,
    next_hop: AddressHash,
    next_hop_iface: AddressHash,
    packet: &Packet,
    mut handler: MutexGuard<'a, TransportHandler>,
) {
    if !handler.config.transport_enabled {
        log::debug!(
            "[tp-diag] link_request_intermediate_skip node={} dst={} from_iface={} reason=transport_disabled",
            handler.config.name,
            packet.destination,
            received_from
        );
        return;
    }

    log::debug!(
        "[tp-diag] link_request_intermediate node={} dst={} from_iface={} next_hop={} next_iface={} packet={}",
        handler.config.name,
        packet.destination,
        received_from,
        next_hop,
        next_hop_iface,
        packet
    );
    let mut packet = packet.clone();
    clamp_forwarded_link_request_mtu(&mut packet, &handler, received_from, next_hop_iface).await;
    handler.link_table.add(&packet, packet.destination, received_from, next_hop, next_hop_iface);

    send_to_next_hop(&packet, &handler, None).await;
}

async fn clamp_forwarded_link_request_mtu<'a>(
    packet: &mut Packet,
    handler: &MutexGuard<'a, TransportHandler>,
    received_from: AddressHash,
    next_hop_iface: AddressHash,
) {
    if packet.header.packet_type != PacketType::LinkRequest {
        return;
    }

    let mut max_mtu = usize::MAX;
    {
        let manager = handler.iface_manager.lock().await;
        if let Some(mtu) = manager.mtu(&received_from) {
            max_mtu = max_mtu.min(mtu);
        }
        if let Some(mtu) = manager.mtu(&next_hop_iface) {
            max_mtu = max_mtu.min(mtu);
        }
    }

    clamp_link_request_signalling_mtu(packet, max_mtu);
}

pub(super) async fn handle_link_request<'a>(
    packet: &Packet,
    iface: AddressHash,
    handler: MutexGuard<'a, TransportHandler>,
) {
    log::trace!(
        "[tp] link_request dst={} ctx={:02x} hops={}",
        packet.destination,
        packet.context as u8,
        packet.header.hops
    );
    if let Some(destination) = handler.single_in_destinations.get(&packet.destination).cloned() {
        log::trace!("tp({}): handle link request for {}", handler.config.name, packet.destination);

        handle_link_request_as_destination(destination, packet, iface, handler).await;
    } else if !handler.config.transport_enabled {
        log::trace!(
            "tp({}): dropping transit link request to {} because transport forwarding is disabled",
            handler.config.name,
            packet.destination
        );
    } else if let Some(entry) = handler.path_table.next_hop_full(&packet.destination) {
        log::trace!(
            "tp({}): handle link request for remote destination {}",
            handler.config.name,
            packet.destination
        );

        let (next_hop, next_iface) = entry;
        handle_link_request_as_intermediate(iface, next_hop, next_iface, packet, handler).await;
    } else {
        log::trace!(
            "tp({}): dropping link request to unknown destination {}",
            handler.config.name,
            packet.destination
        );
    }
}

include!("path_tests.rs");

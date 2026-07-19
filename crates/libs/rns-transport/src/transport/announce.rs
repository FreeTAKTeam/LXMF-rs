use super::announce_limits::AnnounceLimitAction;
use super::*;
use crate::packet::{Header, HeaderType, PropagationType};

async fn process_announce<'a>(
    packet: &Packet,
    mut handler: MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
    source: IfaceSource,
    announce: crate::destination::AnnounceInfo<'_>,
    shared_config: crate::iface::InterfaceSharedConfig,
) -> MutexGuard<'a, TransportHandler> {
    let local_destination_known = handler.has_destination(&packet.destination);
    let remote_destination_known = handler.knows_destination(&packet.destination);

    if let Some(existing) = handler.single_out_destinations.get(&packet.destination).cloned() {
        let existing = existing.lock().await;
        if existing.identity.public_key != announce.destination.identity.public_key
            || existing.identity.verifying_key != announce.destination.identity.verifying_key
        {
            log::warn!(
                "tp({}): rejecting announce for {} due to identity drift",
                handler.config.name,
                packet.destination
            );
            return handler;
        }
    }
    let ratchet = announce.ratchet;
    if let Some(ratchet_bytes) = ratchet {
        if let Some(store) = handler.ratchet_store.as_mut() {
            if let Err(err) = store.remember(&packet.destination, ratchet_bytes) {
                log::warn!(
                    "tp({}): failed to remember ratchet for {}: {:?}",
                    handler.config.name,
                    packet.destination,
                    err
                );
            }
        }
    }
    // Retransmit/path bookkeeping must use the announced destination hash,
    // not the bare identity hash, otherwise peers learn only identity routes
    // and cannot resolve application destinations like `lxmf.delivery`.
    let dest_hash = announce.destination.desc.address_hash;
    if packet.transport.is_some()
        && handler.announce_table.observe_passed_rebroadcast(&dest_hash, packet.header.hops)
    {
        log::trace!(
            "tp({}): completed announce processing for {}, rebroadcast was passed onward",
            handler.config.name,
            dest_hash
        );
    }
    let destination = Arc::new(Mutex::new(announce.destination));

    // Auto-unicast: if this announce arrived over a multicast iface from a
    // known UDP peer, route future point-to-point traffic for this
    // destination over a per-peer unicast UDP iface instead of back onto
    // the multicast group. Otherwise keep the original iface.
    let route_iface = handler.unicast_iface_for_source(iface, source).await.unwrap_or(iface);

    let path_accepted = if local_destination_known {
        false
    } else {
        let existing_path_iface =
            handler.path_table.get(&packet.destination).map(|entry| entry.iface);
        let existing_path_mode = if let Some(existing_iface) = existing_path_iface {
            handler.iface_manager.lock().await.mode(&existing_iface)
        } else {
            None
        };
        handler.path_table.handle_announce(
            packet,
            packet.transport,
            route_iface,
            announce.random_blob,
            |iface: &AddressHash| {
                (Some(*iface) == existing_path_iface).then_some(existing_path_mode).flatten()
            },
        )
    };

    if path_accepted {
        if !handler.single_out_destinations.contains_key(&packet.destination) {
            log::trace!("tp({}): new announce for {}", handler.config.name, packet.destination);

            handler.single_out_destinations.insert(packet.destination, destination.clone());
        }

        if handler.announce_limits.should_suppress_rebroadcast(packet, &shared_config) {
            log::debug!(
                "tp({}): suppressing announce rebroadcast for {} due to announce_rate_target",
                handler.config.name,
                packet.destination
            );
        } else {
            handler.announce_table.add(packet, dest_hash, route_iface);
        }

        let random_blobs = handler.path_table.random_blobs_for(&packet.destination);
        handler.tunnel_table.note_path(super::tunnels::TunnelPathNote {
            iface: route_iface,
            destination: packet.destination,
            received_from: packet.transport.unwrap_or(packet.destination),
            hops: packet.header.hops,
            random_blobs,
            packet_hash: packet.hash(),
            now: std::time::Instant::now(),
        });
    } else if remote_destination_known {
        log::trace!(
            "tp({}): ignored stale announce path refresh for {}",
            handler.config.name,
            packet.destination
        );
    }

    let name_hash = {
        let destination = destination.lock().await;
        let source = destination.desc.name.as_name_hash_slice();
        let mut name_hash = [0u8; crate::destination::NAME_HASH_LENGTH];
        name_hash.copy_from_slice(source);
        name_hash
    };
    let interface = route_iface.as_slice().to_vec();

    if path_accepted {
        let waiting_discovery_requesters =
            handler.path_requests.take_discovery_requesters(&dest_hash);
        for requesting_iface in waiting_discovery_requesters {
            log::debug!(
                "tp({}): answering waiting discovery path request for {} on {}",
                handler.config.name,
                dest_hash,
                requesting_iface
            );
            let response = Packet {
                header: Header {
                    ifac_flag: packet.header.ifac_flag,
                    header_type: HeaderType::Type2,
                    context_flag: packet.header.context_flag,
                    propagation_type: PropagationType::Transport,
                    destination_type: packet.header.destination_type,
                    packet_type: packet.header.packet_type,
                    hops: packet.header.hops,
                },
                ifac: None,
                destination: packet.destination,
                transport: Some(*handler.config.identity.address_hash()),
                context: PacketContext::PathResponse,
                data: packet.data.clone(),
            };
            handler
                .send(TxMessage {
                    tx_type: TxMessageType::Direct(requesting_iface),
                    packet: response,
                })
                .await;
        }
    }

    log::debug!(
        "[announce-debug] accepted dst={} app_data_hex={}",
        packet.destination,
        hex::encode(announce.app_data)
    );

    if path_accepted
        && handler
            .announce_tx
            .send(AnnounceEvent {
                destination,
                app_data: PacketDataBuffer::new_from_slice(announce.app_data),
                ratchet,
                name_hash,
                hops: packet.header.hops,
                interface,
            })
            .is_err()
    {
        log::trace!(
            "[announce-debug] accepted announce has no active subscribers dst={}",
            packet.destination
        );
    }

    handler
}

pub(super) async fn handle_announce<'a>(
    packet: &Packet,
    mut handler: MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
    source: IfaceSource,
) {
    let announce = match DestinationAnnounce::validate(packet) {
        Ok(result) => result,
        Err(err) => {
            log::trace!(
                "[transport] announce validate failed dst={} err={:?}",
                packet.destination,
                err
            );
            return;
        }
    };

    let destination_known = handler.has_destination(&packet.destination)
        || handler.knows_destination(&packet.destination);
    let shared_config = {
        let manager = handler.iface_manager.lock().await;
        manager.shared_config(&iface).cloned().unwrap_or_default()
    };
    if let AnnounceLimitAction::Hold(delay) = handler.announce_limits.check_with_shared_config(
        iface,
        packet,
        source,
        destination_known,
        &shared_config,
    ) {
        log::debug!(
            "tp({}): holding announce for {} for {:?}",
            handler.config.name,
            packet.destination,
            delay
        );
        return;
    }

    let _ = process_announce(packet, handler, iface, source, announce, shared_config).await;
}

pub(super) async fn retransmit_announces<'a>(mut handler: MutexGuard<'a, TransportHandler>) {
    let transport_id = *handler.config.identity.address_hash();
    let messages = handler.announce_table.drain_retransmissions(&transport_id);

    for message in messages {
        handler.send(message).await;
    }
}

pub(super) async fn release_held_announces<'a>(handler: MutexGuard<'a, TransportHandler>) {
    let mut handler = handler;
    let released = handler.announce_limits.release_ready();

    for released_announce in released {
        let packet = released_announce.packet;
        let iface = released_announce.iface;
        let source = released_announce.source;
        let announce = match DestinationAnnounce::validate(&packet) {
            Ok(result) => result,
            Err(err) => {
                log::warn!(
                    "dropping held announce for {} after revalidate failure: {:?}",
                    packet.destination,
                    err
                );
                continue;
            }
        };

        let shared_config = {
            let manager = handler.iface_manager.lock().await;
            manager.shared_config(&iface).cloned().unwrap_or_default()
        };

        handler = process_announce(&packet, handler, iface, source, announce, shared_config).await;
    }
}

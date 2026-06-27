use super::announce_limits::AnnounceLimitAction;
use super::*;

async fn process_announce<'a>(
    packet: &Packet,
    mut handler: MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
    source: IfaceSource,
    announce: crate::destination::AnnounceInfo<'_>,
    shared_config: crate::iface::InterfaceSharedConfig,
) -> MutexGuard<'a, TransportHandler> {
    let destination_known = handler.has_destination(&packet.destination);

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
    let destination = Arc::new(Mutex::new(announce.destination));

    // Auto-unicast: if this announce arrived over a multicast iface from a
    // known UDP peer, route future point-to-point traffic for this
    // destination over a per-peer unicast UDP iface instead of back onto
    // the multicast group. Otherwise keep the original iface.
    let route_iface = handler.unicast_iface_for_source(iface, source).await.unwrap_or(iface);

    if !destination_known {
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

        handler.path_table.handle_announce(packet, packet.transport, route_iface);
        handler.tunnel_table.note_path(
            route_iface,
            packet.destination,
            packet.transport.unwrap_or(packet.destination),
            packet.header.hops,
            packet.hash(),
            std::time::Instant::now(),
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

    log::debug!(
        "[announce-debug] accepted dst={} app_data_hex={}",
        packet.destination,
        hex::encode(announce.app_data)
    );

    let _ = handler.announce_tx.send(AnnounceEvent {
        destination,
        app_data: PacketDataBuffer::new_from_slice(announce.app_data),
        ratchet,
        name_hash,
        hops: packet.header.hops,
        interface,
    });

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

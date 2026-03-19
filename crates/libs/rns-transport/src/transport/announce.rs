use super::announce_limits::AnnounceLimitAction;
use super::*;

async fn process_announce<'a>(
    packet: &Packet,
    mut handler: MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
    announce: crate::destination::AnnounceInfo<'_>,
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

    if !destination_known {
        if !handler.single_out_destinations.contains_key(&packet.destination) {
            log::trace!("tp({}): new announce for {}", handler.config.name, packet.destination);

            handler.single_out_destinations.insert(packet.destination, destination.clone());
        }

        handler.announce_table.add(packet, dest_hash, iface);

        handler.path_table.handle_announce(packet, packet.transport, iface);
    }

    let name_hash = {
        let destination = destination.lock().await;
        let source = destination.desc.name.as_name_hash_slice();
        let mut name_hash = [0u8; crate::destination::NAME_HASH_LENGTH];
        name_hash.copy_from_slice(source);
        name_hash
    };
    let interface = iface.as_slice().to_vec();

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
) {
    let announce = match DestinationAnnounce::validate(packet) {
        Ok(result) => result,
        Err(err) => {
            eprintln!(
                "[transport] announce validate failed dst={} err={:?}",
                packet.destination, err
            );
            return;
        }
    };

    let destination_known = handler.has_destination(&packet.destination)
        || handler.knows_destination(&packet.destination);
    match handler.announce_limits.check(iface, packet, destination_known) {
        AnnounceLimitAction::Allow => {}
        AnnounceLimitAction::Hold(release_after) => {
            log::info!(
                "tp({}): holding announce for {} on iface {} for at least {:?}",
                handler.config.name,
                packet.destination,
                iface,
                release_after,
            );
            return;
        }
    }

    let _ = process_announce(packet, handler, iface, announce).await;
}

pub(super) async fn retransmit_announces<'a>(mut handler: MutexGuard<'a, TransportHandler>) {
    let transport_id = *handler.config.identity.address_hash();
    let messages = handler.announce_table.to_retransmit(&transport_id);

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
        let announce = match DestinationAnnounce::validate(&packet) {
            Ok(result) => result,
            Err(err) => {
                log::warn!(
                    "tp: dropping held announce for {} after revalidate failure: {:?}",
                    packet.destination,
                    err
                );
                continue;
            }
        };

        handler = process_announce(&packet, handler, iface, announce).await;
    }
}

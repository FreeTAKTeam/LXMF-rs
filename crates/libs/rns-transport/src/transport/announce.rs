use super::*;

pub(super) async fn handle_announce<'a>(
    packet: &Packet,
    mut handler: MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
) {
    if let Some(blocked_until) = handler.announce_limits.check(&packet.destination) {
        log::info!(
            "tp({}): too many announces from {}, blocked for {} seconds",
            handler.config.name,
            &packet.destination,
            blocked_until.as_secs(),
        );
        return;
    }

    let destination_known = handler.has_destination(&packet.destination);

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

    let retransmit = handler.config.retransmit;
    if retransmit {
        let transport_id = *handler.config.identity.address_hash();
        if let Some(message) = handler.announce_table.new_packet(&dest_hash, &transport_id) {
            handler.send(message).await;
        }
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
}

pub(super) async fn retransmit_announces<'a>(mut handler: MutexGuard<'a, TransportHandler>) {
    let transport_id = *handler.config.identity.address_hash();
    let messages = handler.announce_table.to_retransmit(&transport_id);

    for message in messages {
        handler.send(message).await;
    }
}

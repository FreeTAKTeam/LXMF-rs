use super::announce_limits::AnnounceLimitAction;
use super::*;
use crate::identity::Identity;
use crate::transport::worker_boundary::{
    WorkerBackend, WorkerError, WorkerJob, WorkerJobKind, WorkerResultKind,
};

const MAX_ANNOUNCE_VALIDATION_WORKERS: usize = 4;

static ANNOUNCE_VALIDATION_PERMITS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

fn announce_validation_permits() -> Arc<tokio::sync::Semaphore> {
    ANNOUNCE_VALIDATION_PERMITS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_ANNOUNCE_VALIDATION_WORKERS)))
        .clone()
}

pub(super) struct ValidatedAnnounce {
    pub destination: SingleOutputDestination,
    pub app_data: PacketDataBuffer,
    pub ratchet: Option<[u8; crate::destination::RATCHET_LENGTH]>,
}

impl ValidatedAnnounce {
    fn from_info(info: crate::destination::AnnounceInfo<'_>) -> Self {
        Self {
            destination: info.destination,
            app_data: PacketDataBuffer::new_from_slice(info.app_data),
            ratchet: info.ratchet,
        }
    }

    #[allow(dead_code)]
    pub(super) fn from_worker_result(kind: WorkerResultKind) -> Result<Self, WorkerError> {
        let WorkerResultKind::AnnounceValidated {
            destination,
            public_key,
            verifying_key,
            name_hash,
            app_data,
            ratchet,
        } = kind
        else {
            return Err(WorkerError::InvalidJob {
                message: "worker returned non-announce result for announce validation".to_string(),
            });
        };

        let identity = Identity::new_from_slices(&public_key, &verifying_key);
        let name = DestinationName::new_from_hash_slice(&name_hash);
        let announced = SingleOutputDestination::new(identity, name);
        if announced.desc.address_hash.as_slice() != destination {
            return Err(WorkerError::InvalidJob {
                message: "worker announce result address hash does not match identity/name"
                    .to_string(),
            });
        }

        let ratchet = match ratchet {
            Some(ratchet) => {
                if ratchet.len() != crate::destination::RATCHET_LENGTH {
                    return Err(WorkerError::InvalidJob {
                        message: "worker announce ratchet has invalid length".to_string(),
                    });
                }
                let mut bytes = [0u8; crate::destination::RATCHET_LENGTH];
                bytes.copy_from_slice(ratchet.as_ref());
                Some(bytes)
            }
            None => None,
        };

        Ok(Self {
            destination: announced,
            app_data: PacketDataBuffer::new_from_slice(app_data.as_ref()),
            ratchet,
        })
    }
}

pub(super) fn validate_announce(packet: &Packet) -> Result<ValidatedAnnounce, RnsError> {
    DestinationAnnounce::validate(packet).map(ValidatedAnnounce::from_info)
}

pub(super) async fn validate_announce_on_worker(
    packet: Packet,
    remote_backend: Option<Arc<dyn WorkerBackend>>,
) -> Result<ValidatedAnnounce, RnsError> {
    if let Some(remote_backend) = remote_backend {
        match validate_announce_on_remote_worker(&packet, remote_backend).await {
            Ok(announce) => return Ok(announce),
            Err(err) => {
                log::debug!(
                    "[transport] remote announce worker unavailable, falling back locally: {:?}",
                    err
                );
            }
        }
    }

    validate_announce_on_local_worker(packet).await
}

async fn validate_announce_on_local_worker(packet: Packet) -> Result<ValidatedAnnounce, RnsError> {
    let permit = announce_validation_permits()
        .acquire_owned()
        .await
        .map_err(|_| RnsError::ConnectionError)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        validate_announce(&packet)
    })
    .await
    .map_err(|_| RnsError::ConnectionError)?
}

async fn validate_announce_on_remote_worker(
    packet: &Packet,
    backend: Arc<dyn WorkerBackend>,
) -> Result<ValidatedAnnounce, RnsError> {
    let packet_wire = packet.to_bytes()?;
    let response = backend
        .submit(WorkerJob {
            id: u64::from_be_bytes(packet.hash().as_slice()[..8].try_into().unwrap_or([0; 8])),
            kind: WorkerJobKind::ValidateAnnounce { packet_wire },
        })
        .await
        .map_err(|err| {
            log::debug!("[transport] remote announce worker failed: {:?}", err);
            RnsError::ConnectionError
        })?;

    ValidatedAnnounce::from_worker_result(response.kind).map_err(|err| {
        log::debug!("[transport] remote announce worker returned invalid result: {:?}", err);
        RnsError::PacketError
    })
}

#[cfg(test)]
async fn process_announce<'a>(
    packet: &Packet,
    mut handler: MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
    source: IfaceSource,
    announce: ValidatedAnnounce,
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

        handler.announce_table.add(packet, dest_hash, route_iface);

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
        app_data: announce.app_data,
        ratchet,
        name_hash,
        hops: packet.header.hops,
        interface,
    });

    handler
}

async fn process_announce_unlocked(
    handler_arc: Arc<Mutex<TransportHandler>>,
    packet: &Packet,
    iface: AddressHash,
    source: IfaceSource,
    announce: ValidatedAnnounce,
) {
    let (existing, config_name) = {
        let handler = handler_arc.lock().await;
        (
            handler.single_out_destinations.get(&packet.destination).cloned(),
            handler.config.name.clone(),
        )
    };

    if let Some(existing) = existing {
        match existing.try_lock() {
            Ok(existing) => {
                if existing.identity.public_key != announce.destination.identity.public_key
                    || existing.identity.verifying_key
                        != announce.destination.identity.verifying_key
                {
                    log::warn!(
                        "tp({}): rejecting announce for {} due to identity drift",
                        config_name,
                        packet.destination
                    );
                    return;
                }
            }
            Err(_) => {
                log::debug!(
                    "tp({}): skipping announce for {} while existing destination is busy",
                    config_name,
                    packet.destination
                );
                return;
            }
        }
    }

    let ratchet = announce.ratchet;
    let dest_hash = announce.destination.desc.address_hash;
    let name_hash = {
        let source = announce.destination.desc.name.as_name_hash_slice();
        let mut name_hash = [0u8; crate::destination::NAME_HASH_LENGTH];
        name_hash.copy_from_slice(source);
        name_hash
    };
    let destination = Arc::new(Mutex::new(announce.destination));
    let route_iface =
        TransportHandler::unicast_iface_for_source_unlocked(handler_arc.clone(), iface, source)
            .await
            .unwrap_or(iface);
    let interface = route_iface.as_slice().to_vec();

    let announce_tx = {
        let mut handler = handler_arc.lock().await;
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

        let destination_known = handler.has_destination(&packet.destination)
            || handler.knows_destination(&packet.destination);
        if !destination_known {
            if !handler.single_out_destinations.contains_key(&packet.destination) {
                log::trace!("tp({}): new announce for {}", handler.config.name, packet.destination);
                handler.single_out_destinations.insert(packet.destination, destination.clone());
            }

            handler.announce_table.add(packet, dest_hash, route_iface);
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

        handler.announce_tx.clone()
    };

    let _ = announce_tx.send(AnnounceEvent {
        destination,
        app_data: announce.app_data,
        ratchet,
        name_hash,
        hops: packet.header.hops,
        interface,
    });
}

#[cfg(test)]
pub(super) async fn handle_announce<'a>(
    packet: &Packet,
    handler: MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
    source: IfaceSource,
) {
    let announce = match validate_announce(packet) {
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

    handle_validated_announce(packet, handler, iface, source, announce).await;
}

#[cfg(test)]
pub(super) async fn handle_validated_announce<'a>(
    packet: &Packet,
    handler: MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
    source: IfaceSource,
    announce: ValidatedAnnounce,
) {
    let mut handler = handler;
    let destination_known = handler.has_destination(&packet.destination)
        || handler.knows_destination(&packet.destination);
    if let AnnounceLimitAction::Hold(delay) =
        handler.announce_limits.check(iface, packet, destination_known)
    {
        log::debug!(
            "tp({}): holding announce for {} for {:?}",
            handler.config.name,
            packet.destination,
            delay
        );
        return;
    }

    let _ = process_announce(packet, handler, iface, source, announce).await;
}

pub(super) async fn handle_validated_announce_unlocked(
    packet: &Packet,
    handler_arc: Arc<Mutex<TransportHandler>>,
    iface: AddressHash,
    source: IfaceSource,
    announce: ValidatedAnnounce,
) {
    let action = {
        let mut handler = handler_arc.lock().await;
        let destination_known = handler.has_destination(&packet.destination)
            || handler.knows_destination(&packet.destination);
        let action = handler.announce_limits.check(iface, packet, destination_known);
        if let AnnounceLimitAction::Hold(release_after) = action {
            log::info!(
                "tp({}): holding announce for {} on iface {} for at least {:?}",
                handler.config.name,
                packet.destination,
                iface,
                release_after,
            );
        }
        action
    };

    if matches!(action, AnnounceLimitAction::Hold(_)) {
        return;
    }

    process_announce_unlocked(handler_arc, packet, iface, source, announce).await;
}

pub(super) async fn retransmit_announces(handler_arc: Arc<Mutex<TransportHandler>>) {
    let messages = {
        let mut handler = handler_arc.lock().await;
        let transport_id = *handler.config.identity.address_hash();
        handler.announce_table.drain_retransmissions(&transport_id)
    };

    for message in messages {
        let _ = TransportHandler::send_message_unlocked(handler_arc.clone(), message).await;
    }
}

#[cfg(test)]
pub(super) async fn release_held_announces<'a>(handler: MutexGuard<'a, TransportHandler>) {
    let mut handler = handler;
    let released = handler.announce_limits.release_ready();

    for released_announce in released {
        let packet = released_announce.packet;
        let iface = released_announce.iface;
        let announce = match validate_announce(&packet) {
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

        // Held announces predate auto-unicast redirection because we
        // don't persist the `IfaceSource` across the hold queue.
        // Replay on the stored iface; if that iface was multicast, the
        // route won't get the unicast redirect until the next fresh
        // announce from this peer arrives.
        handler = process_announce(&packet, handler, iface, IfaceSource::None, announce).await;
    }
}

pub(super) async fn release_held_announces_unlocked(handler_arc: Arc<Mutex<TransportHandler>>) {
    let released = { handler_arc.lock().await.announce_limits.release_ready() };

    for released_announce in released {
        let packet = released_announce.packet;
        let iface = released_announce.iface;
        let announce = match validate_announce(&packet) {
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

        // Held announces predate auto-unicast redirection because we
        // don't persist the `IfaceSource` across the hold queue.
        // Replay on the stored iface; if that iface was multicast, the
        // route won't get the unicast redirect until the next fresh
        // announce from this peer arrives.
        process_announce_unlocked(handler_arc.clone(), &packet, iface, IfaceSource::None, announce)
            .await;
    }
}

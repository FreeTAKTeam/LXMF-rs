use super::announce_limits::AnnounceLimitAction;
use super::path_table::AnnouncePolicyInput;
use super::*;
use crate::identity::Identity;
use crate::packet::{Header, HeaderType, PropagationType};

/// Identity-drift invariant (issue #517), matching reference Reticulum's
/// `Identity.validate_announce`: the ONLY cross-announce binding for a
/// known destination is the public/verifying key pair.
///
/// The destination-hash ↔ name-hash binding is enforced separately by
/// `DestinationAnnounce::validate` (it recomputes the address hash from
/// the announced identity + name hash), so name_hash cannot drift
/// independently of the destination hash. app_data must NOT be compared
/// here — the reference implementation overwrites app_data on every
/// accepted announce, so rejecting on app_data change would break
/// legitimate announces (e.g. LXMF peers rotating stamp-cost metadata)
/// and diverge from protocol behavior.
fn identity_drifted(existing: &Identity, announced: &Identity) -> bool {
    existing.public_key != announced.public_key || existing.verifying_key != announced.verifying_key
}

impl TransportHandler {
    pub(super) fn is_identity_blackholed(&mut self, identity: &AddressHash) -> bool {
        let expired = self
            .blackholed_identities
            .get(identity)
            .and_then(|until| *until)
            .is_some_and(|until| now_secs() > until);
        if expired {
            self.blackholed_identities.remove(identity);
            false
        } else {
            self.blackholed_identities.contains_key(identity)
        }
    }
}

pub(super) async fn admit_announce_before_queue(
    packet: &Packet,
    handler_arc: &Arc<Mutex<TransportHandler>>,
    iface: AddressHash,
    source: IfaceSource,
) -> bool {
    let announce = match DestinationAnnounce::validate(packet) {
        Ok(result) => result,
        Err(err) => {
            let iface_manager = handler_arc.lock().await.iface_manager.clone();
            iface_manager
                .lock()
                .await
                .record_protocol_violation(iface, "invalid announce signature");
            log::debug!(
                "dropping invalid announce before queue iface={} dst={} err={err:?}",
                iface,
                packet.destination
            );
            return false;
        }
    };

    let mut handler = handler_arc.lock().await;
    if handler.is_identity_blackholed(&announce.destination.identity.address_hash) {
        log::debug!(
            "dropping announce from blackholed identity {} before queue for {}",
            announce.destination.identity.address_hash,
            packet.destination
        );
        return false;
    }
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
            "tp({}): holding announce for {} before queue for {:?}",
            handler.config.name,
            packet.destination,
            delay
        );
        return false;
    }
    true
}

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
        // Centralized drift invariant (issue #517): see `identity_drifted`.
        if identity_drifted(&existing.identity, &announce.destination.identity) {
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
    let (from_local_client, local_client_interfaces) = {
        let manager = handler.iface_manager.lock().await;
        (manager.is_local_client_interface(&iface), manager.local_client_interfaces())
    };

    let path_accepted = if local_destination_known {
        false
    } else {
        let existing_path_iface =
            handler.path_table.get(&packet.destination).map(|entry| entry.iface);
        let (incoming_policy, existing_path_policy) = {
            let manager = handler.iface_manager.lock().await;
            (
                manager.policy(&route_iface),
                existing_path_iface.and_then(|iface| manager.policy(&iface)),
            )
        };
        handler.path_table.handle_announce_with_policy(AnnouncePolicyInput {
            announce: packet,
            transport_id: packet.transport,
            iface: route_iface,
            random_blob: announce.random_blob,
            incoming_policy: |_: &AddressHash| incoming_policy,
            now: std::time::Instant::now(),
            policy_for_iface: |iface: &AddressHash| {
                (Some(*iface) == existing_path_iface).then_some(existing_path_policy).flatten()
            },
        })
    };

    if path_accepted {
        if !handler.single_out_destinations.contains_key(&packet.destination) {
            log::trace!("tp({}): new announce for {}", handler.config.name, packet.destination);

            handler.single_out_destinations.insert(packet.destination, destination.clone());
        }

        // Reference parity (`Transport.py:2267`): an announce is only filed for
        // retransmission on a node that will actually retransmit it.
        //
        //     if (transport_enabled() or is_from_local_client) and context != PATH_RESPONSE:
        //         if rate_blocked: RNS.log("Blocking rebroadcast ...")
        //         else:            announce_table[destination_hash] = [...]
        //
        // The inner rate-limit branch was already here; the outer condition was
        // not, so `map` — which only `drain_retransmissions` prunes, and which
        // only the retransmit worker drains, and only when `transport_enabled`
        // — grew without bound on a passive node, one cloned `Packet` per
        // distinct destination for the life of the process.
        //
        // Everything that is not queued is still cached rather than dropped.
        // Unlike the reference, this crate rebuilds a path entry's announce
        // packet out of the announce table when persisting the path table
        // (`save_reticulum_path_table` -> `cached_packet_for_destination`),
        // where the reference stores a packet hash and keeps the packet in its
        // own on-disk cache. Dropping it outright here would persist an empty
        // path table on exactly the nodes this guard is meant to help. The
        // cache is the bounded half of the table (`announce_cache_capacity`),
        // which is what it is for.
        let queue_for_retransmission = (handler.config.transport_enabled || from_local_client)
            && packet.context != PacketContext::PathResponse;
        let rate_blocked =
            handler.announce_limits.should_suppress_rebroadcast(packet, &shared_config);

        if rate_blocked {
            log::debug!(
                "tp({}): suppressing announce rebroadcast for {} due to announce_rate_target",
                handler.config.name,
                packet.destination
            );
        }

        if queue_for_retransmission && !rate_blocked {
            if from_local_client {
                handler.announce_table.add_local_client(packet, dest_hash, route_iface);
            } else {
                handler.announce_table.add(packet, dest_hash, route_iface);
            }
        } else {
            handler.announce_table.add_cached(packet, dest_hash, route_iface);
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

        // Python Reticulum fans accepted announces out to every attached
        // local client immediately. Use direct child targets rather than a
        // broadcast so the announce cannot accidentally transit the network,
        // and never echo it back to the child that supplied the announce.
        for local_client_iface in local_client_interfaces {
            if local_client_iface == iface {
                continue;
            }
            let local_announce = Packet {
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
                context: PacketContext::None,
                data: packet.data.clone(),
            };
            handler
                .send(TxMessage {
                    tx_type: TxMessageType::Direct(local_client_iface),
                    packet: local_announce,
                })
                .await;
        }
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
    if handler.is_identity_blackholed(&announce.destination.identity.address_hash) {
        log::debug!(
            "dropping announce from blackholed identity {} for {}",
            announce.destination.identity.address_hash,
            packet.destination
        );
        return;
    }

    let shared_config = {
        let manager = handler.iface_manager.lock().await;
        manager.shared_config(&iface).cloned().unwrap_or_default()
    };
    let _ = process_announce(packet, handler, iface, source, announce, shared_config).await;
}

pub(super) async fn handle_ingress_limited_announce<'a>(
    packet: &Packet,
    mut handler: MutexGuard<'a, TransportHandler>,
    iface: AddressHash,
    source: IfaceSource,
) {
    let announce = match DestinationAnnounce::validate(packet) {
        Ok(result) => result,
        Err(err) => {
            log::warn!(
                "dropping ingress-limited announce for {} after revalidate failure: {:?}",
                packet.destination,
                err
            );
            return;
        }
    };
    if handler.is_identity_blackholed(&announce.destination.identity.address_hash) {
        log::debug!(
            "dropping ingress-limited announce from blackholed identity {} for {}",
            announce.destination.identity.address_hash,
            packet.destination
        );
        return;
    }
    let shared_config = {
        let manager = handler.iface_manager.lock().await;
        manager.shared_config(&iface).cloned().unwrap_or_default()
    };
    let _ = process_announce(packet, handler, iface, source, announce, shared_config).await;
}

#[cfg(test)]
pub(super) async fn retransmit_announces<'a>(handler: MutexGuard<'a, TransportHandler>) {
    retransmit_announces_at(handler, Instant::now()).await;
}

async fn retransmit_announces_at<'a>(mut handler: MutexGuard<'a, TransportHandler>, now: Instant) {
    let transport_id = *handler.config.identity.address_hash();
    let messages = handler.announce_table.drain_retransmissions_at(&transport_id, now);

    for message in messages {
        handler.send(message).await;
    }
}

pub(super) async fn announce_retransmit_tick(
    handler_arc: &Arc<Mutex<TransportHandler>>,
    now: Instant,
) {
    retransmit_announces_at(handler_arc.lock().await, now).await;
    release_held_announces(handler_arc.lock().await).await;

    let iface_manager = handler_arc.lock().await.iface_manager.clone();
    iface_manager.lock().await.release_queued_announces().await;
}

pub(super) async fn release_held_announces<'a>(handler: MutexGuard<'a, TransportHandler>) {
    let mut handler = handler;
    let released = handler.announce_limits.release_ready();
    let inbound_queues = handler.inbound_queues.clone();

    for released_announce in released {
        let queued = QueuedInbound {
            message: RxMessage {
                address: released_announce.iface,
                packet: released_announce.packet,
                source: released_announce.source,
            },
            ingress_limited: true,
            packet_cache_inserted: false,
            path_request: None,
        };
        if let Err(full) = inbound_queues.enqueue(InboundTrafficClass::IngressLimited, queued) {
            log::warn!(
                "dropping released announce because ingress-limited queue is full iface={} hash={}",
                full.item.message.address,
                full.item.message.packet.hash()
            );
        }
    }
}

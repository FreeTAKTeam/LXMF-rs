use super::diag;
use super::wire::should_encrypt_packet;
use super::worker_boundary::OutboundEncryptBatchItem;
use super::*;

pub(super) const MAX_OUTBOUND_ENCRYPTION_WORKERS: usize = 4;

static OUTBOUND_ENCRYPTION_PERMITS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

pub(super) fn outbound_encryption_permits() -> Arc<tokio::sync::Semaphore> {
    OUTBOUND_ENCRYPTION_PERMITS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_OUTBOUND_ENCRYPTION_WORKERS)))
        .clone()
}

struct OutboundEncryptionContext {
    public_key: [u8; crate::identity::PUBLIC_KEY_LENGTH],
    salt: AddressHash,
    config_name: String,
    remote_batch_lane: Option<crypto_batch_lane::OutboundCryptoBatchLane>,
}

struct UnlockedDispatchContext {
    packet_cache: Arc<Mutex<PacketCache>>,
    iface_manager: Arc<Mutex<InterfaceManager>>,
    link_candidates: Vec<Arc<Mutex<Link>>>,
}

fn send_packet_trace(outcome: SendPacketOutcome) -> SendPacketTrace {
    SendPacketTrace {
        outcome,
        direct_iface: None,
        broadcast: false,
        dispatch: TxDispatchTrace::default(),
    }
}

fn link_candidates_for_packet(
    handler: &TransportHandler,
    packet: &Packet,
) -> Vec<Arc<Mutex<Link>>> {
    if packet.header.packet_type == PacketType::LinkRequest {
        return handler.out_links.values().cloned().collect();
    }

    if packet.header.destination_type != DestinationType::Link {
        return Vec::new();
    }

    if let Some(link) = handler.in_links.get(&packet.destination).cloned() {
        return vec![link];
    }

    handler.out_links.values().cloned().collect()
}

async fn note_link_packet_sent_from_candidates(packet: &Packet, candidates: Vec<Arc<Mutex<Link>>>) {
    if candidates.is_empty() {
        return;
    }

    let requested_id = if packet.header.packet_type == PacketType::LinkRequest {
        Some(crate::destination::link::LinkId::from(packet))
    } else {
        None
    };

    for candidate in candidates {
        let Ok(mut link) = candidate.try_lock() else {
            log::debug!("tp: skipping busy link while noting outbound packet dispatch");
            continue;
        };
        let matches_packet = if let Some(requested_id) = requested_id {
            *link.id() == requested_id
        } else {
            *link.id() == packet.destination
        };
        if matches_packet {
            link.note_outbound(packet.context);
            break;
        }
    }
}

async fn dispatch_message_unlocked(
    message: TxMessage,
    context: UnlockedDispatchContext,
    announce_policy: Option<AnnounceBroadcastPolicy>,
) -> TxDispatchTrace {
    let packet = message.packet;
    context.packet_cache.lock().await.update(&packet);
    let (trace, work) = {
        context.iface_manager.lock().await.plan_send_with_announce_policy(message, announce_policy)
    };
    let dispatch = InterfaceManager::dispatch_tx_work(trace, work).await;
    if dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0 {
        note_link_packet_sent_from_candidates(&packet, context.link_candidates).await;
    }
    dispatch
}

async fn encrypt_packet_on_worker(
    packet: Packet,
    context: OutboundEncryptionContext,
) -> Result<Packet, SendPacketTrace> {
    if let Some(remote_batch_lane) = context.remote_batch_lane.clone() {
        match encrypt_packet_on_remote_worker(&packet, &context, remote_batch_lane).await {
            Ok(packet) => return Ok(packet),
            Err(trace) => {
                log::debug!(
                    "tp({}): remote outbound encryption worker unavailable, falling back locally",
                    context.config_name
                );
                if trace.outcome != SendPacketOutcome::DroppedEncryptFailed {
                    return Err(trace);
                }
            }
        }
    }

    encrypt_packet_on_local_worker(packet, context).await
}

async fn encrypt_packet_on_local_worker(
    mut packet: Packet,
    context: OutboundEncryptionContext,
) -> Result<Packet, SendPacketTrace> {
    let permit = outbound_encryption_permits()
        .try_acquire_owned()
        .map_err(|_| send_packet_trace(SendPacketOutcome::DroppedEncryptFailed))?;
    let plaintext = packet.data;
    let public_key = context.public_key;
    let salt = context.salt;
    let encrypted = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut buffer = PacketDataBuffer::new();
        let ciphertext_len = encrypt_for_public_key_into(
            &x25519_dalek::PublicKey::from(public_key),
            salt.as_slice(),
            plaintext.as_slice(),
            buffer.accuire_buf_max(),
            OsRng,
        )?
        .len();
        buffer.resize(ciphertext_len);
        Ok::<PacketDataBuffer, RnsError>(buffer)
    })
    .await
    .map_err(|_| send_packet_trace(SendPacketOutcome::DroppedEncryptFailed))?;

    match encrypted {
        Ok(buffer) => {
            packet.data = buffer;
            Ok(packet)
        }
        Err(err) => {
            if matches!(err, RnsError::InvalidArgument) {
                log::warn!(
                    "tp({}): ciphertext too large for packet to {}",
                    context.config_name,
                    packet.destination
                );
                return Err(send_packet_trace(SendPacketOutcome::DroppedCiphertextTooLarge));
            }
            log::warn!(
                "tp({}): encrypt failed for {}: {:?}",
                context.config_name,
                packet.destination,
                err
            );
            Err(send_packet_trace(SendPacketOutcome::DroppedEncryptFailed))
        }
    }
}

async fn encrypt_packet_on_remote_worker(
    packet: &Packet,
    context: &OutboundEncryptionContext,
    batch_lane: crypto_batch_lane::OutboundCryptoBatchLane,
) -> Result<Packet, SendPacketTrace> {
    let packet_wire = packet
        .to_bytes()
        .map_err(|_| send_packet_trace(SendPacketOutcome::DroppedEncryptFailed))?;
    let packet_wire = batch_lane
        .encrypt(OutboundEncryptBatchItem {
            packet_wire,
            public_key: context.public_key,
            salt: {
                let mut salt = [0u8; crate::hash::ADDRESS_HASH_SIZE];
                salt.copy_from_slice(context.salt.as_slice());
                salt
            },
        })
        .await
        .map_err(|err| {
            log::debug!(
                "tp({}): remote outbound encryption failed: {:?}",
                context.config_name,
                err
            );
            send_packet_trace(SendPacketOutcome::DroppedEncryptFailed)
        })?
        .packet_wire;
    Packet::from_bytes(packet_wire.as_slice()).map_err(|err| {
        log::debug!(
            "tp({}): remote outbound encryption returned invalid packet: {:?}",
            context.config_name,
            err
        );
        send_packet_trace(SendPacketOutcome::DroppedEncryptFailed)
    })
}

impl TransportHandler {
    async fn outbound_encryption_context_unlocked(
        handler: Arc<Mutex<TransportHandler>>,
        packet: &Packet,
    ) -> Result<Option<OutboundEncryptionContext>, SendPacketTrace> {
        if !should_encrypt_packet(packet) {
            return Ok(None);
        }

        let (destination, config_name, remote_batch_lane) = {
            let handler = handler.lock().await;
            (
                handler.single_out_destinations.get(&packet.destination).cloned(),
                handler.config.name.clone(),
                handler.outbound_crypto_batch_lane.clone(),
            )
        };
        let Some(destination) = destination else {
            log::warn!(
                "tp({}): missing destination identity for {}",
                config_name,
                packet.destination
            );
            return Err(send_packet_trace(SendPacketOutcome::DroppedMissingDestinationIdentity));
        };
        let identity = match destination.try_lock() {
            Ok(destination) => destination.identity,
            Err(_) => {
                log::debug!(
                    "tp({}): skipping outbound encryption for busy destination {}",
                    config_name,
                    packet.destination
                );
                return Err(send_packet_trace(SendPacketOutcome::DroppedEncryptFailed));
            }
        };
        let salt = identity.address_hash;
        let ratchet = {
            let mut handler = handler.lock().await;
            handler.ratchet_store.as_mut().and_then(|store| store.get(&packet.destination))
        };
        let public_key = ratchet
            .map(|ratchet| *PublicKey::from(ratchet).as_bytes())
            .unwrap_or(*identity.public_key.as_bytes());
        Ok(Some(OutboundEncryptionContext { public_key, salt, config_name, remote_batch_lane }))
    }

    pub(super) async fn send_packet_with_trace_unlocked(
        handler: Arc<Mutex<TransportHandler>>,
        packet: Packet,
    ) -> SendPacketTrace {
        if packet.header.packet_type == PacketType::Proof {
            log::trace!(
                "[tp] send_proof dst={} ctx={:02x}",
                packet.destination,
                packet.context as u8
            );
            if packet.context == PacketContext::LinkRequestProof {
                if let Ok(raw) = packet.to_bytes() {
                    log::trace!("[tp] lrproof_raw len={} hex={}", raw.len(), bytes_to_hex(&raw));
                }
            }
        }

        let encryption_context = {
            match Self::outbound_encryption_context_unlocked(handler.clone(), &packet).await {
                Ok(result) => result,
                Err(trace) => return trace,
            }
        };
        let packet = if let Some(context) = encryption_context {
            match encrypt_packet_on_worker(packet, context).await {
                Ok(packet) => packet,
                Err(trace) => return trace,
            }
        } else {
            packet
        };

        let (config_name, route, broadcast, local_destination, dispatch_context) = {
            let handler = handler.lock().await;
            diag::log_route_lookup(&handler.path_table, &packet.destination);
            let route = super::path::route_outbound_packet(&handler.path_table, &packet);
            let dispatch_context = UnlockedDispatchContext {
                packet_cache: handler.packet_cache.clone(),
                iface_manager: handler.iface_manager.clone(),
                link_candidates: link_candidates_for_packet(&handler, &route.packet),
            };
            (
                handler.config.name.clone(),
                route,
                handler.config.broadcast,
                handler.single_in_destinations.contains_key(&route.packet.destination),
                dispatch_context,
            )
        };

        let packet = route.packet;
        if let Some(iface) = route.next_iface {
            let dispatch = dispatch_message_unlocked(
                TxMessage { tx_type: TxMessageType::Direct(iface), packet },
                dispatch_context,
                None,
            )
            .await;
            let outcome = if dispatch.sent_ifaces > 0 {
                SendPacketOutcome::SentDirect
            } else {
                SendPacketOutcome::DroppedNoRoute
            };
            diag::log_direct_send(iface, outcome, &dispatch);
            SendPacketTrace { outcome, direct_iface: Some(iface), broadcast: false, dispatch }
        } else if broadcast || packet.header.packet_type == PacketType::Announce {
            let next_hop_iface = {
                let handler = handler.lock().await;
                handler.path_table.next_hop_iface(&packet.destination)
            };
            let next_hop_iface_mode = if let Some(iface) = next_hop_iface {
                dispatch_context.iface_manager.lock().await.mode(&iface)
            } else {
                None
            };
            let announce_policy = if packet.header.packet_type == PacketType::Announce {
                Some(AnnounceBroadcastPolicy { local_destination, next_hop_iface_mode })
            } else {
                None
            };
            let dispatch = dispatch_message_unlocked(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet },
                dispatch_context,
                announce_policy,
            )
            .await;
            let outcome = if dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0 {
                SendPacketOutcome::SentBroadcast
            } else {
                SendPacketOutcome::DroppedNoRoute
            };
            diag::log_broadcast_send(outcome, &dispatch);
            SendPacketTrace { outcome, direct_iface: None, broadcast: true, dispatch }
        } else {
            log::trace!(
                "tp({}): no route for outbound packet dst={}",
                config_name,
                packet.destination
            );
            send_packet_trace(SendPacketOutcome::DroppedNoRoute)
        }
    }

    pub(super) async fn send_message_unlocked(
        handler: Arc<Mutex<TransportHandler>>,
        message: TxMessage,
    ) -> TxDispatchTrace {
        let packet = message.packet;
        let (dispatch_context, local_destination, next_hop_iface) = {
            let handler = handler.lock().await;
            (
                UnlockedDispatchContext {
                    packet_cache: handler.packet_cache.clone(),
                    iface_manager: handler.iface_manager.clone(),
                    link_candidates: link_candidates_for_packet(&handler, &packet),
                },
                handler.single_in_destinations.contains_key(&packet.destination),
                handler.path_table.next_hop_iface(&packet.destination),
            )
        };
        let next_hop_iface_mode = if packet.header.packet_type == PacketType::Announce
            && matches!(message.tx_type, TxMessageType::Broadcast(_))
        {
            if let Some(iface) = next_hop_iface {
                dispatch_context.iface_manager.lock().await.mode(&iface)
            } else {
                None
            }
        } else {
            None
        };
        let announce_policy = if packet.header.packet_type == PacketType::Announce
            && matches!(message.tx_type, TxMessageType::Broadcast(_))
        {
            Some(AnnounceBroadcastPolicy { local_destination, next_hop_iface_mode })
        } else {
            None
        };

        dispatch_message_unlocked(message, dispatch_context, announce_policy).await
    }

    pub(super) fn has_destination(&self, address: &AddressHash) -> bool {
        self.single_in_destinations.contains_key(address)
    }

    pub(super) fn knows_destination(&self, address: &AddressHash) -> bool {
        self.single_out_destinations.contains_key(address)
    }

    pub(super) async fn filter_duplicate_packets_unlocked(
        handler: Arc<Mutex<TransportHandler>>,
        packet: &Packet,
    ) -> bool {
        if packet.header.packet_type == PacketType::Announce {
            return true;
        }

        let (packet_cache, in_link) = {
            let handler = handler.lock().await;
            let in_link = if packet.header.packet_type == PacketType::Proof
                && packet.context == PacketContext::LinkRequestProof
            {
                handler.in_links.get(&packet.destination).cloned()
            } else {
                None
            };
            (handler.packet_cache.clone(), in_link)
        };

        let mut allow_duplicate = matches!(
            (packet.header.packet_type, packet.context),
            (PacketType::LinkRequest, _)
                | (PacketType::Data, PacketContext::KeepAlive | PacketContext::LinkClose)
        );

        if let Some(link) = in_link {
            match link.try_lock() {
                Ok(link) if link.status().not_yet_active() => {
                    allow_duplicate = true;
                }
                Ok(_) => {}
                Err(_) => {
                    log::debug!(
                        "tp: allowing link-request-proof duplicate while inbound link is busy"
                    );
                    allow_duplicate = true;
                }
            }
        }

        let is_new = packet_cache.lock().await.update(packet);

        is_new || allow_duplicate
    }

    #[cfg(test)]
    pub(super) async fn filter_duplicate_packets(&self, packet: &Packet) -> bool {
        let mut allow_duplicate = false;

        match packet.header.packet_type {
            PacketType::Announce => {
                return true;
            }
            PacketType::LinkRequest => {
                allow_duplicate = true;
            }
            PacketType::Data => {
                allow_duplicate = matches!(
                    packet.context,
                    PacketContext::KeepAlive
                        | PacketContext::LinkClose
                        | PacketContext::ResourceRequest
                );
            }
            PacketType::Proof => {
                if packet.context == PacketContext::LinkRequestProof {
                    if let Some(link) = self.in_links.get(&packet.destination) {
                        if link.lock().await.status().not_yet_active() {
                            allow_duplicate = true;
                        }
                    }
                }
            }
        }

        let is_new = self.packet_cache.lock().await.update(packet);
        if !is_new
            && packet.header.destination_type == DestinationType::Link
            && matches!(
                packet.context,
                PacketContext::Resource
                    | PacketContext::ResourceAdvrtisement
                    | PacketContext::ResourceRequest
                    | PacketContext::ResourceHashUpdate
                    | PacketContext::ResourceProof
            )
            && diag::enabled()
        {
            log::debug!(
                "[resource-diag] duplicate_drop_candidate node={} link={} ctx={:02x}",
                self.config.name,
                packet.destination,
                packet.context as u8
            );
        }

        is_new || allow_duplicate
    }

    pub(super) async fn gc_unicast_ifaces_unlocked(handler: Arc<Mutex<TransportHandler>>) {
        let now = Instant::now();
        let (stale_ifaces, routing_maps, iface_manager, config_name) = {
            let mut handler = handler.lock().await;
            let stale: Vec<std::net::SocketAddr> = handler
                .unicast_udp_ifaces
                .iter()
                .filter(|(_, (_, last_seen))| {
                    now.duration_since(*last_seen) > UNICAST_IFACE_IDLE_TIMEOUT
                })
                .map(|(peer, _)| *peer)
                .collect();

            if stale.is_empty() {
                return;
            }

            let stale_ifaces: Vec<_> = stale
                .into_iter()
                .filter_map(|peer| {
                    handler
                        .unicast_udp_ifaces
                        .remove(&peer)
                        .map(|(iface_hash, _)| (peer, iface_hash))
                })
                .collect();

            (
                stale_ifaces,
                handler.multicast_peer_routings.values().cloned().collect::<Vec<_>>(),
                handler.iface_manager.clone(),
                handler.config.name.clone(),
            )
        };

        for (peer, iface_hash) in stale_ifaces {
            let mut removed_from_routing = false;
            for routing in &routing_maps {
                if routing.lock().await.remove_by_hash(&iface_hash).is_some() {
                    removed_from_routing = true;
                    break;
                }
            }
            let _ = removed_from_routing;
            iface_manager.lock().await.stop_interface(iface_hash);
            log::debug!(
                "tp({}): GC'd idle virtual UDP iface {} for peer {}",
                config_name,
                iface_hash,
                peer
            );
        }
    }

    /// Register (or refresh) the *virtual* unicast iface that the
    /// transport uses to route point-to-point traffic for the peer
    /// that delivered this packet. Only acts when:
    ///   - the packet arrived on a multicast iface, and
    ///   - that multicast iface has a registered `PeerRouting` map
    ///     (i.e. it was registered via
    ///     `Transport::add_multicast_udp_interface`), and
    ///   - the source is a UDP socket address.
    ///
    /// Returns the virtual iface hash to stick in the path_table so
    /// subsequent `Direct` tx for this peer's destinations is routed
    /// through the host multicast socket as a unicast send — and,
    /// symmetrically, so inbound replies from this peer (which arrive
    /// on the host multicast socket) get re-attributed to this same
    /// virtual iface by the host's rx task. That symmetry is what
    /// makes `Link::iface_matches` succeed on the proof/keepalive.
    #[cfg(test)]
    pub(super) async fn unicast_iface_for_source(
        &mut self,
        rx_iface: AddressHash,
        source: IfaceSource,
    ) -> Option<AddressHash> {
        let peer = match source {
            IfaceSource::Udp(addr) => addr,
            IfaceSource::None => return None,
        };

        let role = { self.iface_manager.lock().await.role(&rx_iface) };
        if role != Some(IfaceRole::Multicast) {
            return None;
        }

        let peer_routing = self.multicast_peer_routings.get(&rx_iface).cloned()?;

        let now = Instant::now();
        if let Some(entry) = self.unicast_udp_ifaces.get_mut(&peer) {
            entry.1 = now;
            return Some(entry.0);
        }

        let virtual_hash = {
            let mut mgr = self.iface_manager.lock().await;
            mgr.register_virtual_iface(rx_iface, IfaceRole::VirtualUnicast)?
        };
        peer_routing.lock().await.insert(peer, virtual_hash);
        log::debug!(
            "tp({}): registered virtual UDP iface {} for peer {} on host {}",
            self.config.name,
            virtual_hash,
            peer,
            rx_iface,
        );
        self.unicast_udp_ifaces.insert(peer, (virtual_hash, now));
        Some(virtual_hash)
    }

    pub(super) async fn unicast_iface_for_source_unlocked(
        handler: Arc<Mutex<TransportHandler>>,
        rx_iface: AddressHash,
        source: IfaceSource,
    ) -> Option<AddressHash> {
        let peer = match source {
            IfaceSource::Udp(addr) => addr,
            IfaceSource::None => return None,
        };

        let (iface_manager, peer_routing, config_name) = {
            let mut handler = handler.lock().await;
            if let Some(entry) = handler.unicast_udp_ifaces.get_mut(&peer) {
                entry.1 = Instant::now();
                return Some(entry.0);
            }

            (
                handler.iface_manager.clone(),
                handler.multicast_peer_routings.get(&rx_iface).cloned(),
                handler.config.name.clone(),
            )
        };

        if iface_manager.lock().await.role(&rx_iface) != Some(IfaceRole::Multicast) {
            return None;
        }

        let peer_routing = peer_routing?;

        {
            let mut handler = handler.lock().await;
            if let Some(entry) = handler.unicast_udp_ifaces.get_mut(&peer) {
                entry.1 = Instant::now();
                return Some(entry.0);
            }
        }

        let virtual_hash = {
            let mut mgr = iface_manager.lock().await;
            mgr.register_virtual_iface(rx_iface, IfaceRole::VirtualUnicast)?
        };
        peer_routing.lock().await.insert(peer, virtual_hash);

        {
            let mut handler = handler.lock().await;
            handler.unicast_udp_ifaces.insert(peer, (virtual_hash, Instant::now()));
        }

        log::debug!(
            "tp({}): registered virtual UDP iface {} for peer {} on host {}",
            config_name,
            virtual_hash,
            peer,
            rx_iface,
        );
        Some(virtual_hash)
    }

    /// Register a `PeerRouting` map for a multicast iface at
    /// construction time. Called by
    /// `Transport::add_multicast_udp_interface`.
    pub(super) fn register_multicast_peer_routing(
        &mut self,
        iface: AddressHash,
        routing: Arc<Mutex<crate::iface::udp::PeerRouting>>,
    ) {
        self.multicast_peer_routings.insert(iface, routing);
    }

    /// Drop virtual unicast ifaces that haven't seen a fresh announce
    /// from their peer in `UNICAST_IFACE_IDLE_TIMEOUT`. Also clears
    /// the corresponding entry from the host multicast iface's
    /// `PeerRouting`, so future packets from that peer are reattributed
    /// to the multicast iface (re-triggering a fresh virtual iface
    /// registration if the peer reappears). Called from
    /// `handle_cleanup`.
    #[cfg(test)]
    pub(super) async fn gc_unicast_ifaces(&mut self) {
        let now = Instant::now();
        let stale: Vec<std::net::SocketAddr> = self
            .unicast_udp_ifaces
            .iter()
            .filter(|(_, (_, last_seen))| {
                now.duration_since(*last_seen) > UNICAST_IFACE_IDLE_TIMEOUT
            })
            .map(|(peer, _)| *peer)
            .collect();

        if stale.is_empty() {
            return;
        }

        for peer in stale {
            if let Some((iface_hash, _)) = self.unicast_udp_ifaces.remove(&peer) {
                let mut removed_from_routing = false;
                for routing in self.multicast_peer_routings.values() {
                    if routing.lock().await.remove_by_hash(&iface_hash).is_some() {
                        removed_from_routing = true;
                        break;
                    }
                }
                let _ = removed_from_routing;
                self.iface_manager.lock().await.stop_interface(iface_hash);
                log::debug!(
                    "tp({}): GC'd idle virtual UDP iface {} for peer {}",
                    self.config.name,
                    iface_hash,
                    peer,
                );
            }
        }
    }
}

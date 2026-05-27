use super::diag;
use super::wire::should_encrypt_packet;
use super::*;

impl TransportHandler {
    async fn note_link_packet_sent(&self, packet: &Packet) {
        let link = if packet.header.packet_type == PacketType::LinkRequest {
            let requested_id = crate::destination::link::LinkId::from(packet);
            let mut matched = None;
            for candidate in self.out_links.values() {
                if *candidate.lock().await.id() == requested_id {
                    matched = Some(candidate.clone());
                    break;
                }
            }
            matched
        } else if packet.header.destination_type == DestinationType::Link {
            if let Some(link) = self.in_links.get(&packet.destination).cloned() {
                Some(link)
            } else {
                let mut matched = None;
                for candidate in self.out_links.values() {
                    if *candidate.lock().await.id() == packet.destination {
                        matched = Some(candidate.clone());
                        break;
                    }
                }
                matched
            }
        } else {
            None
        };

        if let Some(link) = link {
            link.lock().await.note_outbound(packet.context);
        }
    }

    pub(super) async fn send_packet(&mut self, packet: Packet) {
        let _ = self.send_packet_with_trace(packet).await;
    }

    pub(super) async fn send_packet_with_outcome(&mut self, packet: Packet) -> SendPacketOutcome {
        self.send_packet_with_trace(packet).await.outcome
    }

    pub(super) async fn send_packet_with_trace(&mut self, mut packet: Packet) -> SendPacketTrace {
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
        if should_encrypt_packet(&packet) {
            let destination = self.single_out_destinations.get(&packet.destination).cloned();
            let Some(destination) = destination else {
                log::warn!(
                    "tp({}): missing destination identity for {}",
                    self.config.name,
                    packet.destination
                );
                return SendPacketTrace {
                    outcome: SendPacketOutcome::DroppedMissingDestinationIdentity,
                    direct_iface: None,
                    broadcast: false,
                    dispatch: TxDispatchTrace::default(),
                };
            };
            let identity = destination.lock().await.identity;
            let salt = identity.address_hash.as_slice();
            let ratchet =
                self.ratchet_store.as_mut().and_then(|store| store.get(&packet.destination));
            let public_key = ratchet.map(PublicKey::from).unwrap_or(identity.public_key);
            match encrypt_for_public_key(&public_key, salt, packet.data.as_slice(), OsRng) {
                Ok(ciphertext) => {
                    let mut buffer = PacketDataBuffer::new();
                    if buffer.write(&ciphertext).is_err() {
                        log::warn!(
                            "tp({}): ciphertext too large for packet to {}",
                            self.config.name,
                            packet.destination
                        );
                        return SendPacketTrace {
                            outcome: SendPacketOutcome::DroppedCiphertextTooLarge,
                            direct_iface: None,
                            broadcast: false,
                            dispatch: TxDispatchTrace::default(),
                        };
                    }
                    packet.data = buffer;
                }
                Err(err) => {
                    log::warn!(
                        "tp({}): encrypt failed for {}: {:?}",
                        self.config.name,
                        packet.destination,
                        err
                    );
                    return SendPacketTrace {
                        outcome: SendPacketOutcome::DroppedEncryptFailed,
                        direct_iface: None,
                        broadcast: false,
                        dispatch: TxDispatchTrace::default(),
                    };
                }
            }
        }

        diag::log_route_lookup(&self.path_table, &packet.destination);

        let route = super::path::route_outbound_packet(&self.path_table, &packet);
        let packet = route.packet;
        if let Some(iface) = route.next_iface {
            let dispatch =
                self.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
            let outcome = if dispatch.sent_ifaces > 0 {
                SendPacketOutcome::SentDirect
            } else {
                SendPacketOutcome::DroppedNoRoute
            };
            diag::log_direct_send(iface, outcome, &dispatch);
            SendPacketTrace { outcome, direct_iface: Some(iface), broadcast: false, dispatch }
        } else if self.config.broadcast || packet.header.packet_type == PacketType::Announce {
            let dispatch =
                self.send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet }).await;
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
                self.config.name,
                packet.destination
            );
            SendPacketTrace {
                outcome: SendPacketOutcome::DroppedNoRoute,
                direct_iface: None,
                broadcast: false,
                dispatch: TxDispatchTrace::default(),
            }
        }
    }

    pub(super) async fn send(&self, message: TxMessage) -> TxDispatchTrace {
        let packet = message.packet;
        self.packet_cache.lock().await.update(&packet);
        let announce_policy = if packet.header.packet_type == PacketType::Announce
            && matches!(message.tx_type, TxMessageType::Broadcast(_))
        {
            let next_hop_iface = self.path_table.next_hop_iface(&packet.destination);
            let mut mgr = self.iface_manager.lock().await;
            let policy = AnnounceBroadcastPolicy {
                local_destination: self.single_in_destinations.contains_key(&packet.destination),
                next_hop_iface_mode: next_hop_iface.and_then(|iface| mgr.mode(&iface)),
            };
            let dispatch = mgr.send_with_announce_policy(message, Some(policy)).await;
            if dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0 {
                self.note_link_packet_sent(&packet).await;
            }
            return dispatch;
        } else {
            None
        };
        let dispatch = self
            .iface_manager
            .lock()
            .await
            .send_with_announce_policy(message, announce_policy)
            .await;
        if dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0 {
            self.note_link_packet_sent(&packet).await;
        }
        dispatch
    }

    pub(super) fn has_destination(&self, address: &AddressHash) -> bool {
        self.single_in_destinations.contains_key(address)
    }

    pub(super) fn knows_destination(&self, address: &AddressHash) -> bool {
        self.single_out_destinations.contains_key(address)
    }

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
            eprintln!(
                "[resource-diag] duplicate_drop_candidate node={} link={} ctx={:02x}",
                self.config.name, packet.destination, packet.context as u8
            );
        }

        is_new || allow_duplicate
    }

    #[allow(dead_code)]
    pub(super) async fn request_path(
        &mut self,
        address: &AddressHash,
        on_iface: Option<AddressHash>,
        tag: Option<TagBytes>,
    ) {
        let packet = self.path_requests.generate(address, tag);

        self.send(TxMessage { tx_type: TxMessageType::Broadcast(on_iface), packet }).await;
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

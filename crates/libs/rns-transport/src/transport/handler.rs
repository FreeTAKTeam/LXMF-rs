use super::wire::should_encrypt_packet;
use super::*;
use std::sync::OnceLock;

fn transport_diag_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("RETICULUMD_DIAGNOSTICS")
            .or_else(|_| std::env::var("RETICULUM_TRANSPORT_DIAGNOSTICS"))
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on" | "debug"
                )
            })
            .unwrap_or(false)
    })
}

impl TransportHandler {
    pub(super) async fn send_packet(&mut self, packet: Packet) {
        let _ = self.send_packet_with_trace(packet).await;
    }

    pub(super) async fn send_packet_with_outcome(&mut self, packet: Packet) -> SendPacketOutcome {
        self.send_packet_with_trace(packet).await.outcome
    }

    pub(super) async fn send_packet_with_trace(&mut self, mut packet: Packet) -> SendPacketTrace {
        if packet.header.packet_type == PacketType::Proof {
            eprintln!(
                "[tp] send_proof dst={} ctx={:02x}",
                packet.destination, packet.context as u8
            );
            if packet.context == PacketContext::LinkRequestProof {
                if let Ok(raw) = packet.to_bytes() {
                    eprintln!("[tp] lrproof_raw len={} hex={}", raw.len(), bytes_to_hex(&raw));
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

        if transport_diag_enabled() {
            if let Some(entry) = self.path_table.get(&packet.destination) {
                eprintln!(
                    "[tp-diag] route_lookup dst={} hops={} via_next_hop={} via_iface={}",
                    packet.destination, entry.hops, entry.received_from, entry.iface
                );
                log::info!(
                    "[tp-diag] route_lookup dst={} hops={} via_next_hop={} via_iface={}",
                    packet.destination,
                    entry.hops,
                    entry.received_from,
                    entry.iface
                );
            } else {
                eprintln!("[tp-diag] route_lookup dst={} missing", packet.destination);
                log::info!("[tp-diag] route_lookup dst={} missing", packet.destination);
            }
        }

        let (packet, maybe_iface) = self.path_table.handle_packet(&packet);
        if let Some(iface) = maybe_iface {
            let dispatch =
                self.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;
            let outcome = if dispatch.sent_ifaces > 0 {
                SendPacketOutcome::SentDirect
            } else {
                SendPacketOutcome::DroppedNoRoute
            };
            if transport_diag_enabled() {
                eprintln!(
                    "[tp-diag] direct_send iface={} outcome={:?} matched={} sent={} failed={}",
                    iface,
                    outcome,
                    dispatch.matched_ifaces,
                    dispatch.sent_ifaces,
                    dispatch.failed_ifaces
                );
                log::info!(
                    "[tp-diag] direct_send iface={} outcome={:?} matched={} sent={} failed={}",
                    iface,
                    outcome,
                    dispatch.matched_ifaces,
                    dispatch.sent_ifaces,
                    dispatch.failed_ifaces
                );
            }
            SendPacketTrace { outcome, direct_iface: Some(iface), broadcast: false, dispatch }
        } else if self.config.broadcast || packet.header.packet_type == PacketType::Announce {
            let dispatch =
                self.send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet }).await;
            let outcome = if dispatch.sent_ifaces > 0 {
                SendPacketOutcome::SentBroadcast
            } else {
                SendPacketOutcome::DroppedNoRoute
            };
            if transport_diag_enabled() {
                eprintln!(
                    "[tp-diag] broadcast_send outcome={:?} matched={} sent={} failed={}",
                    outcome, dispatch.matched_ifaces, dispatch.sent_ifaces, dispatch.failed_ifaces
                );
                log::info!(
                    "[tp-diag] broadcast_send outcome={:?} matched={} sent={} failed={}",
                    outcome,
                    dispatch.matched_ifaces,
                    dispatch.sent_ifaces,
                    dispatch.failed_ifaces
                );
            }
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
        self.packet_cache.lock().await.update(&message.packet);
        self.iface_manager.lock().await.send(message).await
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
                allow_duplicate = packet.context == PacketContext::KeepAlive;
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

        is_new || allow_duplicate
    }

    pub(super) async fn request_path(
        &mut self,
        address: &AddressHash,
        on_iface: Option<AddressHash>,
        tag: Option<TagBytes>,
    ) {
        let packet = self.path_requests.generate(address, tag);

        self.send(TxMessage { tx_type: TxMessageType::Broadcast(on_iface), packet }).await;
    }
}

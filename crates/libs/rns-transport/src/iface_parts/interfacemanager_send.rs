impl InterfaceManager {
    pub async fn send(&mut self, message: TxMessage) -> TxDispatchTrace {
        self.send_with_announce_policy(message, None).await
    }

    pub async fn send_with_announce_policy(
        &mut self,
        message: TxMessage,
        announce_policy: Option<AnnounceBroadcastPolicy>,
    ) -> TxDispatchTrace {
        self.send_with_options(message, announce_policy, false, false, None).await
    }

    pub async fn send_path_request(&mut self, message: TxMessage) -> TxDispatchTrace {
        self.send_with_options(message, None, false, true, None).await
    }

    pub async fn send_recursive_path_request(&mut self, message: TxMessage) -> TxDispatchTrace {
        self.send_recursive_path_request_with_modes(message, None).await
    }

    pub async fn send_recursive_path_request_with_modes(
        &mut self,
        message: TxMessage,
        allowed_modes: Option<&[InterfaceMode]>,
    ) -> TxDispatchTrace {
        self.send_with_options(message, None, true, true, allowed_modes).await
    }

    async fn send_with_options(
        &mut self,
        message: TxMessage,
        announce_policy: Option<AnnounceBroadcastPolicy>,
        apply_egress_control: bool,
        is_path_request: bool,
        allowed_modes: Option<&[InterfaceMode]>,
    ) -> TxDispatchTrace {
        self.cleanup();
        let mut trace = TxDispatchTrace::default();
        let mut saw_closed_queue = false;
        let packet_wire_len = packet_wire_len_for_dispatch(&message);
        let scoped_path_request_iface = match message.tx_type {
            TxMessageType::Broadcast(Some(address)) if apply_egress_control => Some(address),
            _ => None,
        };
        let now = Instant::now();
        let scoped_path_request_blocked = scoped_path_request_iface
            .and_then(|address| self.ifaces.iter().find(|iface| iface.address == address))
            .filter(|iface| !iface.is_shared_instance)
            .map(|iface| !iface.announce_queue.is_empty() || now < iface.announce_allowed_at)
            .unwrap_or(false);
        if !scoped_path_request_blocked {
            if let Some(address) = scoped_path_request_iface {
                if let Some(iface) = self.ifaces.iter_mut().find(|iface| iface.address == address) {
                    iface.announce_allowed_at = now
                        + announce_wait(
                            &message.packet,
                            iface.announce_bitrate_bps,
                            iface.announce_cap_percent,
                        );
                }
            }
        }

        for iface in &mut self.ifaces {
            let should_send = match message.tx_type {
                TxMessageType::Broadcast(address) => {
                    // VirtualUnicast ifaces share their tx channel with a
                    // host (multicast) iface, so broadcasting to both
                    // would double-enqueue each packet. Skip them; the
                    // host iface will carry the broadcast.
                    if iface.role == IfaceRole::VirtualUnicast {
                        false
                    } else {
                        let mut should_send = true;
                        if let Some(address) = address {
                            should_send = address != iface.address;
                        }
                        if should_send {
                            should_send = allowed_modes
                                .is_none_or(|modes| modes.contains(&iface.mode));
                        }
                        if should_send {
                            should_send = allows_announce_broadcast(
                                &message.packet,
                                iface.mode,
                                iface.shared_config.announces_from_internal,
                                announce_policy,
                            );
                        }
                        should_send
                    }
                }
                TxMessageType::Direct(address) => address == iface.address,
            };

            if should_send && iface.outgoing && !iface.stop.is_cancelled() {
                trace.matched_ifaces += 1;
                let now = Instant::now();
                if is_path_request && scoped_path_request_blocked {
                    trace.failed_ifaces += 1;
                    continue;
                }
                if is_path_request && Self::should_egress_limit_pr(iface, now) {
                    trace.failed_ifaces += 1;
                    continue;
                }
                let Some(wire_len) = packet_wire_len else {
                    trace.failed_ifaces += 1;
                    continue;
                };
                if !packet_fits_iface_mtu(iface, &message, wire_len) {
                    trace.failed_ifaces += 1;
                    continue;
                }

                let is_paced_announce = message.packet.header.packet_type == PacketType::Announce
                    && message.packet.header.hops > 0
                    && matches!(message.tx_type, TxMessageType::Broadcast(_));
                if is_paced_announce
                    && !iface.is_shared_instance
                    && (!iface.announce_queue.is_empty() || now < iface.announce_allowed_at)
                {
                    if Self::queue_announce(iface, message.clone(), now) {
                        trace.queued_ifaces += 1;
                    } else {
                        trace.failed_ifaces += 1;
                    }
                    continue;
                }

                if is_paced_announce && !iface.is_shared_instance {
                    iface.announce_allowed_at = now
                        + announce_wait(
                            &message.packet,
                            iface.announce_bitrate_bps,
                            iface.announce_cap_percent,
                        );
                }

                match Self::send_to_iface(iface, message.clone()).await {
                    TxIfaceSendResult::Sent => {
                        trace.sent_ifaces += 1;
                        Self::record_outbound_traffic(
                            iface,
                            message.packet.header.packet_type,
                            is_path_request,
                            wire_len,
                            now,
                        );
                        if is_path_request {
                            Self::record_outgoing_pr(iface, now);
                        }
                    }
                    TxIfaceSendResult::Failed => {
                        trace.failed_ifaces += 1;
                    }
                    TxIfaceSendResult::Closed => {
                        trace.failed_ifaces += 1;
                        saw_closed_queue = true;
                    }
                }
            }
        }

        if saw_closed_queue {
            self.cleanup_closed_tx_queues();
        }
        self.cleanup();
        trace
    }

    #[cfg(test)]
    pub(crate) async fn send_broadcast_on_iface(
        &mut self,
        address: AddressHash,
        packet: Packet,
    ) -> TxDispatchTrace {
        self.send_broadcast_on_iface_with_class(address, packet, false).await
    }

    pub(crate) async fn send_path_request_on_iface(
        &mut self,
        address: AddressHash,
        packet: Packet,
    ) -> TxDispatchTrace {
        self.send_broadcast_on_iface_with_class(address, packet, true).await
    }

    async fn send_broadcast_on_iface_with_class(
        &mut self,
        address: AddressHash,
        packet: Packet,
        is_path_request: bool,
    ) -> TxDispatchTrace {
        self.cleanup();
        let mut trace = TxDispatchTrace::default();
        let mut saw_closed_queue = false;
        let message = TxMessage { tx_type: TxMessageType::Broadcast(None), packet };
        let packet_wire_len = packet_wire_len_for_dispatch(&message);

        for iface in &mut self.ifaces {
            if iface.address != address || !iface.outgoing || iface.stop.is_cancelled() {
                continue;
            }

            trace.matched_ifaces += 1;
            let Some(wire_len) = packet_wire_len else {
                trace.failed_ifaces += 1;
                continue;
            };
            if !packet_fits_iface_mtu(iface, &message, wire_len) {
                trace.failed_ifaces += 1;
                continue;
            }
            match Self::send_to_iface(iface, message.clone()).await {
                TxIfaceSendResult::Sent => {
                    trace.sent_ifaces += 1;
                    Self::record_outbound_traffic(
                        iface,
                        message.packet.header.packet_type,
                        is_path_request,
                        wire_len,
                        Instant::now(),
                    );
                    if is_path_request {
                        Self::record_outgoing_pr(iface, Instant::now());
                    }
                }
                TxIfaceSendResult::Failed => trace.failed_ifaces += 1,
                TxIfaceSendResult::Closed => {
                    trace.failed_ifaces += 1;
                    saw_closed_queue = true;
                }
            }
        }

        if saw_closed_queue {
            self.cleanup_closed_tx_queues();
        }
        self.cleanup();
        trace
    }

    fn cleanup_closed_tx_queues(&mut self) {
        let before = self.ifaces.len();
        self.ifaces.retain(|iface| !iface.tx_send.is_closed());
        let removed = before.saturating_sub(self.ifaces.len());
        if removed > 0 {
            log::warn!("removed {removed} interface records with closed tx queues");
        }
    }

    async fn send_to_iface(iface: &LocalInterface, message: TxMessage) -> TxIfaceSendResult {
        let tx_type = message.tx_type;
        match iface.tx_send.try_send(message) {
            Ok(()) => TxIfaceSendResult::Sent,
            Err(mpsc::error::TrySendError::Full(message)) => {
                if matches!(tx_type, TxMessageType::Broadcast(_)) {
                    log::warn!(
                        "tx queue full dropping broadcast on {} for {:?}",
                        iface.address,
                        tx_type
                    );
                    return TxIfaceSendResult::Failed;
                }
                match tokio::time::timeout(
                    Duration::from_millis(IFACE_TX_ENQUEUE_TIMEOUT_MS),
                    iface.tx_send.send(message),
                )
                .await
                {
                    Ok(Ok(())) => {
                        log::warn!(
                            "recovered from full tx queue on {} for {:?}",
                            iface.address,
                            tx_type
                        );
                        TxIfaceSendResult::Sent
                    }
                    Ok(Err(_)) => {
                        log::warn!("tx queue closed on {} for {:?}", iface.address, tx_type);
                        TxIfaceSendResult::Closed
                    }
                    Err(_) => {
                        log::warn!("tx queue full timeout on {} for {:?}", iface.address, tx_type);
                        TxIfaceSendResult::Failed
                    }
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                log::warn!("tx queue closed on {} for {:?}", iface.address, tx_type);
                TxIfaceSendResult::Closed
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum TxIfaceSendResult {
    Sent,
    Failed,
    Closed,
}

fn packet_wire_len_for_dispatch(message: &TxMessage) -> Option<usize> {
    match message.packet.serialized_len() {
        Ok(wire_len) => Some(wire_len),
        Err(err) => {
            log::warn!(
                "tx packet serialize failed before interface enqueue tx_type={:?} \
                 packet_type={:?} context={:?} dst={} data_len={} err={:?}",
                message.tx_type,
                message.packet.header.packet_type,
                message.packet.context,
                message.packet.destination,
                message.packet.data.len(),
                err
            );
            None
        }
    }
}

fn packet_fits_iface_mtu(iface: &LocalInterface, message: &TxMessage, wire_len: usize) -> bool {
    if wire_len <= iface.mtu {
        return true;
    }

    log::warn!(
        "tx packet exceeds interface mtu iface={} tx_type={:?} packet_type={:?} \
         context={:?} dst={} data_len={} wire_len={} mtu={}",
        iface.address,
        message.tx_type,
        message.packet.header.packet_type,
        message.packet.context,
        message.packet.destination,
        message.packet.data.len(),
        wire_len,
        iface.mtu
    );
    false
}

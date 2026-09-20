impl InterfaceManager {
    pub async fn release_queued_announces(&mut self) -> TxDispatchTrace {
        self.cleanup();
        let mut trace = TxDispatchTrace::default();
        let now = Instant::now();
        let mut saw_closed_queue = false;

        for iface in &mut self.ifaces {
            if iface.stop.is_cancelled()
                || !iface.outgoing
                || iface.is_shared_instance
                || iface.announce_queue.is_empty()
                || now < iface.announce_allowed_at
            {
                continue;
            }

            let Some(message) = Self::pop_next_announce(iface, now) else {
                continue;
            };
            let wire_len = packet_wire_len_for_dispatch(&message);

            trace.matched_ifaces += 1;
            iface.announce_allowed_at = now
                + announce_wait(
                    &message.packet,
                    iface.announce_bitrate_bps,
                    iface.announce_cap_percent,
                );
            match Self::send_to_iface(iface, message).await {
                TxIfaceSendResult::Sent => {
                    trace.sent_ifaces += 1;
                    if let Some(wire_len) = wire_len {
                        Self::record_outbound_traffic(
                            iface,
                            PacketType::Announce,
                            false,
                            wire_len,
                            now,
                        );
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
}

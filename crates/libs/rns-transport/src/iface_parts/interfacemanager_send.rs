impl InterfaceManager {
    pub(crate) async fn send_broadcast_on_iface(
        &mut self,
        address: AddressHash,
        packet: Packet,
    ) -> TxDispatchTrace {
        self.cleanup();
        let mut trace = TxDispatchTrace::default();
        let mut saw_closed_queue = false;
        let message = TxMessage { tx_type: TxMessageType::Broadcast(None), packet };

        for iface in &mut self.ifaces {
            if iface.address != address || !iface.outgoing || iface.stop.is_cancelled() {
                continue;
            }

            trace.matched_ifaces += 1;
            match Self::send_to_iface(iface, message.clone()).await {
                TxIfaceSendResult::Sent => trace.sent_ifaces += 1,
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

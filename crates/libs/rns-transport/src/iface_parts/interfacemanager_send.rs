impl InterfaceManager {
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
                    if tx_diag_enabled() {
                        log::warn!(
                            "tx queue full dropping broadcast on {} for {:?}",
                            iface.address,
                            tx_type
                        );
                    }
                    return TxIfaceSendResult::Failed;
                }
                match tokio::time::timeout(
                    Duration::from_millis(IFACE_TX_ENQUEUE_TIMEOUT_MS),
                    iface.tx_send.send(message),
                )
                .await
                {
                    Ok(Ok(())) => {
                        if tx_diag_enabled() {
                            log::warn!(
                                "recovered from full tx queue on {} for {:?}",
                                iface.address,
                                tx_type
                            );
                        }
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

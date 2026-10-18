use super::*;

impl DeliveryTask {
    /// Observe the receipt owner without replacing an unavailable read with failure.
    pub(super) fn observe_delivery_terminal(&self) -> std::io::Result<bool> {
        if self.receipt_tx.terminal_pending(&self.message_id)? {
            return Ok(true);
        }
        let status = self.daemon.message_receipt_status(&self.message_id)?;
        self.daemon.owns_durable_message(&self.message_id)?;
        let terminal = status.as_deref().is_some_and(Self::is_terminal_status);
        if terminal {
            self.receipt_tx.release_unsent(&self.message_id);
        }
        Ok(terminal)
    }

    /// Keep the semaphore waiter registered while polling persisted cancellation.
    pub(super) async fn acquire_stamp_permit(
        &self,
        limit: &Arc<tokio::sync::Semaphore>,
    ) -> Result<Option<tokio::sync::OwnedSemaphorePermit>, tokio::sync::AcquireError> {
        let acquire = limit.clone().acquire_owned();
        tokio::pin!(acquire);
        let mut stopping = self.stopping.clone();
        loop {
            if self.abort_if_cancelled("stamp-queue") {
                return Ok(None);
            }
            tokio::select! {
                result = &mut acquire => return result.map(Some),
                _ = tokio::time::sleep(Duration::from_millis(250)) => {},
                _ = async {
                    match stopping.as_mut() {
                        Some(stop) => { let _ = stop.changed().await; },
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    if stopping.as_ref().is_some_and(|stop| *stop.borrow() || stop.has_changed().is_err()) {
                        return Ok(None);
                    }
                },
            }
        }
    }

    pub(super) fn abort_if_cancelled(&self, stage: &str) -> bool {
        if self.stopping.as_ref().is_some_and(|stop| *stop.borrow()) {
            return true;
        }
        self.abort_for_status_result(stage, self.daemon.message_receipt_status(&self.message_id))
    }

    pub(super) fn abort_for_status_result(
        &self,
        stage: &str,
        status: Result<Option<String>, std::io::Error>,
    ) -> bool {
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                log::error!(
                    "[daemon-delivery] receipt status lookup failed message_id={} stage={stage}: {error}",
                    self.message_id
                );
                let persisted = crate::receipt_events::persist_receipt_update(
                    self.daemon.as_ref(),
                    ReceiptEvent::new(
                        self.message_id.clone(),
                        format!("failed: receipt status lookup: {error}"),
                    )
                    .with_stage(stage),
                    &self.receipt_map,
                    &self.outbound_resource_map,
                );
                return match persisted {
                    Ok(_) => {
                        self.receipt_tx.complete_terminal(&self.message_id);
                        true
                    }
                    Err(persist_error) => {
                        log::error!(
                            "[daemon-delivery] failed to persist receipt lookup failure message_id={} stage={stage}: {persist_error}; continuing because no terminal state was established",
                            self.message_id
                        );
                        false
                    }
                };
            }
        };
        match self.daemon.owns_durable_message(&self.message_id) {
            Ok(_) => {}
            Err(error) => {
                log::error!("durable dispatch status unavailable: {error}; send suppressed");
                return true;
            }
        };
        let terminal = status.as_deref().is_some_and(Self::is_terminal_status);
        if !terminal && !Self::is_cancelled_status(status.as_deref()) {
            return false;
        }
        self.receipt_tx.complete_terminal(&self.message_id);
        log_delivery_trace(&self.message_id, &self.destination_hex, stage, "cancelled");
        true
    }

    pub(in crate::bridge) fn is_terminal_status(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        matches!(normalized.as_str(), "delivered" | "cancelled" | "expired" | "rejected")
            || normalized.starts_with("failed")
    }
    pub(super) fn is_cancelled_status(status: Option<&str>) -> bool {
        status.is_some_and(|value| value.trim().eq_ignore_ascii_case("cancelled"))
    }
}

#[cfg(test)]
mod durable_status_tests {
    use super::*;
    #[test]
    fn terminal_receipts_suppress_sends_with_storage_normalization() {
        for status in [" Delivered ", " FAILED: unreachable ", "cancelled", " EXPIRED ", "Rejected"]
        {
            assert!(DeliveryTask::is_terminal_status(status));
        }
        for status in ["sent: link", "sending", "queued"] {
            assert!(!DeliveryTask::is_terminal_status(status));
        }
    }
}

use super::*;

impl DeliveryTask {
    /// Drive accepted native I/O across unavailable status reads, while allowing
    /// terminal commitment or coordinated shutdown to interrupt a blocked wait.
    pub(super) async fn drive_delivery_io<T>(
        &self,
        future: impl std::future::Future<Output = T>,
    ) -> Option<T> {
        tokio::pin!(future);
        loop {
            tokio::select! {
                result = &mut future => return Some(result),
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    if matches!(self.check_delivery_ready(), Err(super::delivery_attempt::DeliveryAttemptOutcome::Stopped)) {
                        return None;
                    }
                },
            }
        }
    }

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

    pub(super) fn check_delivery_ready(
        &self,
    ) -> Result<(), super::delivery_attempt::DeliveryAttemptOutcome> {
        use super::delivery_attempt::DeliveryAttemptOutcome;
        if self.stopping.as_ref().is_some_and(|stop| *stop.borrow() || stop.has_changed().is_err())
        {
            return Err(DeliveryAttemptOutcome::Stopped);
        }
        match self.observe_delivery_terminal() {
            Ok(false) => Ok(()),
            Ok(true) => Err(DeliveryAttemptOutcome::Stopped),
            Err(error) => {
                log::warn!("delivery status unavailable message_id={}: {error}", self.message_id);
                Err(DeliveryAttemptOutcome::AwaitStatus)
            }
        }
    }

    pub(super) async fn wait_delivery_ready(&self) -> bool {
        let mut stopping = self.stopping.clone();
        loop {
            match self.check_delivery_ready() {
                Ok(()) => return true,
                Err(super::delivery_attempt::DeliveryAttemptOutcome::AwaitStatus) => {}
                Err(_) => return false,
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(250)) => {},
                _ = async { match stopping.as_mut() {
                    Some(stop) => { if let Err(error) = stop.changed().await { log::debug!("delivery stop producer closed: {error}"); } },
                    None => std::future::pending().await,
                }} => {},
            }
        }
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
            if !self.wait_delivery_ready().await {
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

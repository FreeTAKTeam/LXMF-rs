impl<B> RnodeBleKissRuntime<B>
where
    B: RnodeBleBackend,
{
    async fn drain_startup_notifications(&mut self) -> Result<(), RnodeBleKissError> {
        let deadline = TokioInstant::now() + RNODE_BLE_STARTUP_STABILIZATION_TIMEOUT;
        let mut drained = 0_usize;
        loop {
            let now = TokioInstant::now();
            if now >= deadline {
                break;
            }
            let quiet_timeout = deadline
                .saturating_duration_since(now)
                .min(RNODE_BLE_STARTUP_NOTIFICATION_QUIET_TIMEOUT);
            match timeout(quiet_timeout, self.backend.next_notification()).await {
                Ok(Ok(Some(_))) => {
                    drained += 1;
                }
                Ok(Ok(None)) | Err(_) => break,
                Ok(Err(message)) => {
                    self.connected = false;
                    return Err(RnodeBleKissError::Backend {
                        operation: "drain_startup_notifications",
                        message,
                    });
                }
            }
        }
        if drained > 0 {
            log::debug!("drained {drained} stale RNode BLE startup notifications");
        }
        Ok(())
    }
}

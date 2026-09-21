// Connection ownership is separate from readiness: a failed setup or EOF can
// leave a GATT connection that must be released before another attempt.
impl<B: RnodeBleBackend> RnodeBleKissRuntime<B> {
    fn reset_session(&mut self) {
        let mut config = self.session.config.clone();
        config.max_write_len = self.configured_max_write_len;
        self.session = RnodeBleKissSession::new(config);
        self.connected = false;
    }

    pub async fn startup(&mut self) -> Result<(), RnodeBleKissError> {
        if self.backend_open {
            self.close().await?;
        }
        self.reset_session();
        self.backend_open = true;
        let result = self.startup_session().await;
        if result.is_err() {
            if let Err(cleanup_error) = self.close().await {
                // Keep the setup error as the return value, but retain the cleanup
                // failure in diagnostics rather than silently substituting it.
                log::warn!("RNode BLE cleanup after startup failure: {cleanup_error:?}");
            }
        }
        result
    }

    async fn startup_session(&mut self) -> Result<(), RnodeBleKissError> {
        self.backend.connect().await.map_err(|message| RnodeBleKissError::Backend {
            operation: "connect", message,
        })?;
        if let Some(mtu) = self.backend.negotiated_mtu() {
            let att_payload = usize::from(mtu).saturating_sub(3);
            self.session.config.max_write_len = self.configured_max_write_len
                .min(att_payload).min(self.session.config.mtu);
        }
        self.backend.subscribe_notifications().await.map_err(|message| {
            RnodeBleKissError::Backend { operation: "subscribe_notifications", message }
        })?;
        if self.backend.drains_stale_startup_notifications() {
            self.drain_startup_notifications().await?;
        }
        let writes = self.session.startup_frames();
        self.write_all(writes, "startup_write").await?;
        self.connected = true;
        Ok(())
    }
}

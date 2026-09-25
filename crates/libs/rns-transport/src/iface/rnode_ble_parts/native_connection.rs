#[cfg(feature = "rnode-ble")]
impl NativeRnodeBleBackend {
    async fn bounded_disconnect<F, E>(
        disconnect: F,
        connect_timeout: Duration,
    ) -> Result<(), String>
    where
        F: std::future::Future<Output = Result<(), E>>,
        E: std::fmt::Display,
    {
        timeout(connect_timeout, disconnect)
            .await
            .map_err(|_| {
                format!("disconnect timeout after {} ms", connect_timeout.as_millis())
            })?
            .map_err(|error| format!("disconnect peripheral: {error}"))
    }

    async fn disconnect_with_timeout(
        peripheral: &Peripheral,
        connect_timeout: Duration,
    ) -> Result<(), String> {
        Self::bounded_disconnect(peripheral.disconnect(), connect_timeout).await
    }

    /// Bound platform cleanup so an unresponsive desktop GATT operation cannot
    /// indefinitely prevent a reconnect. Timed-out sessions retain their handles
    /// for a later cleanup attempt.
    pub async fn cleanup(&mut self) -> Result<(), String> {
        let deadline = self.settings.connect_timeout;
        timeout(deadline, self.cleanup_session()).await.map_err(|_| {
            format!("RNode BLE cleanup timeout after {} ms", deadline.as_millis())
        })?
    }

    async fn cleanup_session(&mut self) -> Result<(), String> {
        let mut failures = Vec::new();
        if let (Some(peripheral), Some(notify_char)) =
            (self.peripheral.as_ref(), self.notify_char.as_ref())
        {
            if let Err(err) = peripheral.unsubscribe(notify_char).await {
                failures.push(format!("unsubscribe RNode BLE notify characteristic: {err}"));
            }
        }
        if let Some(adapter) = self.adapter.as_ref() {
            if let Err(err) = adapter.stop_scan().await {
                failures.push(format!("stop BLE scan: {err}"));
            }
        }
        if let Some(peripheral) = self.peripheral.as_ref() {
            match peripheral.is_connected().await {
                Ok(true) => {
                    if let Err(err) = peripheral.disconnect().await {
                        failures.push(format!("disconnect peripheral: {err}"));
                    }
                }
                Ok(false) => {}
                Err(err) => failures.push(format!("read connection state: {err}")),
            }
        }
        self.clear_session_state();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    async fn connect_session(&mut self) -> Result<(), String> {
        let adapter = Self::select_adapter(&self.settings).await?;
        let paired_addresses = native_rnode_windows_paired_addresses().await?;
        // Own handles before fallible awaits, so even a partial connection or
        // service-discovery failure can be cleaned up by the caller.
        self.adapter = Some(adapter.clone());
        let peripheral = match Self::configured_peripheral(&adapter, &self.settings).await? {
            Some(peripheral) => {
                self.peripheral = Some(peripheral.clone());
                match Self::connect_selected_peripheral(&peripheral, self.settings.connect_timeout)
                    .await
                {
                    Ok(()) => peripheral,
                    Err(configured_err) => {
                        log::warn!(
                            "RNode BLE configured Android peripheral connect failed peripheral_id={} err={}; falling back to BLE scan",
                            self.settings.peripheral_id,
                            configured_err
                        );
                        if let Err(err) =
                            Self::disconnect_with_timeout(&peripheral, self.settings.connect_timeout)
                                .await
                        {
                            log::debug!(
                                "RNode BLE configured Android peripheral cleanup failed peripheral_id={} err={}",
                                self.settings.peripheral_id,
                                err
                            );
                        }
                        let excluded_identifiers =
                            vec![self.settings.peripheral_id.clone(), peripheral.id().to_string()];
                        let scanned = Self::scan_for_peripheral(
                            &adapter,
                            &self.settings,
                            Some(&self.settings.peripheral_id),
                            false,
                            &excluded_identifiers,
                            paired_addresses.as_deref(),
                        )
                        .await
                        .map_err(|scan_err| {
                            format!("{configured_err}; fallback scan failed: {scan_err}")
                        })?;
                        self.peripheral = Some(scanned.clone());
                        Self::connect_selected_peripheral(&scanned, self.settings.connect_timeout)
                            .await
                            .map_err(|scan_err| {
                                format!(
                                    "{configured_err}; fallback scanned peripheral connect failed: {scan_err}"
                                )
                            })?;
                        scanned
                    }
                }
            }
            None => {
                let scanned = Self::scan_for_peripheral(
                    &adapter,
                    &self.settings,
                    None,
                    false,
                    &[],
                    paired_addresses.as_deref(),
                )
                .await?;
                self.peripheral = Some(scanned.clone());
                Self::connect_selected_peripheral(&scanned, self.settings.connect_timeout).await?;
                scanned
            }
        };

        self.adapter = Some(adapter);
        self.peripheral = Some(peripheral);
        let mtu = self.peripheral.as_ref().expect("just set above").mtu();
        // On macOS, CoreBluetooth never updates its cached AtomicU16, so peripheral.mtu()
        // always returns DEFAULT_MTU_SIZE (23) regardless of the actual negotiated value.
        // On all other platforms btleplug reports the real negotiated MTU, including 23
        // when that is genuinely what was negotiated.
        self.negotiated_mtu =
            if cfg!(target_os = "macos") && mtu == DEFAULT_MTU_SIZE { None } else { Some(mtu) };
        self.resolve_characteristics()
    }

}

#[cfg(all(test, feature = "rnode-ble"))]
mod disconnect_timeout_tests {
    use super::*;

    #[tokio::test]
    async fn a_stalled_configured_device_disconnect_is_bounded_before_scan_fallback() {
        let started = TokioInstant::now();
        let result = NativeRnodeBleBackend::bounded_disconnect(
            std::future::pending::<Result<(), &'static str>>(),
            Duration::from_millis(10),
        )
        .await;

        assert_eq!(result, Err("disconnect timeout after 10 ms".to_string()));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}

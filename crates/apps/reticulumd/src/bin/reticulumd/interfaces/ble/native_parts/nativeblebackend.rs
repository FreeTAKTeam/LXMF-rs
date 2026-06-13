impl NativeBleBackend {
    fn new(backend_name: &'static str) -> Self {
        Self {
            backend_name,
            adapter: None,
            peripheral: None,
            write_char: None,
            notify_char: None,
            notification_stream: None,
            write_type: None,
        }
    }

    fn clear_session_state(&mut self) {
        self.adapter = None;
        self.peripheral = None;
        self.write_char = None;
        self.notify_char = None;
        self.notification_stream = None;
        self.write_type = None;
    }

    async fn select_adapter(settings: &BleRuntimeSettings) -> Result<Adapter, BleBackendError> {
        let manager = Manager::new()
            .await
            .map_err(|err| BleBackendError::retryable(format!("create BLE manager: {err}")))?;
        let adapters = manager
            .adapters()
            .await
            .map_err(|err| BleBackendError::retryable(format!("enumerate BLE adapters: {err}")))?;

        if adapters.is_empty() {
            return Err(BleBackendError::retryable("no BLE adapters available on host"));
        }

        if let Some(requested) = settings.adapter.as_deref() {
            let requested = requested.trim();
            let mut adapter_lookup_errors = Vec::new();
            for adapter in adapters {
                match adapter_matches(&adapter, requested).await {
                    Ok(true) => return Ok(adapter),
                    Ok(false) => {}
                    Err(err) => adapter_lookup_errors.push(err.message),
                }
            }
            if !adapter_lookup_errors.is_empty() {
                return Err(BleBackendError::retryable(format!(
                    "failed to inspect adapters while looking up '{}': {}",
                    requested,
                    adapter_lookup_errors.join("; ")
                )));
            }
            return Err(BleBackendError::terminal(format!(
                "configured adapter '{}' not found",
                requested
            )));
        }

        Ok(adapters.into_iter().next().expect("adapters already checked as non-empty"))
    }

    fn resolve_characteristics(
        &self,
        settings: &BleRuntimeSettings,
    ) -> Result<(Characteristic, Characteristic, WriteType), BleBackendError> {
        let peripheral = self.peripheral.as_ref().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not select a peripheral")
        })?;
        let service_uuid = parse_gatt_uuid("service_uuid", &settings.service_uuid)?;
        let write_uuid = parse_gatt_uuid("write_char_uuid", &settings.write_char_uuid)?;
        let notify_uuid = parse_gatt_uuid("notify_char_uuid", &settings.notify_char_uuid)?;

        let characteristics = peripheral.characteristics();
        let write_char = characteristics
            .iter()
            .find(|characteristic| characteristic.uuid == write_uuid)
            .cloned()
            .ok_or_else(|| {
                BleBackendError::terminal(format!(
                    "write characteristic {} not found on peripheral",
                    settings.write_char_uuid
                ))
            })?;

        let notify_char = characteristics
            .iter()
            .find(|characteristic| characteristic.uuid == notify_uuid)
            .cloned()
            .ok_or_else(|| {
                BleBackendError::terminal(format!(
                    "notify characteristic {} not found on peripheral",
                    settings.notify_char_uuid
                ))
            })?;

        if write_char.service_uuid != service_uuid {
            return Err(BleBackendError::terminal(format!(
                "write characteristic {} does not belong to service {}",
                settings.write_char_uuid, settings.service_uuid
            )));
        }
        if notify_char.service_uuid != service_uuid {
            return Err(BleBackendError::terminal(format!(
                "notify characteristic {} does not belong to service {}",
                settings.notify_char_uuid, settings.service_uuid
            )));
        }

        let write_type = if write_char.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else if write_char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
            WriteType::WithoutResponse
        } else {
            return Err(BleBackendError::terminal(format!(
                "write characteristic {} does not support write operations",
                settings.write_char_uuid
            )));
        };

        if !notify_char.properties.contains(CharPropFlags::NOTIFY)
            && !notify_char.properties.contains(CharPropFlags::INDICATE)
        {
            return Err(BleBackendError::terminal(format!(
                "notify characteristic {} does not support notifications or indications",
                settings.notify_char_uuid
            )));
        }

        Ok((write_char, notify_char, write_type))
    }

    async fn find_peripheral(
        adapter: &Adapter,
        settings: &BleRuntimeSettings,
    ) -> Result<Peripheral, BleBackendError> {
        let deadline = Instant::now() + settings.scan_timeout;
        loop {
            let peripherals = adapter
                .peripherals()
                .await
                .map_err(|err| BleBackendError::retryable(format!("list peripherals: {err}")))?;

            for peripheral in peripherals {
                if peripheral_matches(&peripheral, &settings.peripheral_id).await? {
                    return Ok(peripheral);
                }
            }

            if Instant::now() >= deadline {
                return Err(BleBackendError::retryable(format!(
                    "scan timeout waiting for peripheral_id={}",
                    settings.peripheral_id
                )));
            }

            sleep(SCAN_POLL_INTERVAL).await;
        }
    }

    async fn write_payload(&mut self, payload: &[u8]) -> Result<(), BleBackendError> {
        let peripheral = self.peripheral.as_ref().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not provide a peripheral")
        })?;
        let write_char = self.write_char.clone().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not resolve write characteristic")
        })?;
        let write_type = self
            .write_type
            .ok_or_else(|| BleBackendError::terminal("connect phase did not resolve write mode"))?;

        peripheral
            .write(&write_char, payload, write_type)
            .await
            .map_err(|err| BleBackendError::retryable(format!("write payload: {err}")))
    }

    async fn read_notification_value(
        &mut self,
        settings: &BleRuntimeSettings,
    ) -> Result<Vec<u8>, BleBackendError> {
        let notify_char = self.notify_char.as_ref().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not resolve notify characteristic")
        })?;
        let notify_uuid = notify_char.uuid;

        let stream = self.notification_stream.as_mut().ok_or_else(|| {
            BleBackendError::terminal("subscribe phase did not initialize notification stream")
        })?;

        let notification = timeout(settings.connect_timeout, stream.as_mut().next())
            .await
            .map_err(|_| {
                BleBackendError::retryable(format!(
                    "notification timeout after {} ms",
                    settings.connect_timeout.as_millis()
                ))
            })?
            .ok_or_else(|| BleBackendError::retryable("notification stream closed"))?;

        if notification.uuid != notify_uuid {
            return Err(BleBackendError::retryable("notification for unexpected characteristic"));
        }

        Ok(notification.value)
    }
}

impl BleBackend for NativeBleBackend {
    fn backend_name(&self) -> &'static str {
        self.backend_name
    }

    async fn scan(&mut self, settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
        if settings
            .adapter
            .as_deref()
            .is_some_and(|adapter| adapter.eq_ignore_ascii_case("disabled"))
        {
            return Err(BleBackendError::terminal(format!(
                "{} BLE adapter is disabled by configuration",
                self.backend_name
            )));
        }

        self.clear_session_state();
        let adapter = Self::select_adapter(settings).await?;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|err| BleBackendError::retryable(format!("start BLE scan: {err}")))?;
        self.adapter = Some(adapter.clone());

        let peripheral = Self::find_peripheral(&adapter, settings).await?;
        self.peripheral = Some(peripheral);
        Ok(())
    }

    async fn connect(&mut self, settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
        let peripheral = self
            .peripheral
            .as_ref()
            .ok_or_else(|| BleBackendError::terminal("scan phase did not identify a peripheral"))?
            .clone();

        timeout(settings.connect_timeout, async {
            let connected = peripheral.is_connected().await.map_err(|err| {
                BleBackendError::retryable(format!("read BLE connection state: {err}"))
            })?;
            if !connected {
                peripheral.connect().await.map_err(|err| {
                    BleBackendError::retryable(format!("connect peripheral: {err}"))
                })?;
            }
            peripheral
                .discover_services()
                .await
                .map_err(|err| BleBackendError::retryable(format!("discover GATT services: {err}")))
        })
        .await
        .map_err(|_| {
            BleBackendError::retryable(format!(
                "connect timeout after {} ms",
                settings.connect_timeout.as_millis()
            ))
        })??;

        let (write_char, notify_char, write_type) = self.resolve_characteristics(settings)?;
        self.write_char = Some(write_char);
        self.notify_char = Some(notify_char);
        self.write_type = Some(write_type);
        Ok(())
    }

    async fn subscribe(&mut self, _settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
        let peripheral = self.peripheral.as_ref().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not provide a peripheral")
        })?;
        let notify_char = self.notify_char.clone().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not resolve notify characteristic")
        })?;

        let stream = peripheral.notifications().await.map_err(|err| {
            BleBackendError::retryable(format!("open notification stream: {err}"))
        })?;
        self.notification_stream = Some(Box::pin(stream));

        peripheral.subscribe(&notify_char).await.map_err(|err| {
            BleBackendError::retryable(format!("subscribe to notify characteristic: {err}"))
        })
    }

    async fn write_probe(
        &mut self,
        payload: &[u8],
        _settings: &BleRuntimeSettings,
    ) -> Result<(), BleBackendError> {
        self.write_payload(payload).await.map_err(|err| {
            BleBackendError::retryable(format!("write probe payload: {}", err.message))
        })
    }

    async fn read_probe_notification(
        &mut self,
        settings: &BleRuntimeSettings,
    ) -> Result<Vec<u8>, BleBackendError> {
        self.read_notification_value(settings).await.map_err(|err| {
            BleBackendError::retryable(format!("probe notification read failed: {}", err.message))
        })
    }

    async fn cleanup(&mut self, _settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
        let mut failures = Vec::new();

        if let (Some(peripheral), Some(notify_char)) =
            (self.peripheral.as_ref(), self.notify_char.as_ref())
        {
            if let Err(err) = peripheral.unsubscribe(notify_char).await {
                failures.push(format!("unsubscribe notify characteristic: {err}"));
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
                Err(err) => {
                    failures.push(format!("read BLE connection state during cleanup: {err}"))
                }
            }
        }

        self.clear_session_state();

        if failures.is_empty() {
            Ok(())
        } else {
            Err(BleBackendError::retryable(format!(
                "native BLE cleanup encountered errors: {}",
                failures.join("; ")
            )))
        }
    }
}

async fn adapter_matches(adapter: &Adapter, requested: &str) -> Result<bool, BleBackendError> {
    let adapter_info = adapter
        .adapter_info()
        .await
        .map_err(|err| BleBackendError::retryable(format!("read adapter info: {err}")))?;

    if identifiers_match(requested, &adapter_info) {
        return Ok(true);
    }

    let peripherals = adapter
        .peripherals()
        .await
        .map_err(|err| BleBackendError::retryable(format!("list adapter peripherals: {err}")))?;
    for peripheral in peripherals {
        if identifiers_match(requested, &peripheral.id().to_string()) {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
) -> Result<bool, BleBackendError> {
    if identifiers_match(configured_id, &peripheral.id().to_string()) {
        return Ok(true);
    }

    let properties = peripheral
        .properties()
        .await
        .map_err(|err| BleBackendError::retryable(format!("read peripheral properties: {err}")))?;

    if let Some(properties) = properties {
        if identifiers_match(configured_id, &properties.address.to_string()) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if identifiers_match(configured_id, &local_name) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn identifiers_match(configured: &str, discovered: &str) -> bool {
    normalize_identifier(configured) == normalize_identifier(discovered)
}

fn normalize_identifier(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, ':' | '-'))
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn parse_gatt_uuid(field: &str, value: &str) -> Result<Uuid, BleBackendError> {
    let normalized = value.trim();
    if normalized.len() == 4 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Uuid::parse_str(&format!("0000{normalized}-0000-1000-8000-00805f9b34fb")).map_err(
            |err| BleBackendError::terminal(format!("invalid ble_gatt.{field} '{value}': {err}")),
        );
    }
    if normalized.len() == 8 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Uuid::parse_str(&format!("{normalized}-0000-1000-8000-00805f9b34fb")).map_err(
            |err| BleBackendError::terminal(format!("invalid ble_gatt.{field} '{value}': {err}")),
        );
    }

    Uuid::parse_str(normalized).map_err(|err| {
        BleBackendError::terminal(format!("invalid ble_gatt.{field} '{value}': {err}"))
    })
}

const BLE_STARTUP_MAX_RETRY_ATTEMPTS: u32 = 5;

const BLE_STARTUP_PROBE_PAYLOAD: &[u8] = b"LXMF-BLE-PROBE";

const DEFAULT_ATT_NOTIFICATION_PAYLOAD_BYTES: usize = 20;

#[derive(Debug, Clone)]
pub(crate) struct BleRuntimeSettings {
    pub(crate) adapter: Option<String>,
    pub(crate) peripheral_id: String,
    pub(crate) service_uuid: String,
    pub(crate) write_char_uuid: String,
    pub(crate) notify_char_uuid: String,
    pub(crate) mtu: usize,
    pub(crate) scan_timeout: Duration,
    pub(crate) connect_timeout: Duration,
    pub(crate) reconnect_backoff: Duration,
    pub(crate) max_reconnect_backoff: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BleLifecyclePhase {
    Scan,
    Connect,
    Subscribe,
    WriteProbe,
    NotificationProbe,
}

impl BleLifecyclePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Connect => "connect",
            Self::Subscribe => "subscribe",
            Self::WriteProbe => "write_probe",
            Self::NotificationProbe => "notification_probe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BleLifecycleOutcome {
    Ok,
    Retry,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BleLifecycleTransition {
    pub(crate) attempt: u32,
    pub(crate) phase: BleLifecyclePhase,
    pub(crate) outcome: BleLifecycleOutcome,
    pub(crate) detail: Option<String>,
    pub(crate) backoff_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BleLifecycleReport {
    pub(crate) backend: &'static str,
    pub(crate) attempts: u32,
    pub(crate) transitions: Vec<BleLifecycleTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BleBackendError {
    message: String,
    retryable: bool,
}

impl BleBackendError {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn retryable(message: impl Into<String>) -> Self {
        Self { message: message.into(), retryable: true }
    }

    pub(crate) fn terminal(message: impl Into<String>) -> Self {
        Self { message: message.into(), retryable: false }
    }
}

#[allow(async_fn_in_trait)]
pub(crate) trait BleBackend {
    fn backend_name(&self) -> &'static str;

    async fn scan(&mut self, settings: &BleRuntimeSettings) -> Result<(), BleBackendError>;

    async fn connect(&mut self, settings: &BleRuntimeSettings) -> Result<(), BleBackendError>;

    async fn subscribe(&mut self, settings: &BleRuntimeSettings) -> Result<(), BleBackendError>;

    async fn write_probe(
        &mut self,
        payload: &[u8],
        settings: &BleRuntimeSettings,
    ) -> Result<(), BleBackendError>;

    async fn read_probe_notification(
        &mut self,
        settings: &BleRuntimeSettings,
    ) -> Result<Vec<u8>, BleBackendError>;

    async fn cleanup(&mut self, _settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
        Ok(())
    }
}

pub(crate) async fn spawn(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    iface: &InterfaceConfig,
) -> Result<BleSpawnResult, String> {
    let settings = runtime_settings(iface)?;

    #[cfg(target_os = "android")]
    {
        return android::spawn(iface_manager, iface, settings).await;
    }
    #[cfg(target_os = "linux")]
    {
        return linux::spawn(iface_manager, iface, settings).await;
    }
    #[cfg(target_os = "macos")]
    {
        return macos::spawn(iface_manager, iface, settings).await;
    }
    #[cfg(target_os = "windows")]
    {
        return windows::spawn(iface_manager, iface, settings).await;
    }
    #[allow(unreachable_code)]
    Err(format!(
        "ble_gatt is not available on this target for interface {}",
        iface.name.as_deref().unwrap_or("<unnamed>")
    ))
}

pub(crate) async fn run_startup_lifecycle<B: BleBackend>(
    backend: &mut B,
    settings: &BleRuntimeSettings,
) -> Result<BleLifecycleReport, String> {
    let mut attempt = 1u32;
    let mut backoff = settings.reconnect_backoff;
    let mut transitions = Vec::new();

    loop {
        if let Some(err) = stage_result(
            backend.backend_name(),
            attempt,
            BleLifecyclePhase::Scan,
            backoff,
            attempt < BLE_STARTUP_MAX_RETRY_ATTEMPTS,
            &mut transitions,
            backend.scan(settings).await,
        ) {
            if should_retry(&err, attempt) {
                cleanup_backend(backend, settings).await;
                schedule_retry(&mut attempt, &mut backoff, settings.max_reconnect_backoff).await;
                continue;
            }
            cleanup_backend(backend, settings).await;
            return Err(format!(
                "ble_gatt startup failed backend={} phase={} attempt={} err={}",
                backend.backend_name(),
                BleLifecyclePhase::Scan.as_str(),
                attempt,
                err.message
            ));
        }

        if let Some(err) = stage_result(
            backend.backend_name(),
            attempt,
            BleLifecyclePhase::Connect,
            backoff,
            attempt < BLE_STARTUP_MAX_RETRY_ATTEMPTS,
            &mut transitions,
            backend.connect(settings).await,
        ) {
            if should_retry(&err, attempt) {
                cleanup_backend(backend, settings).await;
                schedule_retry(&mut attempt, &mut backoff, settings.max_reconnect_backoff).await;
                continue;
            }
            cleanup_backend(backend, settings).await;
            return Err(format!(
                "ble_gatt startup failed backend={} phase={} attempt={} err={}",
                backend.backend_name(),
                BleLifecyclePhase::Connect.as_str(),
                attempt,
                err.message
            ));
        }

        if let Some(err) = stage_result(
            backend.backend_name(),
            attempt,
            BleLifecyclePhase::Subscribe,
            backoff,
            attempt < BLE_STARTUP_MAX_RETRY_ATTEMPTS,
            &mut transitions,
            backend.subscribe(settings).await,
        ) {
            if should_retry(&err, attempt) {
                cleanup_backend(backend, settings).await;
                schedule_retry(&mut attempt, &mut backoff, settings.max_reconnect_backoff).await;
                continue;
            }
            cleanup_backend(backend, settings).await;
            return Err(format!(
                "ble_gatt startup failed backend={} phase={} attempt={} err={}",
                backend.backend_name(),
                BleLifecyclePhase::Subscribe.as_str(),
                attempt,
                err.message
            ));
        }

        if let Some(err) = stage_result(
            backend.backend_name(),
            attempt,
            BleLifecyclePhase::WriteProbe,
            backoff,
            attempt < BLE_STARTUP_MAX_RETRY_ATTEMPTS,
            &mut transitions,
            backend.write_probe(BLE_STARTUP_PROBE_PAYLOAD, settings).await,
        ) {
            if should_retry(&err, attempt) {
                cleanup_backend(backend, settings).await;
                schedule_retry(&mut attempt, &mut backoff, settings.max_reconnect_backoff).await;
                continue;
            }
            cleanup_backend(backend, settings).await;
            return Err(format!(
                "ble_gatt startup failed backend={} phase={} attempt={} err={}",
                backend.backend_name(),
                BleLifecyclePhase::WriteProbe.as_str(),
                attempt,
                err.message
            ));
        }

        let notification_result = backend.read_probe_notification(settings).await;
        if let Some(err) = stage_result(
            backend.backend_name(),
            attempt,
            BleLifecyclePhase::NotificationProbe,
            backoff,
            attempt < BLE_STARTUP_MAX_RETRY_ATTEMPTS,
            &mut transitions,
            notification_result.as_ref().map(|_| ()).map_err(Clone::clone),
        ) {
            if should_retry(&err, attempt) {
                cleanup_backend(backend, settings).await;
                schedule_retry(&mut attempt, &mut backoff, settings.max_reconnect_backoff).await;
                continue;
            }
            cleanup_backend(backend, settings).await;
            return Err(format!(
                "ble_gatt startup failed backend={} phase={} attempt={} err={}",
                backend.backend_name(),
                BleLifecyclePhase::NotificationProbe.as_str(),
                attempt,
                err.message
            ));
        }

        if let Ok(payload) = notification_result {
            if payload != BLE_STARTUP_PROBE_PAYLOAD {
                cleanup_backend(backend, settings).await;
                let diagnostic = startup_probe_mismatch_diagnostic(
                    BLE_STARTUP_PROBE_PAYLOAD.len(),
                    payload.len(),
                );
                return Err(format!(
                    "ble_gatt startup failed backend={} phase={} attempt={} err=probe payload mismatch expected_len={} actual_len={}{}",
                    backend.backend_name(),
                    BleLifecyclePhase::NotificationProbe.as_str(),
                    attempt,
                    BLE_STARTUP_PROBE_PAYLOAD.len(),
                    payload.len(),
                    diagnostic,
                ));
            }
        }

        cleanup_backend(backend, settings).await;
        return Ok(BleLifecycleReport {
            backend: backend.backend_name(),
            attempts: attempt,
            transitions,
        });
    }
}

fn should_retry(err: &BleBackendError, attempt: u32) -> bool {
    err.retryable && attempt < BLE_STARTUP_MAX_RETRY_ATTEMPTS
}

async fn schedule_retry(attempt: &mut u32, backoff: &mut Duration, max_backoff: Duration) {
    sleep_before_retry(*backoff).await;
    *backoff = bounded_backoff_next(*backoff, max_backoff);
    *attempt += 1;
}

async fn cleanup_backend<B: BleBackend>(backend: &mut B, settings: &BleRuntimeSettings) {
    if let Err(err) = backend.cleanup(settings).await {
        log::error!(
            "[daemon] ble_gatt backend={} cleanup err={}",
            backend.backend_name(),
            err.message
        );
    }
}

async fn sleep_before_retry(backoff: Duration) {
    #[cfg(not(test))]
    tokio::time::sleep(backoff).await;
    #[cfg(test)]
    let _ = backoff;
}

fn stage_result(
    backend_name: &str,
    attempt: u32,
    phase: BleLifecyclePhase,
    backoff: Duration,
    can_retry: bool,
    transitions: &mut Vec<BleLifecycleTransition>,
    result: Result<(), BleBackendError>,
) -> Option<BleBackendError> {
    match result {
        Ok(()) => {
            transitions.push(BleLifecycleTransition {
                attempt,
                phase,
                outcome: BleLifecycleOutcome::Ok,
                detail: None,
                backoff_ms: None,
            });
            None
        }
        Err(err) => {
            let will_retry = err.retryable && can_retry;
            let outcome =
                if will_retry { BleLifecycleOutcome::Retry } else { BleLifecycleOutcome::Failed };
            let backoff_ms = will_retry.then_some(backoff.as_millis() as u64);
            transitions.push(BleLifecycleTransition {
                attempt,
                phase,
                outcome,
                detail: Some(err.message.clone()),
                backoff_ms,
            });
            if will_retry {
                log::warn!(
                    "[daemon] ble_gatt backend={} phase={} retrying attempt={} backoff_ms={} err={}",
                    backend_name,
                    phase.as_str(),
                    attempt,
                    backoff.as_millis(),
                    err.message
                );
            }
            Some(err)
        }
    }
}

fn runtime_settings(iface: &InterfaceConfig) -> Result<BleRuntimeSettings, String> {
    let peripheral_id = required_non_empty(iface.peripheral_id.as_deref(), "peripheral_id")?;
    let service_uuid = required_non_empty(iface.service_uuid.as_deref(), "service_uuid")?;
    let write_char_uuid = required_non_empty(iface.write_char_uuid.as_deref(), "write_char_uuid")?;
    let notify_char_uuid =
        required_non_empty(iface.notify_char_uuid.as_deref(), "notify_char_uuid")?;

    let adapter = iface
        .adapter
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let mtu = iface.mtu.unwrap_or(247).clamp(23, 517);
    let scan_timeout_ms = iface.scan_timeout_ms.unwrap_or(5_000);
    let connect_timeout_ms = iface.ble_connect_timeout_ms.or(iface.connect_timeout_ms).unwrap_or(10_000);
    let reconnect_backoff_ms = iface.reconnect_backoff_ms.unwrap_or(500).max(50);
    let max_reconnect_backoff_ms =
        iface.max_reconnect_backoff_ms.unwrap_or_else(|| reconnect_backoff_ms.max(5_000));
    if max_reconnect_backoff_ms < reconnect_backoff_ms {
        return Err("ble_gatt.max_reconnect_backoff_ms must be >= ble_gatt.reconnect_backoff_ms"
            .to_string());
    }

    Ok(BleRuntimeSettings {
        adapter,
        peripheral_id,
        service_uuid,
        write_char_uuid,
        notify_char_uuid,
        mtu,
        scan_timeout: Duration::from_millis(scan_timeout_ms),
        connect_timeout: Duration::from_millis(connect_timeout_ms),
        reconnect_backoff: Duration::from_millis(reconnect_backoff_ms),
        max_reconnect_backoff: Duration::from_millis(max_reconnect_backoff_ms),
    })
}

fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

fn required_non_empty(value: Option<&str>, field: &str) -> Result<String, String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("ble_gatt.{field} is required"))
}

fn startup_probe_mismatch_diagnostic(expected_len: usize, actual_len: usize) -> &'static str {
    if actual_len == DEFAULT_ATT_NOTIFICATION_PAYLOAD_BYTES
        || actual_len < expected_len && actual_len < DEFAULT_ATT_NOTIFICATION_PAYLOAD_BYTES
    {
        "; likely ATT MTU 23 / 20-byte notification payload; the BLE host/backend did not report a usable negotiated MTU before notifications, so verify btleplug 0.12+ platform MTU support or configure a host adapter that negotiates a larger ATT MTU"
    } else {
        ""
    }
}

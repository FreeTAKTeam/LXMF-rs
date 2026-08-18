use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::{sleep, timeout, Instant};

use crate::buffer::InputBuffer;
use crate::iface::{IfaceSource, Interface, InterfaceContext, RxMessage};
use crate::packet::Packet;

use super::lora::LoraConfig;
use super::rnode_bearer::{
    RnodeBearerBackend, RnodeBearerInfo, RnodeBearerKind, RnodeBearerKissRuntime,
};
use super::rnode_ble::{
    rnode_ble_initial_runtime_status_json, rnode_ble_payload_writes_enabled,
    RnodeBleCommandMonitor, RnodeBleKissConfig,
};

const IO_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STARTUP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
const DETECTION_FALLBACK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct RnodeBearerRuntimeStatusHandle {
    inner: Arc<Mutex<serde_json::Value>>,
}

impl RnodeBearerRuntimeStatusHandle {
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }
}

/// A single RNode bearer attempt integrated with the Reticulum interface manager.
///
/// It intentionally does not reconnect. The platform owner creates a fresh backend
/// and interface after this task returns.
pub struct RnodeBearerKissInterface<B> {
    label: String,
    endpoint: String,
    backend: Option<B>,
    config: RnodeBleKissConfig,
    lora: LoraConfig,
    status: Arc<Mutex<serde_json::Value>>,
}

impl<B> RnodeBearerKissInterface<B> {
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        endpoint: impl Into<String>,
        backend: B,
        config: RnodeBleKissConfig,
        lora: LoraConfig,
    ) -> Self {
        let endpoint = endpoint.into();
        let status = Arc::new(Mutex::new(rnode_ble_initial_runtime_status_json(lora, &endpoint)));
        Self { label: label.into(), endpoint, backend: Some(backend), config, lora, status }
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> RnodeBearerRuntimeStatusHandle {
        RnodeBearerRuntimeStatusHandle { inner: self.status.clone() }
    }

    pub async fn spawn(context: InterfaceContext<Self>)
    where
        B: RnodeBearerBackend + Send + 'static,
    {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (rx_channel, mut tx_channel) = context.channel.split();
        let (label, endpoint, backend, config, lora, status) = {
            let mut guard = context.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                guard.label.clone(),
                guard.endpoint.clone(),
                guard.backend.take(),
                guard.config.clone(),
                guard.lora,
                guard.status.clone(),
            )
        };
        let Some(backend) = backend else {
            set_error_status(&status, "RNode bearer backend was already consumed");
            iface_stop.cancel();
            return;
        };
        let packet_mtu = config.mtu;
        let id_beacon = config.kiss.id_beacon.clone();
        let mut runtime = RnodeBearerKissRuntime::new(backend, config);
        let info = tokio::select! {
            result = runtime.startup() => match result {
                Ok(info) => info,
                Err(error) => {
                    set_error_status(&status, &format!("RNode bearer startup failed: {error:?}"));
                    close_after_aborted_startup(
                        &mut runtime,
                        &status,
                        &label,
                        "startup_failure",
                    )
                    .await;
                    iface_stop.cancel();
                    return;
                }
            },
            () = context.cancel.cancelled() => {
                close_after_aborted_startup(
                    &mut runtime,
                    &status,
                    &label,
                    "startup_cancellation",
                )
                .await;
                iface_stop.cancel();
                return;
            }
        };

        let bearer = bearer_label(info);
        let mut monitor = RnodeBleCommandMonitor::new(lora, STARTUP_RESPONSE_TIMEOUT);
        publish_monitor_status(&status, &monitor, &endpoint, bearer);
        let mut radio_config_sent = false;
        let detection_deadline = Instant::now() + DETECTION_FALLBACK_TIMEOUT;
        let mut first_tx_at: Option<Instant> = None;
        let mut attempt_failed = false;
        let mut cancelled = false;

        while !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
            if !radio_config_sent && Instant::now() >= detection_deadline {
                radio_config_sent = true;
                let Some(result) =
                    run_or_cancel(runtime.send_deferred_frames(), &context.cancel, &iface_stop)
                        .await
                else {
                    cancelled = true;
                    break;
                };
                match result {
                    Ok(()) => monitor.accept_degraded_startup(),
                    Err(error) => {
                        set_error_status(
                            &status,
                            &format!("RNode radio configuration failed: {error:?}"),
                        );
                        break;
                    }
                }
            }

            if rnode_ble_payload_writes_enabled(radio_config_sent, Some(&monitor)) {
                while let Ok(message) = tx_channel.try_recv() {
                    let raw = match message.packet.to_bytes() {
                        Ok(raw) => raw,
                        Err(error) => {
                            log::warn!(
                                "RNode packet serialization failed iface={label} error={error:?}"
                            );
                            continue;
                        }
                    };
                    if raw.len() > packet_mtu {
                        log::warn!(
                            "RNode packet exceeds configured MTU iface={} actual={} mtu={}",
                            label,
                            raw.len(),
                            packet_mtu
                        );
                        continue;
                    }
                    let Some(result) =
                        run_or_cancel(runtime.send_packet(&raw), &context.cancel, &iface_stop)
                            .await
                    else {
                        cancelled = true;
                        attempt_failed = true;
                        break;
                    };
                    if let Err(error) = result {
                        set_error_status(&status, &format!("RNode packet write failed: {error:?}"));
                        attempt_failed = true;
                        break;
                    }
                    first_tx_at.get_or_insert_with(Instant::now);
                }
            }
            if attempt_failed {
                break;
            }

            if let (Some(beacon), Some(first_tx)) = (id_beacon.as_ref(), first_tx_at) {
                if first_tx.elapsed() >= beacon.interval {
                    let Some(result) =
                        run_or_cancel(runtime.send_id_beacon(), &context.cancel, &iface_stop).await
                    else {
                        cancelled = true;
                        break;
                    };
                    if let Err(error) = result {
                        set_error_status(
                            &status,
                            &format!("RNode station ID write failed: {error:?}"),
                        );
                        break;
                    }
                    first_tx_at = None;
                }
            }

            let polled = tokio::select! {
                result = timeout(IO_POLL_INTERVAL, runtime.poll()) => result,
                () = context.cancel.cancelled() => {
                    cancelled = true;
                    break;
                },
                () = iface_stop.cancelled() => {
                    cancelled = true;
                    break;
                },
            };
            match polled {
                Err(_) => {}
                Ok(Err(error)) => {
                    set_error_status(&status, &format!("RNode bearer read failed: {error:?}"));
                    break;
                }
                Ok(Ok(None)) => {
                    tokio::select! {
                        () = sleep(IO_POLL_INTERVAL) => {}
                        () = context.cancel.cancelled() => {
                            cancelled = true;
                            break;
                        }
                        () = iface_stop.cancelled() => {
                            cancelled = true;
                            break;
                        }
                    }
                }
                Ok(Ok(Some(notification))) => {
                    if let Err(error) = monitor.accept_notification(&notification) {
                        set_error_status(&status, &error);
                        break;
                    }
                    if !radio_config_sent && monitor.is_detected() {
                        radio_config_sent = true;
                        let Some(result) = run_or_cancel(
                            runtime.send_deferred_frames(),
                            &context.cancel,
                            &iface_stop,
                        )
                        .await
                        else {
                            cancelled = true;
                            break;
                        };
                        if let Err(error) = result {
                            set_error_status(
                                &status,
                                &format!("RNode radio configuration failed: {error:?}"),
                            );
                            break;
                        }
                        monitor.reset_startup_deadline(STARTUP_RESPONSE_TIMEOUT);
                    }
                    publish_monitor_status(&status, &monitor, &endpoint, bearer);
                    for payload in notification.packets {
                        match Packet::deserialize(&mut InputBuffer::new(&payload)) {
                            Ok(packet) => {
                                if rx_channel
                                    .send(RxMessage {
                                        address: iface_address,
                                        packet,
                                        source: IfaceSource::None,
                                    })
                                    .await
                                    .is_err()
                                {
                                    iface_stop.cancel();
                                    break;
                                }
                            }
                            Err(error) => log::warn!(
                                "RNode packet deserialize failed iface={} len={} error={error:?}",
                                label,
                                payload.len()
                            ),
                        }
                    }
                }
            }
            if let Err(error) = monitor.validate_startup_deadline() {
                log::warn!("RNode startup response validation failed iface={label} error={error}");
                set_error_status(&status, &error);
                break;
            }
            publish_monitor_status(&status, &monitor, &endpoint, bearer);
        }

        if cancelled || context.cancel.is_cancelled() || iface_stop.is_cancelled() {
            if let Err(error) = runtime.close().await {
                log::warn!("RNode bearer close failed iface={label} error={error:?}");
            }
        } else {
            let shutdown_prefix = monitor.external_framebuffer_frame(false).into_iter().collect();
            if let Err(error) = runtime.shutdown_with_prefix_frames(shutdown_prefix).await {
                log::warn!("RNode bearer shutdown failed iface={label} error={error:?}");
            }
        }
        iface_stop.cancel();
    }
}

async fn run_or_cancel<T>(
    future: impl Future<Output = T>,
    cancel: &tokio_util::sync::CancellationToken,
    stop: &tokio_util::sync::CancellationToken,
) -> Option<T> {
    tokio::select! {
        result = future => Some(result),
        () = cancel.cancelled() => None,
        () = stop.cancelled() => None,
    }
}

impl<B> Interface for RnodeBearerKissInterface<B> {
    fn mtu() -> usize {
        508
    }

    fn configured_mtu(&self) -> usize {
        self.config.mtu
    }
}

fn bearer_label(info: RnodeBearerInfo) -> &'static str {
    match info.kind {
        RnodeBearerKind::Ble => "ble",
        RnodeBearerKind::BluetoothClassic => "bluetooth_classic",
    }
}

fn publish_monitor_status(
    status: &Arc<Mutex<serde_json::Value>>,
    monitor: &RnodeBleCommandMonitor,
    endpoint: &str,
    bearer: &str,
) {
    let mut value = monitor.runtime_status_json(endpoint);
    if let Some(object) = value.as_object_mut() {
        object.insert("bearer".to_string(), serde_json::Value::String(bearer.to_string()));
    }
    *status.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

fn set_error_status(status: &Arc<Mutex<serde_json::Value>>, error: &str) {
    let mut guard = status.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(object) = guard.as_object_mut() else {
        return;
    };
    object.insert("online".to_string(), serde_json::Value::Bool(false));
    object.insert("last_command_error".to_string(), serde_json::Value::String(error.to_string()));
}

async fn close_after_aborted_startup<B>(
    runtime: &mut RnodeBearerKissRuntime<B>,
    status: &Arc<Mutex<serde_json::Value>>,
    label: &str,
    phase: &str,
) where
    B: RnodeBearerBackend,
{
    if let Err(error) = runtime.close().await {
        let message =
            format!("RNode bearer close failed iface={label} phase={phase} error={error:?}");
        log::warn!("{message}");
        append_error_status(status, &message);
    }
}

fn append_error_status(status: &Arc<Mutex<serde_json::Value>>, error: &str) {
    let mut guard = status.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = guard
        .get("last_command_error")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let Some(object) = guard.as_object_mut() else {
        return;
    };
    let combined = previous.map_or_else(|| error.to_string(), |value| format!("{value}; {error}"));
    object.insert("online".to_string(), serde_json::Value::Bool(false));
    object.insert("last_command_error".to_string(), serde_json::Value::String(combined));
}

#[cfg(test)]
#[path = "rnode_bearer_interface_tests.rs"]
mod tests;

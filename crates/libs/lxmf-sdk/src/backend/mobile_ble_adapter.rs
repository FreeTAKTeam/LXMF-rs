//! Opt-in projection into the existing mobile contract. No native backend feature
//! is enabled here; the composition layer supplies the authoritative transport.
use super::mobile_ble::*;
use crate::{
    error::{code, ErrorCategory, SdkError},
    types::Ack,
};
use rns_bluetooth as bt;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone)]
struct Channels {
    write: bt::CharacteristicId,
    notify: bt::CharacteristicId,
}

/// An owner must be exclusive to this adapter, including its backend event queue.
/// The map records only the request's channel projection, never connection state.
#[derive(Default)]
struct Projection {
    pending: BTreeMap<String, PendingProjection>,
    sessions: BTreeMap<u64, Channels>,
}

struct PendingProjection {
    channels: Channels,
    published: bool,
}

pub struct SharedMobileBleAdapter {
    backend: Arc<dyn bt::GattClient>,
    owner: bt::OwnerId,
    channels: Mutex<Projection>,
}
impl SharedMobileBleAdapter {
    pub fn new(backend: Arc<dyn bt::GattClient>, owner: bt::OwnerId) -> Result<Self, SdkError> {
        backend.limits().validate().map_err(map_error)?;
        if owner.0.trim().is_empty() || !backend.capabilities().gatt_client {
            return Err(SdkError::capability_disabled("sdk.capability.mobile_ble"));
        }
        Ok(Self { backend, owner, channels: Mutex::new(Projection::default()) })
    }
    fn session(&self, value: &str) -> Result<bt::SessionId, SdkError> {
        let generation = value
            .strip_prefix("bluetooth:")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .ok_or_else(invalid)?;
        Ok(bt::SessionId { owner: self.owner.clone(), generation })
    }
    fn route(&self, session: &bt::SessionId) -> Result<Channels, SdkError> {
        self.channels
            .lock()
            .map_err(|_| internal())?
            .sessions
            .get(&session.generation)
            .cloned()
            .ok_or_else(invalid)
    }
}
fn token(id: &bt::SessionId) -> String {
    format!("bluetooth:{}", id.generation)
}
fn deadline(ms: u64) -> Result<bt::Deadline, SdkError> {
    bt::Deadline::after(Duration::from_millis(ms)).map_err(map_error)
}
fn invalid() -> SdkError {
    SdkError::new(
        code::VALIDATION_INVALID_ARGUMENT,
        ErrorCategory::Validation,
        "invalid mobile BLE request",
    )
}
fn internal() -> SdkError {
    SdkError::new(code::INTERNAL, ErrorCategory::Internal, "mobile BLE adapter unavailable")
}
fn operation(id: &str) -> Result<(), SdkError> {
    if id.trim().is_empty() {
        Err(invalid())
    } else {
        Ok(())
    }
}
fn map_error(error: bt::BluetoothError) -> SdkError {
    use bt::ErrorKind as K;
    let (code, category) = match error.kind {
        K::InvalidInput | K::PayloadTooLarge => {
            (code::VALIDATION_INVALID_ARGUMENT, ErrorCategory::Validation)
        }
        K::Unsupported => (code::CAPABILITY_DISABLED, ErrorCategory::Capability),
        K::PermissionDenied => (code::SECURITY_AUTHZ_DENIED, ErrorCategory::Security),
        K::Timeout => (code::RUNTIME_STREAM_DEGRADED, ErrorCategory::Timeout),
        _ => (code::RUNTIME_STREAM_DEGRADED, ErrorCategory::Transport),
    };
    SdkError::new(code, category, error.to_string())
}
impl MobileBleHostAdapter for SharedMobileBleAdapter {
    fn capabilities(&self) -> MobileBleCapabilities {
        let capabilities = self.backend.capabilities();
        let limits = self.backend.limits();
        MobileBleCapabilities {
            supports_background_restore: capabilities.background_restore,
            supports_write_without_response: capabilities.write_without_response,
            supports_operation_cancel: capabilities.operation_cancel,
            max_notification_queue: limits.max_events,
            max_payload_bytes: limits.max_payload_bytes,
        }
    }
    fn connect(
        &self,
        req: MobileBleConnectRequest,
    ) -> Result<MobileBleSessionDescriptor, SdkError> {
        operation(&req.operation_id)?;
        if [&req.peripheral_id, &req.service_uuid, &req.write_char_uuid, &req.notify_char_uuid]
            .iter()
            .any(|s| s.trim().is_empty())
        {
            return Err(invalid());
        }
        let timeout = deadline(req.connect_timeout_ms)?;
        let channels = Channels {
            write: bt::CharacteristicId {
                service: req.service_uuid.clone(),
                characteristic: req.write_char_uuid,
            },
            notify: bt::CharacteristicId {
                service: req.service_uuid,
                characteristic: req.notify_char_uuid,
            },
        };
        {
            let mut projection = self.channels.lock().map_err(|_| internal())?;
            if projection.pending.contains_key(&req.operation_id) {
                return Err(invalid());
            }
            // Only bound in-flight projection records; backend owns live admission.
            if projection.pending.len() >= self.backend.limits().max_sessions {
                return Err(invalid());
            }
            projection.pending.insert(
                req.operation_id.clone(),
                PendingProjection { channels: channels.clone(), published: false },
            );
        }
        let opened = self.backend.connect(bt::ConnectRequest {
            owner: self.owner.clone(),
            device: bt::DeviceId(req.peripheral_id),
            operation_id: req.operation_id.clone(),
            characteristics: vec![channels.write.clone(), channels.notify.clone()],
            subscriptions: vec![channels.notify.clone()],
            deadline: timeout,
        });
        let mut projection = self.channels.lock().map_err(|_| internal())?;
        let pending = projection.pending.remove(&req.operation_id).ok_or_else(internal)?;
        let session = opened.map_err(map_error)?;
        if !pending.published {
            projection.sessions.insert(session.id.generation, channels);
        }
        drop(projection);
        let configured = (|| {
            if session.id.owner != self.owner {
                return Err(internal());
            }
            let mtu = session.att_mtu.filter(|mtu| *mtu >= 23).ok_or_else(invalid)?;
            Ok(MobileBleSessionDescriptor {
                session_id: token(&session.id),
                peripheral_id: session.device.0.clone(),
                negotiated_mtu: mtu,
            })
        })();
        match configured {
            Ok(descriptor) => Ok(descriptor),
            Err(error) => {
                let _ = self.backend.close(&session.id);
                Err(error)
            }
        }
    }
    fn disconnect(&self, session_id: &str) -> Result<Ack, SdkError> {
        self.backend.close(&self.session(session_id)?).map_err(map_error)?;
        Ok(Ack { accepted: true, revision: None })
    }
    fn write(&self, req: MobileBleWriteRequest) -> Result<MobileBleWriteAck, SdkError> {
        operation(&req.operation_id)?;
        if req.payload.is_empty() || req.payload.len() > self.capabilities().max_payload_bytes {
            return Err(invalid());
        }
        if !req.require_response && !self.capabilities().supports_write_without_response {
            return Err(SdkError::capability_disabled(
                "sdk.capability.mobile_ble_write_without_response",
            ));
        }
        let timeout = deadline(req.write_timeout_ms)?;
        let session = self.session(&req.session_id)?;
        let route = self.route(&session)?;
        let bytes_written = self
            .backend
            .write(bt::WriteRequest {
                session,
                operation_id: req.operation_id.clone(),
                characteristic: route.write,
                mode: if req.require_response {
                    bt::WriteMode::WithResponse
                } else {
                    bt::WriteMode::WithoutResponse
                },
                payload: req.payload,
                deadline: timeout,
            })
            .map_err(map_error)?;
        Ok(MobileBleWriteAck { operation_id: req.operation_id, bytes_written })
    }
    fn read(&self, req: MobileBleReadRequest) -> Result<MobileBleReadResult, SdkError> {
        operation(&req.operation_id)?;
        if req.max_bytes == 0 || req.max_bytes > self.capabilities().max_payload_bytes {
            return Err(invalid());
        }
        let timeout = deadline(req.read_timeout_ms)?;
        let session = self.session(&req.session_id)?;
        let route = self.route(&session)?;
        let payload = self
            .backend
            .read(bt::ReadRequest {
                session,
                characteristic: route.notify,
                max_bytes: req.max_bytes,
                operation_id: req.operation_id.clone(),
                deadline: timeout,
            })
            .map_err(map_error)?;
        if payload.len() > req.max_bytes {
            return Err(invalid());
        }
        Ok(MobileBleReadResult { operation_id: req.operation_id, payload })
    }
    fn poll_event(&self, timeout_ms: u64) -> Result<Option<MobileBleEvent>, SdkError> {
        let timeout = deadline(timeout_ms)?;
        loop {
            let Some(event) = self.backend.poll_event(&self.owner, timeout).map_err(map_error)?
            else {
                return Ok(None);
            };
            if event.session.owner != self.owner {
                return Err(internal());
            }
            let mut routes = self.channels.lock().map_err(|_| internal())?;
            let mut projected = MobileBleEvent {
                sequence_no: event.sequence,
                session_id: token(&event.session),
                operation_id: event.operation_id,
                kind: MobileBleEventKind::Connected,
                payload: None,
                error: None,
            };
            projected.kind = match event.kind {
                bt::EventKind::Connected => {
                    if !routes.sessions.contains_key(&event.session.generation) {
                        let selected = projected
                            .operation_id
                            .as_ref()
                            .and_then(|id| routes.pending.get_mut(id))
                            .ok_or_else(internal)?;
                        selected.published = true;
                        let channels = selected.channels.clone();
                        routes.sessions.insert(event.session.generation, channels);
                    }
                    MobileBleEventKind::Connected
                }
                bt::EventKind::Disconnected => {
                    routes.sessions.remove(&event.session.generation);
                    MobileBleEventKind::Disconnected
                }
                bt::EventKind::Notification { characteristic, payload } => {
                    if routes
                        .sessions
                        .get(&event.session.generation)
                        .is_none_or(|r| r.notify != characteristic)
                    {
                        if timeout.remaining().is_err() {
                            return Ok(None);
                        }
                        continue;
                    }
                    projected.payload = Some(payload);
                    MobileBleEventKind::Notification
                }
                bt::EventKind::WriteComplete { .. } => MobileBleEventKind::WriteComplete,
                bt::EventKind::Error(error) => {
                    let kind = if error.kind == bt::ErrorKind::Timeout {
                        MobileBleEventKind::Timeout
                    } else {
                        MobileBleEventKind::Error
                    };
                    projected.error = Some(map_error(error).to_string());
                    kind
                }
            };
            validate_event_payload_bounds(std::slice::from_ref(&projected), &self.capabilities())?;
            return Ok(Some(projected));
        }
    }
    fn cancel_operation(&self, operation_id: &str) -> Result<Ack, SdkError> {
        operation(operation_id)?;
        if !self.capabilities().supports_operation_cancel {
            return Err(SdkError::capability_disabled("sdk.capability.mobile_ble_cancel"));
        }
        self.backend.cancel(&self.owner, operation_id).map_err(map_error)?;
        Ok(Ack { accepted: true, revision: None })
    }
}

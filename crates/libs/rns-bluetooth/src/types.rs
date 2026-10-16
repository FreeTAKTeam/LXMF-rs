use std::time::{Duration, Instant};

use crate::{BluetoothError, ErrorKind, Result};

/// Opaque platform token. It is not a stable application/peer identity.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DeviceId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct OwnerId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SessionId {
    pub owner: OwnerId,
    pub generation: u64,
}

/// Preserve service and characteristic identity, even for multi-channel devices.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CharacteristicId {
    pub service: String,
    pub characteristic: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteMode {
    WithResponse,
    WithoutResponse,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackendCapabilities {
    pub gatt_client: bool,
    pub gatt_server: bool,
    pub advertising: bool,
    pub simultaneous_roles: bool,
    pub classic_stream: bool,
    pub operation_cancel: bool,
    pub background_restore: bool,
    pub write_without_response: bool,
}

/// All limits are caller supplied. No device capacity or operational policy is inferred.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_sessions: usize,
    pub max_events: usize,
    pub max_payload_bytes: usize,
}
impl Limits {
    pub fn validate(self) -> Result<Self> {
        if self.max_sessions == 0 || self.max_events == 0 || self.max_payload_bytes == 0 {
            return Err(BluetoothError::new(ErrorKind::InvalidInput, "limits"));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Deadline(Instant);
impl Deadline {
    pub fn after(timeout: Duration) -> Result<Self> {
        if timeout.is_zero() {
            return Err(BluetoothError::new(ErrorKind::InvalidInput, "deadline"));
        }
        Instant::now()
            .checked_add(timeout)
            .map(Self)
            .ok_or_else(|| BluetoothError::new(ErrorKind::InvalidInput, "deadline"))
    }
    pub fn remaining(self) -> Result<Duration> {
        self.0
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| BluetoothError::new(ErrorKind::Timeout, "deadline"))
    }
}

#[derive(Clone, Debug)]
pub struct DiscoveryRequest {
    pub services: Vec<String>,
    pub max_results: usize,
    pub deadline: Deadline,
}

#[derive(Clone, Debug)]
pub struct ConnectRequest {
    pub owner: OwnerId,
    pub device: DeviceId,
    pub operation_id: String,
    pub characteristics: Vec<CharacteristicId>,
    /// Installed within the same cancellable open operation, before Connected.
    pub subscriptions: Vec<CharacteristicId>,
    pub deadline: Deadline,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionInfo {
    pub id: SessionId,
    pub device: DeviceId,
    pub att_mtu: Option<u16>,
    pub max_write_bytes: usize,
    pub max_notification_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventKind {
    Connected,
    Disconnected,
    Notification { characteristic: CharacteristicId, payload: Vec<u8> },
    WriteComplete { bytes_written: usize },
    Error(BluetoothError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BluetoothEvent {
    pub sequence: u64,
    pub session: SessionId,
    pub operation_id: Option<String>,
    pub kind: EventKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacteristicProperties {
    pub id: CharacteristicId,
    pub read: bool,
    pub write: bool,
    pub write_without_response: bool,
    pub notify: bool,
    pub indicate: bool,
}

#[derive(Clone, Debug)]
pub struct WriteRequest {
    pub session: SessionId,
    pub operation_id: String,
    pub characteristic: CharacteristicId,
    pub mode: WriteMode,
    pub payload: Vec<u8>,
    pub deadline: Deadline,
}

#[derive(Clone, Debug)]
pub struct ServerDefinition {
    pub owner: OwnerId,
    pub characteristics: Vec<CharacteristicProperties>,
    pub advertise_services: Vec<String>,
    pub deadline: Deadline,
}

/// A characteristic read with a cancellable operation identity and native budget.
#[derive(Clone, Debug)]
pub struct ReadRequest {
    pub session: SessionId,
    pub operation_id: String,
    pub characteristic: CharacteristicId,
    pub max_bytes: usize,
    pub deadline: Deadline,
}

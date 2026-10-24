#[cfg(feature = "embedded")]
use crate::Error;

pub const CAPACITY_BYTES: u64 = 26_214_400;
pub const RETENTION_SECS: i64 = 7 * 24 * 60 * 60;
pub const MAX_PENDING: usize = 4096;
pub const MAX_STATUS: usize = 8192;

#[derive(Clone, Copy, Debug)]
pub struct StoreLimits {
    pub capacity_bytes: u64,
    pub retention_secs: i64,
}
impl Default for StoreLimits {
    fn default() -> Self {
        Self { capacity_bytes: CAPACITY_BYTES, retention_secs: RETENTION_SECS }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum State {
    StoredLocally = 0,
    Uploading = 1,
    UploadedUnverified = 2,
    RelayHolds = 3,
    Delivered = 4,
    Acknowledged = 5,
    Expired = 6,
    Evicted = 7,
}
impl State {
    #[cfg(feature = "embedded")]
    pub(crate) fn from_sql(value: i64) -> rusqlite::Result<Self> {
        match value {
            0 => Ok(Self::StoredLocally),
            1 => Ok(Self::Uploading),
            2 => Ok(Self::UploadedUnverified),
            3 => Ok(Self::RelayHolds),
            4 => Ok(Self::Delivered),
            5 => Ok(Self::Acknowledged),
            6 => Ok(Self::Expired),
            7 => Ok(Self::Evicted),
            _ => Err(rusqlite::Error::IntegralValueOutOfRange(0, value)),
        }
    }
    pub fn terminal(self) -> bool {
        (self as i64) >= Self::Delivered as i64
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Chat,
    Mission,
    Telemetry { key: String },
}
impl MessageKind {
    #[cfg(feature = "embedded")]
    pub(crate) fn sql(&self) -> Result<(i64, Option<&str>), Error> {
        match self {
            Self::Chat => Ok((0, None)),
            Self::Mission => Ok((1, None)),
            Self::Telemetry { key } if !key.is_empty() && key.len() <= 128 => Ok((2, Some(key))),
            _ => Err(Error::InvalidState("invalid telemetry key")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub transient_id: String,
    pub message_id: String,
    pub application_key: Option<String>,
    pub destination: String,
    pub state: State,
    pub kind: MessageKind,
    pub admitted_at: i64,
    pub expires_at: i64,
    pub encrypted_bytes: u64,
    pub uploaded: bool,
    pub relay: Option<String>,
    pub attempts: u32,
    pub next_attempt_at: i64,
}

/// Recovery material from the same durable record, not another queue entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryLease {
    pub(crate) transient_id: String,
    pub(crate) attempt: u32,
}
impl RecoveryLease {
    pub fn transient_id(&self) -> &str {
        &self.transient_id
    }
}

pub struct Recovery {
    pub lease: RecoveryLease,
    pub status: Status,
    pub direct_wire: Vec<u8>,
    pub recipient_transient: Vec<u8>,
}

/// Original recipient ciphertext; reading it neither retires nor hands off custody.
pub struct RetrievedMessage {
    pub status: Status,
    pub recipient_transient: Vec<u8>,
}

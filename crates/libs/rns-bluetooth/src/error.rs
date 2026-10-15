use std::fmt;

pub type Result<T> = std::result::Result<T, BluetoothError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    InvalidInput,
    Unsupported,
    PermissionDenied,
    Unavailable,
    Busy,
    StaleSession,
    Closed,
    Cancelled,
    Timeout,
    QueueFull,
    PayloadTooLarge,
    Platform,
}

/// Backend errors must not contain device identifiers or application payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BluetoothError {
    pub kind: ErrorKind,
    pub operation: &'static str,
}
impl BluetoothError {
    pub const fn new(kind: ErrorKind, operation: &'static str) -> Self {
        Self { kind, operation }
    }
}
impl fmt::Display for BluetoothError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bluetooth {}: {:?}", self.operation, self.kind)
    }
}
impl std::error::Error for BluetoothError {}

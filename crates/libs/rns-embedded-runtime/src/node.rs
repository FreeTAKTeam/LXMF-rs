use crate::constants::DEFAULT_CAPTURE_MAX_BYTES;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NodeTransportMode {
    BleOnly,
    TcpClient,
    TcpServer,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NodeLifecycleState {
    Boot,
    Unprovisioned,
    ProvisionedOffline,
    TcpOnline,
    BleRecovery,
    FailureReconnect,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CaptureDefaults {
    pub max_bytes: u32,
}

impl Default for CaptureDefaults {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_CAPTURE_MAX_BYTES,
        }
    }
}

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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TcpClientConfig {
    pub host: alloc::string::String,
    pub port: u16,
    pub reconnect_backoff_ms: alloc::vec::Vec<u64>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TcpServerConfig {
    pub listen_port: u16,
}

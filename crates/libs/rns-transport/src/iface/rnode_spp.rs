use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::iface::kiss::KissConfig;
use crate::kiss::{encode_data_frame, KissCommand, KissDecodeError, KissFrame, KissStreamDecoder};

pub const RNODE_SPP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const RNODE_SPP_READ_FRAME_TIMEOUT: Duration = Duration::from_millis(1_250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RnodeSppKissConfig {
    pub connect_timeout: Duration,
    pub read_frame_timeout: Duration,
    pub mtu: usize,
    pub initial_frames: Vec<Vec<u8>>,
    pub deferred_frames: Vec<Vec<u8>>,
    pub shutdown_frames: Vec<Vec<u8>>,
    pub kiss: KissConfig,
}

impl Default for RnodeSppKissConfig {
    fn default() -> Self {
        Self {
            connect_timeout: RNODE_SPP_CONNECT_TIMEOUT,
            read_frame_timeout: RNODE_SPP_READ_FRAME_TIMEOUT,
            mtu: 508,
            initial_frames: Vec::new(),
            deferred_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            kiss: KissConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RnodeSppKissStatus {
    pub connected: bool,
    pub interface_ready: bool,
    pub pending_payloads: usize,
    pub pending_writes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RnodeSppRead {
    pub packets: Vec<Vec<u8>>,
    pub commands: Vec<(u8, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RnodeSppKissError {
    Kiss(KissDecodeError),
    Backend { operation: &'static str, message: String },
    ConnectTimeout { timeout: Duration },
    PacketTooLarge { limit: usize, actual: usize },
}

impl From<KissDecodeError> for RnodeSppKissError {
    fn from(value: KissDecodeError) -> Self {
        Self::Kiss(value)
    }
}

#[allow(async_fn_in_trait)]
pub trait RnodeSppBackend {
    async fn connect(&mut self) -> Result<(), String>;

    async fn write(&mut self, payload: Vec<u8>) -> Result<(), String>;

    async fn read(&mut self) -> Result<Option<Vec<u8>>, String>;

    /// Close the current RFCOMM stream. The default preserves existing backends for one release.
    async fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RnodeSppSettings {
    pub device_id: String,
    pub device_name: String,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
}

impl RnodeSppSettings {
    #[must_use]
    pub fn for_device_id(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            device_name: String::new(),
            connect_timeout: RNODE_SPP_CONNECT_TIMEOUT,
            read_timeout: RNODE_SPP_READ_FRAME_TIMEOUT,
        }
    }

    #[must_use]
    pub fn with_device_name(mut self, device_name: impl Into<String>) -> Self {
        self.device_name = device_name.into();
        self
    }
}

#[cfg(target_os = "android")]
pub type AndroidRnodeSppSettings = RnodeSppSettings;

#[derive(Debug, Clone)]
pub struct RnodeSppKissSession {
    config: RnodeSppKissConfig,
    decoder: KissStreamDecoder,
    interface_ready: bool,
    last_read_at: Instant,
    pending_payloads: VecDeque<Vec<u8>>,
    pending_writes: VecDeque<Vec<u8>>,
}

impl RnodeSppKissSession {
    #[must_use]
    pub fn new(config: RnodeSppKissConfig) -> Self {
        Self {
            decoder: KissStreamDecoder::new(config.mtu),
            interface_ready: !config.kiss.flow_control,
            last_read_at: Instant::now(),
            pending_payloads: VecDeque::new(),
            pending_writes: VecDeque::new(),
            config,
        }
    }

    #[must_use]
    pub fn status(&self) -> RnodeSppKissStatus {
        self.status_with_connection(false)
    }

    fn status_with_connection(&self, connected: bool) -> RnodeSppKissStatus {
        RnodeSppKissStatus {
            connected,
            interface_ready: self.interface_ready,
            pending_payloads: self.pending_payloads.len(),
            pending_writes: self.pending_writes.len(),
        }
    }

    #[must_use]
    pub fn mtu(&self) -> usize {
        self.config.mtu
    }

    #[must_use]
    pub fn startup_frames(&mut self) -> Vec<Vec<u8>> {
        self.config
            .kiss
            .command_frames()
            .into_iter()
            .chain(self.config.initial_frames.iter().cloned())
            .collect()
    }

    #[must_use]
    pub fn deferred_frames(&self) -> Vec<Vec<u8>> {
        self.config.deferred_frames.clone()
    }

    #[must_use]
    pub fn shutdown_frames(&self) -> Vec<Vec<u8>> {
        self.config.shutdown_frames.clone()
    }

    #[must_use]
    pub fn enqueue_packet(&mut self, payload: &[u8]) -> Vec<Vec<u8>> {
        if self.config.kiss.flow_control && !self.interface_ready {
            self.pending_payloads.push_back(payload.to_vec());
            return Vec::new();
        }

        let write = encode_data_frame(payload);
        if self.config.kiss.flow_control {
            self.interface_ready = false;
        }
        vec![write]
    }

    pub fn accept_read(&mut self, payload: &[u8]) -> Result<RnodeSppRead, RnodeSppKissError> {
        if self.decoder.has_partial_frame()
            && self.last_read_at.elapsed() >= self.config.read_frame_timeout
        {
            self.decoder.clear_partial_frame();
        }
        self.last_read_at = Instant::now();
        let frames = self.decoder.push_bytes(payload)?;
        let mut read = RnodeSppRead::default();
        for frame in frames {
            match frame {
                KissFrame::Data(payload) => read.packets.push(payload),
                KissFrame::Command(KissCommand::Ready) => {
                    self.interface_ready = true;
                    self.flush_pending_payloads();
                }
                KissFrame::Command(KissCommand::Unknown(command, payload)) => {
                    read.commands.push((command, payload));
                }
            }
        }
        Ok(read)
    }

    #[must_use]
    pub fn take_pending_writes(&mut self) -> Vec<Vec<u8>> {
        self.pending_writes.drain(..).collect()
    }

    fn flush_pending_payloads(&mut self) {
        while self.interface_ready {
            let Some(payload) = self.pending_payloads.pop_front() else {
                break;
            };
            self.pending_writes.push_back(encode_data_frame(&payload));
            if self.config.kiss.flow_control {
                self.interface_ready = false;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RnodeSppKissRuntime<B> {
    backend: B,
    session: RnodeSppKissSession,
    connected: bool,
}

impl<B> RnodeSppKissRuntime<B>
where
    B: RnodeSppBackend,
{
    #[must_use]
    pub fn new(backend: B, config: RnodeSppKissConfig) -> Self {
        Self { backend, session: RnodeSppKissSession::new(config), connected: false }
    }

    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    #[must_use]
    pub fn into_backend(self) -> B {
        self.backend
    }

    #[must_use]
    pub fn status(&self) -> RnodeSppKissStatus {
        self.session.status_with_connection(self.connected)
    }

    pub async fn startup(&mut self) -> Result<(), RnodeSppKissError> {
        self.connected = false;
        let connect_timeout = self.session.config.connect_timeout;
        tokio::time::timeout(connect_timeout, self.backend.connect())
            .await
            .map_err(|_| RnodeSppKissError::ConnectTimeout { timeout: connect_timeout })?
            .map_err(|message| RnodeSppKissError::Backend { operation: "connect", message })?;
        let writes = self.session.startup_frames();
        self.write_all(writes, "startup_write").await?;
        self.connected = true;
        Ok(())
    }

    pub async fn send_deferred_frames(&mut self) -> Result<(), RnodeSppKissError> {
        self.write_all(self.session.deferred_frames(), "deferred_frames_write").await
    }

    pub async fn send_packet(&mut self, payload: &[u8]) -> Result<(), RnodeSppKissError> {
        if payload.len() > self.session.mtu() {
            return Err(RnodeSppKissError::PacketTooLarge {
                limit: self.session.mtu(),
                actual: payload.len(),
            });
        }
        let writes = self.session.enqueue_packet(payload);
        self.write_all(writes, "write_packet").await
    }

    pub async fn shutdown(&mut self) -> Result<(), RnodeSppKissError> {
        let write_result = self.write_all(self.session.shutdown_frames(), "shutdown_write").await;
        let close_result = self
            .backend
            .close()
            .await
            .map_err(|message| RnodeSppKissError::Backend { operation: "close", message });
        self.connected = false;
        write_result.and(close_result)
    }

    pub async fn poll_read(&mut self) -> Result<Vec<Vec<u8>>, RnodeSppKissError> {
        Ok(self.poll_read_events().await?.packets)
    }

    pub async fn poll_read_events(&mut self) -> Result<RnodeSppRead, RnodeSppKissError> {
        let Some(payload) = self.backend.read().await.map_err(|message| {
            self.connected = false;
            RnodeSppKissError::Backend { operation: "read", message }
        })?
        else {
            return Ok(RnodeSppRead::default());
        };
        let read = self.session.accept_read(&payload)?;
        let writes = self.session.take_pending_writes();
        self.write_all(writes, "write_pending").await?;
        Ok(read)
    }

    async fn write_all(
        &mut self,
        writes: Vec<Vec<u8>>,
        operation: &'static str,
    ) -> Result<(), RnodeSppKissError> {
        for write in writes {
            self.backend.write(write).await.map_err(|message| {
                self.connected = false;
                RnodeSppKissError::Backend { operation, message }
            })?;
        }
        Ok(())
    }
}

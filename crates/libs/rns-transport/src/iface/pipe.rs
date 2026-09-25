use alloc::string::String;
use alloc::vec::Vec;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::{Interface, InterfaceContext, TxMessage};
use crate::buffer::OutputBuffer;
use crate::hash::AddressHash;
use crate::iface::{
    decode_packet_ifac, encode_packet_ifac, hdlc::Hdlc, is_ifac_violation, record_ifac_violation,
    IfacState, IfaceRole, IfaceSource, InterfaceManager, RxMessage, MAX_IFAC_SIZE_BYTES,
};

#[path = "pipe_parts/process_cleanup.rs"]
mod process_cleanup;
use process_cleanup::terminate_pipe_child;

#[path = "pipe_parts/status.rs"]
mod status;
use status::update_pipe_status;

pub struct PipeInterface {
    command: String,
    respawn_delay: Duration,
    mtu: usize,
    runtime_status: Arc<std::sync::Mutex<PipeRuntimeStatus>>,
}

impl PipeInterface {
    pub const DEFAULT_MTU: usize = 1_064;
    pub const DEFAULT_RESPAWN_DELAY: Duration = Duration::from_secs(5);

    pub fn new<T: Into<String>>(command: T) -> Self {
        let command = command.into();
        Self {
            command: command.clone(),
            respawn_delay: Self::DEFAULT_RESPAWN_DELAY,
            mtu: Self::DEFAULT_MTU,
            runtime_status: Arc::new(std::sync::Mutex::new(PipeRuntimeStatus::new(command))),
        }
    }

    #[must_use]
    pub fn with_respawn_delay(mut self, respawn_delay: Duration) -> Self {
        self.respawn_delay = respawn_delay;
        self
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(256);
        self
    }

    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }

    #[must_use]
    pub fn runtime_status_json(&self) -> serde_json::Value {
        self.runtime_status.lock().expect("pipe runtime status mutex poisoned").to_json()
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> PipeRuntimeStatusHandle {
        PipeRuntimeStatusHandle { inner: self.runtime_status.clone() }
    }

    #[must_use]
    pub fn mtu_value(&self) -> usize {
        self.mtu
    }

    pub fn parse_command(command: &str) -> Result<Vec<String>, String> {
        let argv = shlex::split(command)
            .ok_or_else(|| "pipe.command contains unterminated shell quoting".to_string())?;
        if argv.is_empty() {
            return Err("pipe.command is required".to_string());
        }
        Ok(argv)
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let ifac_state = context.channel.ifac_state.clone();
        let ifac_violations = context.channel.ifac_violations.clone();
        let online = context.channel.online.clone();
        online.store(false, std::sync::atomic::Ordering::Release);
        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        loop {
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            let (command, respawn_delay, mtu) = {
                let guard = context.inner.lock().expect("pipe interface mutex poisoned");
                (guard.command.clone(), guard.respawn_delay, guard.mtu)
            };
            let runtime_status = {
                let guard = context.inner.lock().expect("pipe interface mutex poisoned");
                guard.runtime_status.clone()
            };
            update_pipe_status(&runtime_status, |status| {
                status.command.clone_from(&command);
                status.process_state = "spawning".to_string();
                status.pipe_is_open = false;
                status.last_error = None;
            });

            if let Err(err) = run_pipe_process(
                command.as_str(),
                iface_address,
                mtu,
                context.cancel.clone(),
                iface_stop.clone(),
                rx_channel.clone(),
                tx_channel.clone(),
                runtime_status.clone(),
                online.clone(),
                ifac_state.clone(),
                ifac_violations.clone(),
            )
            .await
            {
                update_pipe_status(&runtime_status, |status| {
                    status.process_state = "respawning".to_string();
                    status.pipe_is_open = false;
                    status.respawn_attempts = status.respawn_attempts.saturating_add(1);
                    status.last_error = Some(err.clone());
                });
                log::warn!("pipe_interface command failed iface={} err={}", iface_address, err);
            } else if !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
                update_pipe_status(&runtime_status, |status| {
                    status.process_state = "respawning".to_string();
                    status.pipe_is_open = false;
                    status.respawn_attempts = status.respawn_attempts.saturating_add(1);
                });
            }

            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            tokio::select! {
                _ = context.cancel.cancelled() => break,
                _ = iface_stop.cancelled() => break,
                _ = tokio::time::sleep(respawn_delay) => {}
            }
        }

        let runtime_status = {
            let guard = context.inner.lock().expect("pipe interface mutex poisoned");
            guard.runtime_status.clone()
        };
        update_pipe_status(&runtime_status, |status| {
            status.process_state = "stopped".to_string();
            status.pipe_is_open = false;
        });
        online.store(false, std::sync::atomic::Ordering::Release);
        iface_stop.cancel();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeRuntimeStatus {
    pub command: String,
    pub process_state: String,
    pub pipe_is_open: bool,
    pub respawn_attempts: u64,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct PipeRuntimeStatusHandle {
    inner: Arc<std::sync::Mutex<PipeRuntimeStatus>>,
}

impl PipeRuntimeStatusHandle {
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("pipe runtime status mutex poisoned").to_json()
    }

    pub fn record_error_for_test(&self, state: &str, error: impl Into<String>) {
        update_pipe_status(&self.inner, |status| {
            status.process_state = state.to_string();
            status.pipe_is_open = false;
            status.respawn_attempts = status.respawn_attempts.saturating_add(1);
            status.last_error = Some(error.into());
        });
    }
}

impl PipeRuntimeStatus {
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            process_state: "configured".to_string(),
            pipe_is_open: false,
            respawn_attempts: 0,
            last_error: None,
        }
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "command": self.command,
            "process_state": self.process_state,
            "pipe_is_open": self.pipe_is_open,
            "respawn_attempts": self.respawn_attempts,
            "last_error": self.last_error,
        })
    }
}

impl Interface for PipeInterface {
    fn ifac_default_size_bytes() -> usize {
        8
    }

    fn mtu() -> usize {
        Self::DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_pipe_process(
    command: &str,
    iface_address: AddressHash,
    mtu: usize,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
    runtime_status: Arc<std::sync::Mutex<PipeRuntimeStatus>>,
    online: Arc<std::sync::atomic::AtomicBool>,
    ifac_state: IfacState,
    ifac_violations: Arc<std::sync::atomic::AtomicU64>,
) -> Result<(), String> {
    let argv = PipeInterface::parse_command(command)?;
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn {} failed: {}", argv[0], err))?;

    let stdout = child.stdout.take().ok_or_else(|| "pipe stdout unavailable".to_string())?;
    let stdin = child.stdin.take().ok_or_else(|| "pipe stdin unavailable".to_string())?;
    log::info!("pipe_interface spawned iface={} command={}", iface_address, command);
    update_pipe_status(&runtime_status, |status| {
        status.process_state = "running".to_string();
        status.pipe_is_open = true;
        status.last_error = None;
    });
    online.store(true, std::sync::atomic::Ordering::Release);

    run_pipe_stream(
        stdout,
        stdin,
        iface_address,
        mtu,
        cancel,
        iface_stop,
        rx_channel,
        tx_channel,
        runtime_status,
        ifac_state,
        ifac_violations,
    )
    .await;
    online.store(false, std::sync::atomic::Ordering::Release);

    terminate_pipe_child(&mut child, command).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_pipe_stream<R, W>(
    mut reader: R,
    mut writer: W,
    iface_address: AddressHash,
    mtu: usize,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
    runtime_status: Arc<std::sync::Mutex<PipeRuntimeStatus>>,
    ifac_state: IfacState,
    ifac_violations: Arc<std::sync::atomic::AtomicU64>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let stop = CancellationToken::new();
    let rx_stop = stop.clone();
    let tx_stop = stop.clone();

    let rx_task = {
        let cancel = cancel.clone();
        let iface_stop = iface_stop.clone();
        let runtime_status = runtime_status.clone();
        let ifac_state = ifac_state.clone();
        let ifac_violations = ifac_violations.clone();
        tokio::spawn(async move {
            let mut hdlc_rx_buffer = vec![0_u8; mtu.saturating_add(MAX_IFAC_SIZE_BYTES)];
            let mut frame_buffer = Vec::<u8>::with_capacity(mtu * 4);
            let mut read_buffer = vec![0_u8; mtu.clamp(256, 32_768)];

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = rx_stop.cancelled() => break,
                    result = reader.read(&mut read_buffer[..]) => {
                        match result {
                            Ok(0) => {
                                update_pipe_status(&runtime_status, |status| {
                                    status.process_state = "closed".to_string();
                                    status.pipe_is_open = false;
                                    status.last_error = Some("pipe stdout closed".to_string());
                                });
                                rx_stop.cancel();
                                break;
                            }
                            Ok(n) => {
                                frame_buffer.extend_from_slice(&read_buffer[..n]);
                                while let Some((start, end)) = Hdlc::find(&frame_buffer) {
                                    let frame = &frame_buffer[start..=end];
                                    let mut output = OutputBuffer::new(&mut hdlc_rx_buffer[..]);
                                    if Hdlc::decode(frame, &mut output).is_ok() {
                                        match decode_packet_ifac(&ifac_state, output.as_slice()) {
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
                                                log::warn!(
                                                    "pipe receive queue closed iface={iface_address}"
                                                );
                                                update_pipe_status(&runtime_status, |status| {
                                                    status.process_state = "closed".to_string();
                                                    status.pipe_is_open = false;
                                                    status.last_error = Some(
                                                        "transport receive queue closed".to_string(),
                                                    );
                                                });
                                                rx_stop.cancel();
                                                return;
                                            }
                                            }
                                            Err(error) => {
                                                if is_ifac_violation(&error) {
                                                    record_ifac_violation(&ifac_violations, &error);
                                                }
                                                log::debug!("pipe packet rejected: {error}");
                                            }
                                        }
                                    }
                                    frame_buffer.drain(..=end);
                                }

                                if frame_buffer.len() > mtu * 64 {
                                    frame_buffer.clear();
                                }
                            }
                            Err(err) => {
                                log::warn!("pipe read error iface={} err={}", iface_address, err);
                                update_pipe_status(&runtime_status, |status| {
                                    status.process_state = "read_error".to_string();
                                    status.pipe_is_open = false;
                                    status.last_error = Some(err.to_string());
                                });
                                rx_stop.cancel();
                                break;
                            }
                        }
                    }
                }
            }
        })
    };

    let tx_task = {
        let cancel = cancel.clone();
        let iface_stop = iface_stop.clone();
        let tx_channel = tx_channel.clone();
        let runtime_status = runtime_status.clone();
        let ifac_state = ifac_state.clone();
        tokio::spawn(async move {
            loop {
                if tx_stop.is_cancelled() {
                    break;
                }

                let mut hdlc_tx_buffer = vec![
                    0_u8;
                    mtu.saturating_add(MAX_IFAC_SIZE_BYTES)
                        .saturating_mul(2)
                        .saturating_add(16)
                ];
                let mut tx_channel = tx_channel.lock().await;

                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = tx_stop.cancelled() => break,
                    Some(message) = tx_channel.recv() => {
                        let payload = match encode_packet_ifac(&ifac_state, &message.packet) {
                            Ok(payload) => payload,
                            Err(err) => {
                                log::warn!(
                                    "pipe packet encode failed iface={} err={:?}",
                                    iface_address,
                                    err
                                );
                                continue;
                            }
                        };
                        let mut hdlc_output = OutputBuffer::new(&mut hdlc_tx_buffer[..]);
                        if Hdlc::encode(&payload, &mut hdlc_output).is_ok() {
                            if let Err(err) = writer.write_all(hdlc_output.as_slice()).await {
                                log::warn!("pipe write error iface={} err={}", iface_address, err);
                                update_pipe_status(&runtime_status, |status| {
                                    status.process_state = "write_error".to_string();
                                    status.pipe_is_open = false;
                                    status.last_error = Some(err.to_string());
                                });
                                tx_stop.cancel();
                                break;
                            }
                            if let Err(err) = writer.flush().await {
                                log::warn!("pipe flush error iface={} err={}", iface_address, err);
                                update_pipe_status(&runtime_status, |status| {
                                    status.process_state = "write_error".to_string();
                                    status.pipe_is_open = false;
                                    status.last_error = Some(err.to_string());
                                });
                                tx_stop.cancel();
                                break;
                            }
                        }
                    }
                }
            }
        })
    };

    if let Err(error) = rx_task.await {
        log::error!("pipe receive task failed iface={iface_address}: {error}");
        update_pipe_status(&runtime_status, |status| {
            status.process_state = "receive_task_failed".to_string();
            status.pipe_is_open = false;
            status.last_error = Some(error.to_string());
        });
    }
    stop.cancel();
    if let Err(error) = tx_task.await {
        log::error!("pipe transmit task failed iface={iface_address}: {error}");
        update_pipe_status(&runtime_status, |status| {
            status.process_state = "transmit_task_failed".to_string();
            status.pipe_is_open = false;
            status.last_error = Some(error.to_string());
        });
    }
}

pub fn spawn_pipe(
    mgr: &mut InterfaceManager,
    command: String,
    respawn_delay: Duration,
    mtu: usize,
) -> AddressHash {
    let iface = PipeInterface::new(command).with_respawn_delay(respawn_delay).with_mtu(mtu);
    mgr.spawn_as(iface, PipeInterface::spawn, IfaceRole::Unicast)
}

#[cfg(test)]
#[path = "pipe_tests.rs"]
mod tests;

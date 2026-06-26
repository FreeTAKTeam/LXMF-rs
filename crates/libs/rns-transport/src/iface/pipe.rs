use alloc::string::String;
use alloc::vec::Vec;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::hash::AddressHash;
use crate::iface::{hdlc::Hdlc, IfaceRole, IfaceSource, InterfaceManager, RxMessage};
use crate::packet::Packet;
use crate::serde::Serialize;

use super::{Interface, InterfaceContext, TxMessage};

pub struct PipeInterface {
    command: String,
    respawn_delay: Duration,
    mtu: usize,
}

impl PipeInterface {
    pub const DEFAULT_MTU: usize = 1_064;
    pub const DEFAULT_RESPAWN_DELAY: Duration = Duration::from_secs(5);

    pub fn new<T: Into<String>>(command: T) -> Self {
        Self {
            command: command.into(),
            respawn_delay: Self::DEFAULT_RESPAWN_DELAY,
            mtu: Self::DEFAULT_MTU,
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

            if let Err(err) = run_pipe_process(
                command.as_str(),
                iface_address,
                mtu,
                context.cancel.clone(),
                iface_stop.clone(),
                rx_channel.clone(),
                tx_channel.clone(),
            )
            .await
            {
                log::warn!("pipe_interface command failed iface={} err={}", iface_address, err);
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

        iface_stop.cancel();
    }
}

impl Interface for PipeInterface {
    fn mtu() -> usize {
        Self::DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

async fn run_pipe_process(
    command: &str,
    iface_address: AddressHash,
    mtu: usize,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
) -> Result<(), String> {
    let argv = PipeInterface::parse_command(command)?;
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn {} failed: {}", argv[0], err))?;

    let stdout = child.stdout.take().ok_or_else(|| "pipe stdout unavailable".to_string())?;
    let stdin = child.stdin.take().ok_or_else(|| "pipe stdin unavailable".to_string())?;
    log::info!("pipe_interface spawned iface={} command={}", iface_address, command);

    run_pipe_stream(stdout, stdin, iface_address, mtu, cancel, iface_stop, rx_channel, tx_channel)
        .await;

    let _ = child.kill().await;
    let _ = child.wait().await;
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
        tokio::spawn(async move {
            let mut hdlc_rx_buffer = vec![0_u8; mtu];
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
                                rx_stop.cancel();
                                break;
                            }
                            Ok(n) => {
                                frame_buffer.extend_from_slice(&read_buffer[..n]);
                                while let Some((start, end)) = Hdlc::find(&frame_buffer) {
                                    let frame = &frame_buffer[start..=end];
                                    let mut output = OutputBuffer::new(&mut hdlc_rx_buffer[..]);
                                    if Hdlc::decode(frame, &mut output).is_ok() {
                                        if let Ok(packet) =
                                            Packet::deserialize(&mut InputBuffer::new(output.as_slice()))
                                        {
                                            let _ = rx_channel
                                                .send(RxMessage {
                                                    address: iface_address,
                                                    packet,
                                                    source: IfaceSource::None,
                                                })
                                                .await;
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
        tokio::spawn(async move {
            loop {
                if tx_stop.is_cancelled() {
                    break;
                }

                let mut hdlc_tx_buffer = vec![0_u8; mtu.saturating_mul(2).saturating_add(16)];
                let mut tx_buffer = vec![0_u8; mtu];
                let mut tx_channel = tx_channel.lock().await;

                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = tx_stop.cancelled() => break,
                    Some(message) = tx_channel.recv() => {
                        let mut output = OutputBuffer::new(&mut tx_buffer[..]);
                        if message.packet.serialize(&mut output).is_ok() {
                            let mut hdlc_output = OutputBuffer::new(&mut hdlc_tx_buffer[..]);
                            if Hdlc::encode(output.as_slice(), &mut hdlc_output).is_ok() {
                                if let Err(err) = writer.write_all(hdlc_output.as_slice()).await {
                                    log::warn!("pipe write error iface={} err={}", iface_address, err);
                                    tx_stop.cancel();
                                    break;
                                }
                                if let Err(err) = writer.flush().await {
                                    log::warn!("pipe flush error iface={} err={}", iface_address, err);
                                    tx_stop.cancel();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        })
    };

    let _ = rx_task.await;
    stop.cancel();
    let _ = tx_task.await;
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
mod tests {
    use super::PipeInterface;
    use std::time::Duration;

    #[test]
    fn pipe_command_parser_matches_python_shlex_baseline() {
        let argv = PipeInterface::parse_command("prog --flag 'two words'").expect("parse");
        assert_eq!(argv, vec!["prog", "--flag", "two words"]);
        assert!(PipeInterface::parse_command("'unterminated").is_err());
        assert!(PipeInterface::parse_command("").is_err());
    }

    #[test]
    fn pipe_builder_exposes_defaults_and_overrides() {
        let adapter =
            PipeInterface::new("cat").with_respawn_delay(Duration::from_millis(250)).with_mtu(512);
        assert_eq!(adapter.command(), "cat");
        assert_eq!(adapter.mtu_value(), 512);
    }
}

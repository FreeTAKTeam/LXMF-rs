use std::sync::Arc;
use std::sync::OnceLock;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::error::RnsError;
use crate::iface::{IfaceSource, RxMessage};
use crate::packet::Packet;
use crate::serde::Serialize;

use tokio::io::AsyncReadExt;

use alloc::string::String;

use super::hdlc::Hdlc;
use super::{Interface, InterfaceContext};

// TCP packet tracing is kept off by default and gated by diagnostics env flags.
const PACKET_TRACE: bool = false;

fn tx_diag_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("RETICULUMD_DIAGNOSTICS")
            .or_else(|_| std::env::var("RETICULUM_TRANSPORT_DIAGNOSTICS"))
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on" | "debug"
                )
            })
            .unwrap_or(false)
    })
}

fn tcp_wire_buffer_capacity(mtu: usize) -> usize {
    // Worst-case HDLC expansion doubles bytes (all escaped) plus frame delimiters.
    mtu.saturating_mul(2).saturating_add(16)
}

pub struct TcpClient {
    addr: String,
    stream: Option<TcpStream>,
    mtu: usize,
}

impl TcpClient {
    pub const DEFAULT_MTU: usize = 2048;

    pub fn new<T: Into<String>>(addr: T) -> Self {
        Self { addr: addr.into(), stream: None, mtu: Self::DEFAULT_MTU }
    }

    pub fn new_from_stream<T: Into<String>>(addr: T, stream: TcpStream) -> Self {
        Self { addr: addr.into(), stream: Some(stream), mtu: Self::DEFAULT_MTU }
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(256);
        self
    }

    #[must_use]
    pub fn addr(&self) -> &str {
        &self.addr
    }

    #[must_use]
    pub fn mtu_value(&self) -> usize {
        self.mtu
    }

    #[tracing::instrument(name = "tcp_peer", skip_all, fields(addr = tracing::field::Empty))]
    pub async fn spawn(context: InterfaceContext<TcpClient>) {
        let iface_stop = context.channel.stop.clone();
        let (addr, mtu) = {
            let guard = context.inner.lock().unwrap();
            (guard.addr.clone(), guard.mtu)
        };
        tracing::Span::current().record("addr", addr.as_str());
        let iface_address = context.channel.address;
        let mut stream = { context.inner.lock().unwrap().stream.take() };

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        let mut running = true;
        loop {
            if !running || context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            let stream = {
                match stream.take() {
                    Some(stream) => {
                        running = false;
                        Ok(stream)
                    }
                    None => TcpStream::connect(addr.clone())
                        .await
                        .map_err(|_| RnsError::ConnectionError),
                }
            };

            if stream.is_err() {
                log::warn!("couldn't connect to <{}>", addr);
                tokio::select! {
                    _ = context.cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                }
                continue;
            }

            let cancel = context.cancel.clone();
            let stop = CancellationToken::new();
            let iface_stop_rx = iface_stop.clone();
            let iface_stop_tx = iface_stop.clone();

            let stream = stream.unwrap();
            let (read_stream, write_stream) = stream.into_split();

            log::info!("connected to <{}>", addr);

            // Use protocol MTU-scale buffers, not size_of::<Packet>(), since packet
            // struct size does not reflect serialized wire size and can silently drop
            // larger payloads during serialization.
            // Start receive task
            let rx_task = {
                let cancel = cancel.clone();
                let stop = stop.clone();
                let mut stream = read_stream;
                let rx_channel = rx_channel.clone();

                tokio::spawn(async move {
                    let mut hdlc_rx_buffer = vec![0u8; mtu];
                    let mut frame_buffer: Vec<u8> = Vec::with_capacity(mtu.saturating_mul(4));
                    let mut tcp_buffer = vec![0u8; mtu.saturating_mul(16)];

                    loop {
                        tokio::select! {
                            _ = cancel.cancelled() => {
                                    break;
                            }
                            _ = iface_stop_rx.cancelled() => {
                                    stop.cancel();
                                    break;
                            }
                            _ = stop.cancelled() => {
                                    break;
                            }
                            result = stream.read(&mut tcp_buffer[..]) => {
                                    match result {
                                        Ok(0) => {
                                            log::warn!("connection closed");
                                            stop.cancel();
                                            break;
                                        }
                                        Ok(n) => {
                                            // TCP can deliver partial or multiple HDLC frames.
                                            frame_buffer.extend_from_slice(&tcp_buffer[..n]);

                                            while let Some((start, end)) = Hdlc::find(&frame_buffer) {
                                                let frame = &frame_buffer[start..=end];
                                                let mut output = OutputBuffer::new(&mut hdlc_rx_buffer[..]);
                                                if Hdlc::decode(frame, &mut output).is_ok() {
                                                    if let Ok(packet) =
                                                        Packet::deserialize(&mut InputBuffer::new(output.as_slice()))
                                                    {
                                                        if PACKET_TRACE {
                                                            log::trace!("rx << ({}) {}", iface_address, packet);
                                                        }
                                                        if tx_diag_enabled() {
                                                            log::debug!(
                                                                "[tp-diag] tcp_client rx_packet iface={} type={:?} dst={} ctx={:02x} hops={}",
                                                                iface_address,
                                                                packet.header.packet_type,
                                                                packet.destination,
                                                                packet.context as u8,
                                                                packet.header.hops
                                                            );
                                                        }
                                                        let _ = rx_channel
                                                            .send(RxMessage {
                                                                address: iface_address,
                                                                packet,
                                                                source: IfaceSource::None,
                                                            })
                                                            .await;
                                                    } else {
                                                        log::warn!("couldn't decode packet");
                                                    }
                                                } else {
                                                    log::warn!("couldn't decode hdlc frame");
                                                }

                                                // Drop all bytes up to and including the closing
                                                // flag of the frame we just handled.
                                                frame_buffer.drain(..=end);
                                            }

                                            if frame_buffer.len() > mtu.saturating_mul(64) {
                                                // Guard against unbounded growth on malformed
                                                // streams where no valid frame closes.
                                                frame_buffer.clear();
                                            }
                                        }
                                        Err(e) => {
                                            log::warn!("connection error {}", e);
                                            break;
                                        }
                                    }
                                },
                        };
                    }
                })
            };

            // Start transmit task
            let tx_task = {
                let cancel = cancel.clone();
                let tx_channel = tx_channel.clone();
                let mut stream = write_stream;

                tokio::spawn(async move {
                    loop {
                        if stop.is_cancelled() {
                            break;
                        }

                        let mut hdlc_tx_buffer = vec![0u8; tcp_wire_buffer_capacity(mtu)];
                        let mut tx_buffer = vec![0u8; mtu];

                        let mut tx_channel = tx_channel.lock().await;

                        tokio::select! {
                            _ = cancel.cancelled() => {
                                    break;
                            }
                            _ = iface_stop_tx.cancelled() => {
                                    stop.cancel();
                                    break;
                            }
                            _ = stop.cancelled() => {
                                    break;
                            }
                            Some(message) = tx_channel.recv() => {
                                let packet = message.packet;
                                if PACKET_TRACE {
                                    log::trace!("tx >> ({}) {}", iface_address, packet);
                                }
                                if tx_diag_enabled() {
                                    log::debug!("[tp-diag] tcp_client tx_dequeue iface={} {}", iface_address, packet);
                                }
                                let mut output = OutputBuffer::new(&mut tx_buffer);
                                if packet.serialize(&mut output).is_ok() {
                                    let mut hdlc_output = OutputBuffer::new(&mut hdlc_tx_buffer[..]);
                                    if Hdlc::encode(output.as_slice(), &mut hdlc_output).is_ok() {
                                        if let Err(err) = stream.write_all(hdlc_output.as_slice()).await {
                                            log::warn!("[tp-diag] write_all failed iface={} err={}", iface_address, err);
                                            stop.cancel();
                                            break;
                                        }
                                        if let Err(err) = stream.flush().await {
                                            log::warn!("[tp-diag] flush failed iface={} err={}", iface_address, err);
                                            stop.cancel();
                                            break;
                                        }
                                        if tx_diag_enabled() {
                                            log::debug!(
                                                "[tp-diag] tcp_client tx_write_ok iface={} wire_len={} raw_len={}",
                                                iface_address,
                                                hdlc_output.as_slice().len(),
                                                output.as_slice().len()
                                            );
                                        }
                                    } else {
                                        log::warn!(
                                            "[tp-diag] hdlc_encode failed iface={} raw_len={}",
                                            iface_address,
                                            output.as_slice().len()
                                        );
                                    }
                                } else {
                                    log::warn!(
                                        "[tp-diag] serialize failed iface={} buffer_cap={}",
                                        iface_address,
                                        tx_buffer.len()
                                    );
                                }
                            }
                        };
                    }
                })
            };

            tx_task.await.unwrap();
            rx_task.await.unwrap();

            log::info!("disconnected from <{}>", addr);
        }

        iface_stop.cancel();
    }
}

impl Interface for TcpClient {
    fn mtu() -> usize {
        TcpClient::DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

#[cfg(test)]
mod tests {
    use super::{tcp_wire_buffer_capacity, TcpClient};
    use crate::buffer::OutputBuffer;
    use crate::iface::hdlc::Hdlc;

    #[test]
    fn tcp_client_default_and_configured_mtu_are_exposed() {
        assert_eq!(TcpClient::new("rmap.world:4242").mtu_value(), TcpClient::DEFAULT_MTU);
        assert_eq!(TcpClient::new("rmap.world:4242").with_mtu(4096).mtu_value(), 4096);
        assert_eq!(TcpClient::new("rmap.world:4242").with_mtu(64).mtu_value(), 256);
    }

    #[test]
    fn tcp_wire_capacity_handles_worst_case_hdlc_escape_expansion() {
        let mtu = 512;
        let raw = vec![0x7e_u8; mtu];
        let mut wire = vec![0_u8; tcp_wire_buffer_capacity(mtu)];
        let mut output = OutputBuffer::new(&mut wire[..]);

        let encoded_len = Hdlc::encode(&raw, &mut output).expect("encode worst-case payload");
        assert!(encoded_len >= (mtu * 2) + 2, "wire len must cover escaped payload plus flags");
    }
}

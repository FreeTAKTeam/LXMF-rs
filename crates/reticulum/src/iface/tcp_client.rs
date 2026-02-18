use std::sync::Arc;
use std::sync::OnceLock;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::error::RnsError;
use crate::iface::RxMessage;
use crate::packet::Packet;
use crate::serde::Serialize;

use tokio::io::AsyncReadExt;

use alloc::string::String;

use super::hdlc::Hdlc;
use super::{Interface, InterfaceContext};

// TODO: Configure via features
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

pub struct TcpClient {
    addr: String,
    stream: Option<TcpStream>,
}

impl TcpClient {
    pub fn new<T: Into<String>>(addr: T) -> Self {
        Self { addr: addr.into(), stream: None }
    }

    pub fn new_from_stream<T: Into<String>>(addr: T, stream: TcpStream) -> Self {
        Self { addr: addr.into(), stream: Some(stream) }
    }

    pub async fn spawn(context: InterfaceContext<TcpClient>) {
        let iface_stop = context.channel.stop.clone();
        let addr = { context.inner.lock().unwrap().addr.clone() };
        let iface_address = context.channel.address;
        let mut stream = { context.inner.lock().unwrap().stream.take() };

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        let mut running = true;
        loop {
            if !running || context.cancel.is_cancelled() {
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
                log::info!("tcp_client: couldn't connect to <{}>", addr);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            let cancel = context.cancel.clone();
            let stop = CancellationToken::new();

            let stream = stream.unwrap();
            let (read_stream, write_stream) = stream.into_split();

            log::info!("tcp_client connected to <{}>", addr);

            // Use protocol MTU-scale buffers, not size_of::<Packet>(), since packet
            // struct size does not reflect serialized wire size and can silently drop
            // larger payloads during serialization.
            const BUFFER_SIZE: usize = 2048;

            // Start receive task
            let rx_task = {
                let cancel = cancel.clone();
                let stop = stop.clone();
                let mut stream = read_stream;
                let rx_channel = rx_channel.clone();

                tokio::spawn(async move {
                    let mut hdlc_rx_buffer = [0u8; BUFFER_SIZE];
                    let mut frame_buffer: Vec<u8> = Vec::with_capacity(BUFFER_SIZE * 4);
                    let mut tcp_buffer = [0u8; (BUFFER_SIZE * 16)];

                    loop {
                        tokio::select! {
                            _ = cancel.cancelled() => {
                                    break;
                            }
                            _ = stop.cancelled() => {
                                    break;
                            }
                            result = stream.read(&mut tcp_buffer[..]) => {
                                    match result {
                                        Ok(0) => {
                                            log::warn!("tcp_client: connection closed");
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
                                                            log::trace!("tcp_client: rx << ({}) {}", iface_address, packet);
                                                        }
                                                        if tx_diag_enabled() {
                                                            eprintln!(
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
                                                            })
                                                            .await;
                                                    } else {
                                                        log::warn!("tcp_client: couldn't decode packet");
                                                    }
                                                } else {
                                                    log::warn!("tcp_client: couldn't decode hdlc frame");
                                                }

                                                // Drop all bytes up to and including the closing
                                                // flag of the frame we just handled.
                                                frame_buffer.drain(..=end);
                                            }

                                            if frame_buffer.len() > BUFFER_SIZE * 64 {
                                                // Guard against unbounded growth on malformed
                                                // streams where no valid frame closes.
                                                frame_buffer.clear();
                                            }
                                        }
                                        Err(e) => {
                                            log::warn!("tcp_client: connection error {}", e);
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

                        let mut hdlc_tx_buffer = [0u8; BUFFER_SIZE];
                        let mut tx_buffer = [0u8; BUFFER_SIZE];

                        let mut tx_channel = tx_channel.lock().await;

                        tokio::select! {
                            _ = cancel.cancelled() => {
                                    break;
                            }
                            _ = stop.cancelled() => {
                                    break;
                            }
                            Some(message) = tx_channel.recv() => {
                                let packet = message.packet;
                                if PACKET_TRACE {
                                    log::trace!("tcp_client: tx >> ({}) {}", iface_address, packet);
                                }
                                if tx_diag_enabled() {
                                    eprintln!("[tp-diag] tcp_client tx_dequeue iface={} {}", iface_address, packet);
                                    log::info!("[tp-diag] tcp_client tx_dequeue iface={} {}", iface_address, packet);
                                }
                                let mut output = OutputBuffer::new(&mut tx_buffer);
                                if packet.serialize(&mut output).is_ok() {
                                    let mut hdlc_output = OutputBuffer::new(&mut hdlc_tx_buffer[..]);
                                    if Hdlc::encode(output.as_slice(), &mut hdlc_output).is_ok() {
                                        if let Err(err) = stream.write_all(hdlc_output.as_slice()).await {
                                            log::warn!("tcp_client: write_all failed on {}: {}", iface_address, err);
                                            eprintln!(
                                                "[tp-diag] tcp_client write_all failed iface={} err={}",
                                                iface_address, err
                                            );
                                            stop.cancel();
                                            break;
                                        }
                                        if let Err(err) = stream.flush().await {
                                            log::warn!("tcp_client: flush failed on {}: {}", iface_address, err);
                                            eprintln!(
                                                "[tp-diag] tcp_client flush failed iface={} err={}",
                                                iface_address, err
                                            );
                                            stop.cancel();
                                            break;
                                        }
                                        if tx_diag_enabled() {
                                            eprintln!(
                                                "[tp-diag] tcp_client tx_write_ok iface={} wire_len={} raw_len={}",
                                                iface_address,
                                                hdlc_output.as_slice().len(),
                                                output.as_slice().len()
                                            );
                                            log::info!(
                                                "[tp-diag] tcp_client tx_write_ok iface={} wire_len={} raw_len={}",
                                                iface_address,
                                                hdlc_output.as_slice().len(),
                                                output.as_slice().len()
                                            );
                                        }
                                    } else {
                                        log::warn!(
                                            "tcp_client: failed to HDLC-encode packet on {} (raw_len={})",
                                            iface_address,
                                            output.as_slice().len()
                                        );
                                        eprintln!(
                                            "[tp-diag] tcp_client hdlc_encode failed iface={} raw_len={}",
                                            iface_address,
                                            output.as_slice().len()
                                        );
                                    }
                                } else {
                                    log::warn!(
                                        "tcp_client: failed to serialize packet on {} (buffer_cap={})",
                                        iface_address,
                                        tx_buffer.len()
                                    );
                                    eprintln!(
                                        "[tp-diag] tcp_client serialize failed iface={} buffer_cap={}",
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

            log::info!("tcp_client: disconnected from <{}>", addr);
        }

        iface_stop.cancel();
    }
}

impl Interface for TcpClient {
    fn mtu() -> usize {
        2048
    }
}

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::error::RnsError;
use crate::iface::RxMessage;
use crate::packet::Packet;
use crate::serde::Serialize;

use super::{Interface, InterfaceContext};

fn bind_udp(bind_addr: &str) -> std::io::Result<UdpSocket> {
    let parsed: SocketAddr = bind_addr
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("bad bind address {}: {}", bind_addr, e)))?;

    let domain = if parsed.is_ipv6() { Domain::IPV6 } else { Domain::IPV4 };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;

    // Allow multiple nodes (or restart of the same node) to bind the same port.
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;

    // If binding to a multicast group directly, bind to the unspecified address on
    // the same port instead; then join the group. This works cross-platform.
    let (bound_addr, multicast_group) = match parsed.ip() {
        IpAddr::V6(ip) if ip.is_multicast() => {
            let any: SocketAddr = (std::net::Ipv6Addr::UNSPECIFIED, parsed.port()).into();
            (any, Some(IpAddr::V6(ip)))
        }
        IpAddr::V4(ip) if ip.is_multicast() => {
            let any: SocketAddr = (std::net::Ipv4Addr::UNSPECIFIED, parsed.port()).into();
            (any, Some(IpAddr::V4(ip)))
        }
        _ => (parsed, None),
    };

    socket.bind(&bound_addr.into())?;

    if let Some(group) = multicast_group {
        match group {
            IpAddr::V6(g) => socket.join_multicast_v6(&g, 0)?,
            IpAddr::V4(g) => {
                socket.join_multicast_v4(&g, &std::net::Ipv4Addr::UNSPECIFIED)?
            }
        }
    }

    socket.set_nonblocking(true)?;
    let std_socket: std::net::UdpSocket = socket.into();
    UdpSocket::from_std(std_socket)
}

// UDP trace logging stays on by default for packet-level network bring-up visibility.
const PACKET_TRACE: bool = true;

pub struct UdpInterface {
    bind_addr: String,
    forward_addr: Option<String>,
}

impl UdpInterface {
    pub fn new<T: Into<String>>(bind_addr: T, forward_addr: Option<T>) -> Self {
        Self { bind_addr: bind_addr.into(), forward_addr: forward_addr.map(Into::into) }
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let bind_addr = { context.inner.lock().unwrap().bind_addr.clone() };
        let forward_addr = { context.inner.lock().unwrap().forward_addr.clone() };
        let iface_address = context.channel.address;

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            let socket = bind_udp(&bind_addr).map_err(|_| RnsError::ConnectionError);

            if socket.is_err() {
                log::info!("udp_interface: couldn't bind to <{}>", bind_addr);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            let cancel = context.cancel.clone();
            let stop = CancellationToken::new();

            let socket = socket.unwrap();
            let read_socket = Arc::new(socket);
            let write_socket = read_socket.clone();

            log::info!("udp_interface bound to <{}>", bind_addr);

            const BUFFER_SIZE: usize = core::mem::size_of::<Packet>() * 3;

            // Start receive task
            let rx_task = {
                let cancel = cancel.clone();
                let stop = stop.clone();
                let socket = read_socket;
                let rx_channel = rx_channel.clone();

                tokio::spawn(async move {
                    loop {
                        let mut rx_buffer = [0u8; BUFFER_SIZE];

                        tokio::select! {
                            _ = cancel.cancelled() => {
                                    break;
                            }
                            _ = stop.cancelled() => {
                                    break;
                            }
                            result = socket.recv_from(&mut rx_buffer) => {
                                match result {
                                    Ok((0, _)) => {
                                        log::warn!("udp_interface: connection closed");
                                        stop.cancel();
                                        break;
                                    }
                                    Ok((n, _in_addr)) => {
                                        if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(&rx_buffer[..n])) {
                                            if PACKET_TRACE {
                                                log::trace!("udp_interface: rx << ({}) {}", iface_address, packet);
                                            }
                                            let _ = rx_channel.send(RxMessage { address: iface_address, packet }).await;
                                        } else {
                                            log::warn!("udp_interface: couldn't decode packet");
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!("udp_interface: connection error {}", e);
                                        break;
                                    }
                                }
                            },
                        };
                    }
                })
            };

            if let Some(forward_addr) = forward_addr.clone() {
                // Start transmit task
                let tx_task = {
                    let cancel = cancel.clone();
                    let tx_channel = tx_channel.clone();
                    let socket = write_socket;

                    tokio::spawn(async move {
                        loop {
                            if stop.is_cancelled() {
                                break;
                            }

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
                                        log::trace!("udp_interface: tx >> ({}) {}", iface_address, packet);
                                    }
                                    let mut output = OutputBuffer::new(&mut tx_buffer);
                                    if packet.serialize(&mut output).is_ok() {
                                        let _ = socket.send_to(output.as_slice(), &forward_addr).await;
                                    }
                                }
                            };
                        }
                    })
                };
                tx_task.await.unwrap();
            }

            rx_task.await.unwrap();

            log::info!("udp_interface <{}>: closed", bind_addr);
        }
    }
}

impl Interface for UdpInterface {
    fn mtu() -> usize {
        2048
    }
}

pub fn encode_frame(data: &[u8]) -> Result<Vec<u8>, RnsError> {
    Ok(data.to_vec())
}

pub fn decode_frame(frame: &[u8]) -> Result<Vec<u8>, RnsError> {
    Ok(frame.to_vec())
}

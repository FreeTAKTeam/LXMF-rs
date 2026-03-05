use rns_embedded_core::{
    EmbeddedError, EmbeddedResult,
    packet::{PacketFrame, decode_frame, encode_frame},
    transport::{EmbeddedTransport, LinkState, TransportCaps},
};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};

const HEADER_LEN: usize = 14;

pub struct TcpEmbeddedTransport {
    stream: TcpStream,
    state: LinkState,
    caps: TransportCaps,
    recv_buf: Vec<u8>,
}

impl TcpEmbeddedTransport {
    pub fn connect(addr: SocketAddr, mtu_hint: u16) -> EmbeddedResult<Self> {
        let stream = TcpStream::connect(addr).map_err(map_connect_error)?;
        Self::from_stream(stream, mtu_hint)
    }

    pub fn from_stream(stream: TcpStream, mtu_hint: u16) -> EmbeddedResult<Self> {
        stream
            .set_nonblocking(true)
            .map_err(|_| EmbeddedError::InvalidState)?;
        Ok(Self {
            stream,
            state: LinkState::Up,
            caps: TransportCaps {
                mtu_hint,
                ordered_delivery: true,
            },
            recv_buf: Vec::new(),
        })
    }

    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.stream.peer_addr()
    }

    fn next_frame_len(&self) -> EmbeddedResult<Option<usize>> {
        if self.recv_buf.is_empty() {
            return Ok(None);
        }
        if self.recv_buf.len() < HEADER_LEN {
            return Ok(None);
        }
        let payload_len = u32::from_le_bytes([
            self.recv_buf[10],
            self.recv_buf[11],
            self.recv_buf[12],
            self.recv_buf[13],
        ]);
        let payload_len = usize::try_from(payload_len).map_err(|_| EmbeddedError::InvalidInput)?;
        Ok(Some(HEADER_LEN + payload_len))
    }

    fn refill_read_buffer(&mut self) -> EmbeddedResult<()> {
        let mut scratch = [0_u8; 2048];
        loop {
            match self.stream.read(&mut scratch) {
                Ok(0) => {
                    self.state = LinkState::Down;
                    return Err(EmbeddedError::Disconnected);
                }
                Ok(n) => {
                    self.recv_buf.extend_from_slice(&scratch[..n]);
                    if n < scratch.len() {
                        return Ok(());
                    }
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(_) => {
                    self.state = LinkState::Down;
                    return Err(EmbeddedError::Disconnected);
                }
            }
        }
    }
}

impl EmbeddedTransport for TcpEmbeddedTransport {
    fn link_state(&self) -> LinkState {
        self.state
    }

    fn capabilities(&self) -> TransportCaps {
        self.caps
    }

    fn send_frame(&mut self, frame: &PacketFrame) -> EmbeddedResult<()> {
        if self.state != LinkState::Up {
            return Err(EmbeddedError::Disconnected);
        }
        if frame.payload.len() > usize::from(self.caps.mtu_hint) {
            return Err(EmbeddedError::InvalidArgument);
        }
        let encoded = encode_frame(frame)?;
        self.stream
            .write_all(&encoded)
            .map_err(|_| EmbeddedError::Disconnected)?;
        self.stream.flush().map_err(|_| EmbeddedError::Disconnected)?;
        Ok(())
    }

    fn poll_frame(&mut self) -> EmbeddedResult<Option<PacketFrame>> {
        if self.state == LinkState::Down {
            return Err(EmbeddedError::Disconnected);
        }
        self.refill_read_buffer()?;
        let Some(frame_len) = self.next_frame_len()? else {
            return Ok(None);
        };
        if self.recv_buf.len() < frame_len {
            return Ok(None);
        }

        let frame_bytes: Vec<u8> = self.recv_buf.drain(..frame_len).collect();
        let frame = decode_frame(&frame_bytes)?;
        Ok(Some(frame))
    }
}

fn map_connect_error(err: std::io::Error) -> EmbeddedError {
    match err.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => EmbeddedError::Timeout,
        ErrorKind::ConnectionRefused
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::NotConnected
        | ErrorKind::AddrNotAvailable
        | ErrorKind::BrokenPipe => EmbeddedError::Disconnected,
        _ => EmbeddedError::InvalidState,
    }
}

#[cfg(test)]
mod tests {
    use super::TcpEmbeddedTransport;
    use rns_embedded_core::{
        packet::PacketFrame,
        transport::{EmbeddedTransport, LinkState},
    };
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    fn frame(kind: u8, seq: u32, payload: &[u8]) -> PacketFrame {
        PacketFrame::new(kind, seq, payload.to_vec()).expect("frame")
    }

    fn connected_pair() -> (TcpEmbeddedTransport, TcpEmbeddedTransport) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            TcpEmbeddedTransport::from_stream(stream, 1024).expect("server transport")
        });

        let client = TcpEmbeddedTransport::connect(addr, 1024).expect("client transport");
        let server = server.join().expect("server join");
        (client, server)
    }

    fn poll_until_frame(
        transport: &mut TcpEmbeddedTransport,
        timeout: Duration,
    ) -> PacketFrame {
        let deadline = Instant::now() + timeout;
        loop {
            match transport.poll_frame() {
                Ok(Some(frame)) => return frame,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                Ok(None) => panic!("timed out waiting for frame"),
                Err(err) => panic!("poll failed: {err:?}"),
            }
        }
    }

    #[test]
    fn tcp_transport_round_trips_frame() {
        let (mut client, mut server) = connected_pair();
        client.send_frame(&frame(0x11, 7, b"announce")).expect("send");

        let received = poll_until_frame(&mut server, Duration::from_secs(1));
        assert_eq!(received.kind, 0x11);
        assert_eq!(received.sequence, 7);
        assert_eq!(received.payload, b"announce");
    }

    #[test]
    fn tcp_transport_supports_bidirectional_exchange() {
        let (mut client, mut server) = connected_pair();
        client.send_frame(&frame(0x31, 1, b"hello")).expect("send client");
        server.send_frame(&frame(0x32, 2, b"world")).expect("send server");

        let rx_server = poll_until_frame(&mut server, Duration::from_secs(1));
        let rx_client = poll_until_frame(&mut client, Duration::from_secs(1));
        assert_eq!(rx_server.payload, b"hello");
        assert_eq!(rx_client.payload, b"world");
    }

    #[test]
    fn tcp_transport_tracks_disconnect() {
        let (client, mut server) = connected_pair();
        drop(client);

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match server.poll_frame() {
                Err(rns_embedded_core::EmbeddedError::Disconnected) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                other => panic!("unexpected poll result: {other:?}"),
            }
        }
        assert_eq!(server.link_state(), LinkState::Down);
    }
}

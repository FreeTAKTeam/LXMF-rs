use std::future::Future;

use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

use super::worker_boundary::{
    decode_worker_frame, encode_worker_frame, read_worker_frame, write_worker_frame,
    WorkerCodecError,
};
use crate::hash::{AddressHash, ADDRESS_HASH_SIZE};
use crate::iface::{
    IfaceSource, InterfaceRxSender, InterfaceTxReceiver, RxMessage, TxMessage, TxMessageType,
};
use crate::packet::Packet;

pub const INTERFACE_WORKER_PROTOCOL_VERSION: u16 = 1;
pub const MAX_INTERFACE_WORKER_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceWorkerEnvelope {
    pub protocol_version: u16,
    pub sequence: u64,
    pub event: InterfaceWorkerEvent,
}

impl InterfaceWorkerEnvelope {
    pub fn new(sequence: u64, event: InterfaceWorkerEvent) -> Self {
        Self { protocol_version: INTERFACE_WORKER_PROTOCOL_VERSION, sequence, event }
    }

    pub fn inbound_from_rx_message(
        sequence: u64,
        message: &RxMessage,
    ) -> Result<Self, WorkerCodecError> {
        Ok(Self::new(sequence, InterfaceWorkerEvent::inbound_from_rx_message(message)?))
    }

    pub fn outbound_from_tx_message(
        sequence: u64,
        message: &TxMessage,
    ) -> Result<Self, WorkerCodecError> {
        Ok(Self::new(sequence, InterfaceWorkerEvent::outbound_from_tx_message(message)?))
    }

    pub fn encode(&self) -> Result<Vec<u8>, WorkerCodecError> {
        validate_interface_protocol_version(self.protocol_version)?;
        let encoded = rmp_serde::to_vec_named(self)
            .map_err(|err| WorkerCodecError::Encode { message: err.to_string() })?;
        validate_interface_event_size(encoded.len())?;
        Ok(encoded)
    }

    pub fn decode(data: &[u8]) -> Result<Self, WorkerCodecError> {
        validate_interface_event_size(data.len())?;
        let envelope: Self = rmp_serde::from_slice(data)
            .map_err(|err| WorkerCodecError::Decode { message: err.to_string() })?;
        validate_interface_protocol_version(envelope.protocol_version)?;
        Ok(envelope)
    }

    pub fn encode_frame(&self) -> Result<Vec<u8>, WorkerCodecError> {
        encode_worker_frame(&self.encode()?, MAX_INTERFACE_WORKER_EVENT_BYTES)
    }

    pub fn decode_frame(frame: &[u8]) -> Result<Self, WorkerCodecError> {
        let payload = decode_worker_frame(frame, MAX_INTERFACE_WORKER_EVENT_BYTES)?;
        Self::decode(payload)
    }
}

pub async fn write_interface_worker_envelope<W>(
    writer: &mut W,
    envelope: &InterfaceWorkerEnvelope,
) -> Result<(), WorkerCodecError>
where
    W: AsyncWrite + Unpin,
{
    write_worker_frame(writer, &envelope.encode()?, MAX_INTERFACE_WORKER_EVENT_BYTES).await
}

pub async fn read_interface_worker_envelope<R>(
    reader: &mut R,
) -> Result<InterfaceWorkerEnvelope, WorkerCodecError>
where
    R: AsyncRead + Unpin,
{
    let payload = read_worker_frame(reader, MAX_INTERFACE_WORKER_EVENT_BYTES).await?;
    InterfaceWorkerEnvelope::decode(&payload)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceWorkerServeStopReason {
    Eof,
    Shutdown,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfaceWorkerServeSummary {
    pub handled: usize,
    pub stop_reason: InterfaceWorkerServeStopReason,
}

pub async fn serve_interface_worker_envelopes<R, F, Fut>(
    reader: &mut R,
    mut handler: F,
) -> Result<InterfaceWorkerServeSummary, WorkerCodecError>
where
    R: AsyncRead + Unpin,
    F: FnMut(InterfaceWorkerEnvelope) -> Fut,
    Fut: Future<Output = Result<(), WorkerCodecError>>,
{
    let mut handled = 0usize;
    loop {
        match read_interface_worker_envelope(reader).await {
            Ok(envelope) if matches!(envelope.event, InterfaceWorkerEvent::Shutdown) => {
                return Ok(InterfaceWorkerServeSummary {
                    handled,
                    stop_reason: InterfaceWorkerServeStopReason::Shutdown,
                });
            }
            Ok(envelope) => {
                handler(envelope).await?;
                handled = handled.saturating_add(1);
            }
            Err(WorkerCodecError::Io { message }) if is_eof_io(&message) => {
                return Ok(InterfaceWorkerServeSummary {
                    handled,
                    stop_reason: InterfaceWorkerServeStopReason::Eof,
                });
            }
            Err(err) => return Err(err),
        }
    }
}

pub async fn serve_interface_worker_envelopes_until_cancelled<R, F, Fut>(
    reader: &mut R,
    mut handler: F,
    cancellation: CancellationToken,
) -> Result<InterfaceWorkerServeSummary, WorkerCodecError>
where
    R: AsyncRead + Unpin,
    F: FnMut(InterfaceWorkerEnvelope) -> Fut,
    Fut: Future<Output = Result<(), WorkerCodecError>>,
{
    let mut handled = 0usize;
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                return Ok(InterfaceWorkerServeSummary {
                    handled,
                    stop_reason: InterfaceWorkerServeStopReason::Cancelled,
                });
            }
            result = read_interface_worker_envelope(reader) => {
                match result {
                    Ok(envelope) if matches!(envelope.event, InterfaceWorkerEvent::Shutdown) => {
                        return Ok(InterfaceWorkerServeSummary {
                            handled,
                            stop_reason: InterfaceWorkerServeStopReason::Shutdown,
                        });
                    }
                    Ok(envelope) => {
                        handler(envelope).await?;
                        handled = handled.saturating_add(1);
                    }
                    Err(WorkerCodecError::Io { message }) if is_eof_io(&message) => {
                        return Ok(InterfaceWorkerServeSummary {
                            handled,
                            stop_reason: InterfaceWorkerServeStopReason::Eof,
                        });
                    }
                    Err(err) => return Err(err),
                }
            }
        }
    }
}

pub async fn forward_interface_tx_to_worker<W>(
    writer: &mut W,
    tx_receiver: &mut InterfaceTxReceiver,
    cancellation: CancellationToken,
) -> Result<InterfaceWorkerServeSummary, WorkerCodecError>
where
    W: AsyncWrite + Unpin,
{
    let mut handled = 0usize;
    let mut sequence = 0u64;
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                return Ok(InterfaceWorkerServeSummary {
                    handled,
                    stop_reason: InterfaceWorkerServeStopReason::Cancelled,
                });
            }
            message = tx_receiver.recv() => {
                let Some(message) = message else {
                    return Ok(InterfaceWorkerServeSummary {
                        handled,
                        stop_reason: InterfaceWorkerServeStopReason::Eof,
                    });
                };
                let envelope = InterfaceWorkerEnvelope::outbound_from_tx_message(sequence, &message)?;
                sequence = sequence.saturating_add(1);
                write_interface_worker_envelope(writer, &envelope).await?;
                handled = handled.saturating_add(1);
            }
        }
    }
}

pub async fn forward_interface_worker_rx_to_transport<R>(
    reader: &mut R,
    rx_sender: &InterfaceRxSender,
    cancellation: CancellationToken,
) -> Result<InterfaceWorkerServeSummary, WorkerCodecError>
where
    R: AsyncRead + Unpin,
{
    let mut handled = 0usize;
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                return Ok(InterfaceWorkerServeSummary {
                    handled,
                    stop_reason: InterfaceWorkerServeStopReason::Cancelled,
                });
            }
            result = read_interface_worker_envelope(reader) => {
                match result {
                    Ok(envelope) if matches!(envelope.event, InterfaceWorkerEvent::Shutdown) => {
                        return Ok(InterfaceWorkerServeSummary {
                            handled,
                            stop_reason: InterfaceWorkerServeStopReason::Shutdown,
                        });
                    }
                    Ok(envelope) => {
                        if let Some(message) = envelope.event.to_rx_message()? {
                            if let Err(err) = rx_sender.try_send(message) {
                                log::debug!(
                                    "interface worker rx channel rejected packet: {:?}",
                                    err
                                );
                            }
                        }
                        handled = handled.saturating_add(1);
                    }
                    Err(WorkerCodecError::Io { message }) if is_eof_io(&message) => {
                        return Ok(InterfaceWorkerServeSummary {
                            handled,
                            stop_reason: InterfaceWorkerServeStopReason::Eof,
                        });
                    }
                    Err(err) => return Err(err),
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterfaceWorkerEvent {
    InboundPacket {
        interface: [u8; ADDRESS_HASH_SIZE],
        packet_wire: ByteBuf,
        source: InterfaceWorkerSource,
    },
    OutboundPacket {
        target: InterfaceWorkerTarget,
        packet_wire: ByteBuf,
    },
    Health {
        interface: [u8; ADDRESS_HASH_SIZE],
        tx_queue_depth: usize,
        rx_queue_depth: usize,
    },
    Shutdown,
}

impl InterfaceWorkerEvent {
    pub fn inbound_from_rx_message(message: &RxMessage) -> Result<Self, WorkerCodecError> {
        Ok(Self::InboundPacket {
            interface: address_hash_bytes(message.address),
            packet_wire: ByteBuf::from(packet_wire_bytes(&message.packet)?),
            source: InterfaceWorkerSource::from_iface_source(message.source),
        })
    }

    pub fn outbound_from_tx_message(message: &TxMessage) -> Result<Self, WorkerCodecError> {
        Ok(Self::OutboundPacket {
            target: InterfaceWorkerTarget::from_tx_type(message.tx_type),
            packet_wire: ByteBuf::from(packet_wire_bytes(&message.packet)?),
        })
    }

    pub fn to_rx_message(&self) -> Result<Option<RxMessage>, WorkerCodecError> {
        let Self::InboundPacket { interface, packet_wire, source } = self else {
            return Ok(None);
        };
        Ok(Some(RxMessage {
            address: AddressHash::new(*interface),
            packet: packet_from_wire(packet_wire.as_ref())?,
            source: source.to_iface_source()?,
        }))
    }

    pub fn to_tx_message(&self) -> Result<Option<TxMessage>, WorkerCodecError> {
        let Self::OutboundPacket { target, packet_wire } = self else {
            return Ok(None);
        };
        Ok(Some(TxMessage {
            tx_type: target.to_tx_type(),
            packet: packet_from_wire(packet_wire.as_ref())?,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterfaceWorkerSource {
    None,
    Udp(String),
}

impl InterfaceWorkerSource {
    fn from_iface_source(source: IfaceSource) -> Self {
        match source {
            IfaceSource::None => Self::None,
            IfaceSource::Udp(addr) => Self::Udp(addr.to_string()),
        }
    }

    fn to_iface_source(&self) -> Result<IfaceSource, WorkerCodecError> {
        match self {
            Self::None => Ok(IfaceSource::None),
            Self::Udp(addr) => {
                addr.parse().map(IfaceSource::Udp).map_err(|err| WorkerCodecError::Decode {
                    message: format!("invalid interface udp source address: {err}"),
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterfaceWorkerTarget {
    Broadcast { from_interface: Option<[u8; ADDRESS_HASH_SIZE]> },
    Direct { interface: [u8; ADDRESS_HASH_SIZE] },
}

impl InterfaceWorkerTarget {
    fn from_tx_type(tx_type: TxMessageType) -> Self {
        match tx_type {
            TxMessageType::Broadcast(from_interface) => {
                Self::Broadcast { from_interface: from_interface.map(address_hash_bytes) }
            }
            TxMessageType::Direct(interface) => {
                Self::Direct { interface: address_hash_bytes(interface) }
            }
        }
    }

    fn to_tx_type(&self) -> TxMessageType {
        match self {
            Self::Broadcast { from_interface } => {
                TxMessageType::Broadcast(from_interface.map(AddressHash::new))
            }
            Self::Direct { interface } => TxMessageType::Direct(AddressHash::new(*interface)),
        }
    }
}

fn packet_wire_bytes(packet: &Packet) -> Result<Vec<u8>, WorkerCodecError> {
    packet
        .to_bytes()
        .map_err(|err| WorkerCodecError::Encode { message: format!("packet wire encode: {err:?}") })
}

fn packet_from_wire(wire: &[u8]) -> Result<Packet, WorkerCodecError> {
    Packet::from_bytes(wire)
        .map_err(|err| WorkerCodecError::Decode { message: format!("packet wire decode: {err:?}") })
}

fn address_hash_bytes(address: AddressHash) -> [u8; ADDRESS_HASH_SIZE] {
    let mut bytes = [0u8; ADDRESS_HASH_SIZE];
    bytes.copy_from_slice(address.as_slice());
    bytes
}

fn validate_interface_protocol_version(actual: u16) -> Result<(), WorkerCodecError> {
    if actual == INTERFACE_WORKER_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(WorkerCodecError::UnsupportedProtocolVersion {
            expected: INTERFACE_WORKER_PROTOCOL_VERSION,
            actual,
        })
    }
}

fn validate_interface_event_size(actual_bytes: usize) -> Result<(), WorkerCodecError> {
    if actual_bytes <= MAX_INTERFACE_WORKER_EVENT_BYTES {
        Ok(())
    } else {
        Err(WorkerCodecError::MessageTooLarge {
            max_bytes: MAX_INTERFACE_WORKER_EVENT_BYTES,
            actual_bytes,
        })
    }
}

fn is_eof_io(message: &str) -> bool {
    message.contains("early eof") || message.contains("unexpected end of file")
}

#[cfg(test)]
mod tests {
    use super::super::worker_boundary::WORKER_FRAME_HEADER_BYTES;
    use super::*;
    use crate::packet::{DestinationType, PacketContext, PacketDataBuffer, PacketType};

    fn packet(payload: &[u8]) -> Packet {
        let mut packet = Packet {
            destination: AddressHash::new([0xAB; ADDRESS_HASH_SIZE]),
            ..Default::default()
        };
        packet.header.destination_type = DestinationType::Single;
        packet.header.packet_type = PacketType::Data;
        packet.context = PacketContext::None;
        packet.data = PacketDataBuffer::new_from_slice(payload);
        packet
    }

    #[test]
    fn interface_inbound_envelope_round_trips_rx_message() {
        let message = RxMessage {
            address: AddressHash::new([0x11; ADDRESS_HASH_SIZE]),
            packet: packet(b"inbound"),
            source: IfaceSource::Udp("127.0.0.1:4242".parse().expect("udp source")),
        };

        let envelope =
            InterfaceWorkerEnvelope::inbound_from_rx_message(7, &message).expect("envelope");
        let frame = envelope.encode_frame().expect("encode frame");
        assert!(frame.len() >= WORKER_FRAME_HEADER_BYTES);

        let decoded = InterfaceWorkerEnvelope::decode_frame(&frame).expect("decode frame");
        assert_eq!(decoded.sequence, 7);
        assert_eq!(decoded.event.to_rx_message().expect("rx message"), Some(message));
    }

    #[test]
    fn interface_outbound_envelope_round_trips_tx_message() {
        let message = TxMessage {
            tx_type: TxMessageType::Broadcast(Some(AddressHash::new([0x22; ADDRESS_HASH_SIZE]))),
            packet: packet(b"outbound"),
        };

        let envelope =
            InterfaceWorkerEnvelope::outbound_from_tx_message(8, &message).expect("envelope");
        let decoded =
            InterfaceWorkerEnvelope::decode(&envelope.encode().expect("encode")).expect("decode");

        assert_eq!(decoded.sequence, 8);
        assert_eq!(decoded.event.to_tx_message().expect("tx message"), Some(message));
    }

    #[tokio::test]
    async fn interface_envelope_async_io_round_trips_between_stream_halves() {
        let (mut router, mut worker) = tokio::io::duplex(256);
        let message = RxMessage {
            address: AddressHash::new([0x33; ADDRESS_HASH_SIZE]),
            packet: packet(b"stream-inbound"),
            source: IfaceSource::None,
        };
        let envelope =
            InterfaceWorkerEnvelope::inbound_from_rx_message(9, &message).expect("envelope");
        let expected = envelope.clone();

        let writer = tokio::spawn(async move {
            write_interface_worker_envelope(&mut router, &envelope)
                .await
                .expect("write interface envelope");
        });

        let decoded =
            read_interface_worker_envelope(&mut worker).await.expect("read interface envelope");
        writer.await.expect("writer task");

        assert_eq!(decoded, expected);
        assert_eq!(decoded.event.to_rx_message().expect("rx message"), Some(message));
    }

    #[tokio::test]
    async fn interface_envelope_async_read_rejects_oversized_length_before_payload_alloc() {
        use tokio::io::AsyncWriteExt;

        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer
            .write_all(&((MAX_INTERFACE_WORKER_EVENT_BYTES + 1) as u32).to_be_bytes())
            .await
            .expect("write length");

        let err = read_interface_worker_envelope(&mut reader)
            .await
            .expect_err("oversized frame should fail");

        assert_eq!(
            err,
            WorkerCodecError::MessageTooLarge {
                max_bytes: MAX_INTERFACE_WORKER_EVENT_BYTES,
                actual_bytes: MAX_INTERFACE_WORKER_EVENT_BYTES + 1,
            }
        );
    }

    #[tokio::test]
    async fn interface_envelope_server_processes_until_shutdown() {
        let (mut writer, mut reader) = tokio::io::duplex(512);
        let outbound = InterfaceWorkerEnvelope::outbound_from_tx_message(
            10,
            &TxMessage {
                tx_type: TxMessageType::Direct(AddressHash::new([0x44; ADDRESS_HASH_SIZE])),
                packet: packet(b"one"),
            },
        )
        .expect("outbound envelope");
        let health = InterfaceWorkerEnvelope::new(
            11,
            InterfaceWorkerEvent::Health {
                interface: [0x55; ADDRESS_HASH_SIZE],
                tx_queue_depth: 2,
                rx_queue_depth: 3,
            },
        );
        let shutdown = InterfaceWorkerEnvelope::new(12, InterfaceWorkerEvent::Shutdown);

        let writer_task = tokio::spawn(async move {
            write_interface_worker_envelope(&mut writer, &outbound).await.expect("write outbound");
            write_interface_worker_envelope(&mut writer, &health).await.expect("write health");
            write_interface_worker_envelope(&mut writer, &shutdown).await.expect("write shutdown");
        });

        let mut sequences = Vec::new();
        let summary = serve_interface_worker_envelopes(&mut reader, |envelope| {
            sequences.push(envelope.sequence);
            async { Ok(()) }
        })
        .await
        .expect("serve interface envelopes");
        writer_task.await.expect("writer task");

        assert_eq!(sequences, vec![10, 11]);
        assert_eq!(
            summary,
            InterfaceWorkerServeSummary {
                handled: 2,
                stop_reason: InterfaceWorkerServeStopReason::Shutdown,
            }
        );
    }

    #[tokio::test]
    async fn interface_envelope_server_reports_cancelled() {
        let (_writer, mut reader) = tokio::io::duplex(64);
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let summary = serve_interface_worker_envelopes_until_cancelled(
            &mut reader,
            |_| async { Ok(()) },
            cancellation,
        )
        .await
        .expect("cancelled serve loop");

        assert_eq!(
            summary,
            InterfaceWorkerServeSummary {
                handled: 0,
                stop_reason: InterfaceWorkerServeStopReason::Cancelled,
            }
        );
    }

    #[tokio::test]
    async fn interface_tx_forwarder_writes_transport_messages_to_worker_stream() {
        let (tx_sender, mut tx_receiver) = tokio::sync::mpsc::channel(4);
        let (mut router, mut worker) = tokio::io::duplex(512);
        tx_sender
            .send(TxMessage {
                tx_type: TxMessageType::Direct(AddressHash::new([0x66; ADDRESS_HASH_SIZE])),
                packet: packet(b"tx-forward"),
            })
            .await
            .expect("queue tx message");
        drop(tx_sender);

        let cancellation = CancellationToken::new();
        let forwarder = tokio::spawn(async move {
            forward_interface_tx_to_worker(&mut router, &mut tx_receiver, cancellation)
                .await
                .expect("forward tx")
        });
        let envelope =
            read_interface_worker_envelope(&mut worker).await.expect("read worker envelope");
        let summary = forwarder.await.expect("forwarder task");

        assert_eq!(envelope.sequence, 0);
        assert_eq!(
            envelope.event.to_tx_message().expect("tx message"),
            Some(TxMessage {
                tx_type: TxMessageType::Direct(AddressHash::new([0x66; ADDRESS_HASH_SIZE])),
                packet: packet(b"tx-forward"),
            })
        );
        assert_eq!(
            summary,
            InterfaceWorkerServeSummary {
                handled: 1,
                stop_reason: InterfaceWorkerServeStopReason::Eof,
            }
        );
    }

    #[tokio::test]
    async fn interface_rx_forwarder_sends_worker_inbound_packets_to_transport_channel() {
        let (rx_sender, mut rx_receiver) = tokio::sync::mpsc::channel(4);
        let (mut worker, mut router) = tokio::io::duplex(512);
        let message = RxMessage {
            address: AddressHash::new([0x77; ADDRESS_HASH_SIZE]),
            packet: packet(b"rx-forward"),
            source: IfaceSource::None,
        };
        let envelope =
            InterfaceWorkerEnvelope::inbound_from_rx_message(42, &message).expect("envelope");
        let shutdown = InterfaceWorkerEnvelope::new(43, InterfaceWorkerEvent::Shutdown);

        let writer = tokio::spawn(async move {
            write_interface_worker_envelope(&mut worker, &envelope).await.expect("write inbound");
            write_interface_worker_envelope(&mut worker, &shutdown).await.expect("write shutdown");
        });
        let summary = forward_interface_worker_rx_to_transport(
            &mut router,
            &rx_sender,
            CancellationToken::new(),
        )
        .await
        .expect("forward rx");
        writer.await.expect("writer task");

        assert_eq!(rx_receiver.recv().await, Some(message));
        assert_eq!(
            summary,
            InterfaceWorkerServeSummary {
                handled: 1,
                stop_reason: InterfaceWorkerServeStopReason::Shutdown,
            }
        );
    }

    #[tokio::test]
    async fn interface_rx_forwarder_drops_when_transport_channel_is_full() {
        let (rx_sender, mut rx_receiver) = tokio::sync::mpsc::channel(1);
        let existing = RxMessage {
            address: AddressHash::new([0x11; ADDRESS_HASH_SIZE]),
            packet: packet(b"existing"),
            source: IfaceSource::None,
        };
        rx_sender.try_send(existing).expect("prefill rx channel");

        let (mut worker, mut router) = tokio::io::duplex(512);
        let inbound = RxMessage {
            address: AddressHash::new([0x77; ADDRESS_HASH_SIZE]),
            packet: packet(b"rx-forward"),
            source: IfaceSource::None,
        };
        let envelope =
            InterfaceWorkerEnvelope::inbound_from_rx_message(42, &inbound).expect("envelope");
        let shutdown = InterfaceWorkerEnvelope::new(43, InterfaceWorkerEvent::Shutdown);

        let writer = tokio::spawn(async move {
            write_interface_worker_envelope(&mut worker, &envelope).await.expect("write inbound");
            write_interface_worker_envelope(&mut worker, &shutdown).await.expect("write shutdown");
        });
        let summary = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            forward_interface_worker_rx_to_transport(
                &mut router,
                &rx_sender,
                CancellationToken::new(),
            ),
        )
        .await
        .expect("full transport rx channel should not stall interface worker forwarder")
        .expect("forward rx");
        writer.await.expect("writer task");

        assert_eq!(rx_receiver.try_recv().expect("existing message"), existing);
        assert!(rx_receiver.try_recv().is_err());
        assert_eq!(
            summary,
            InterfaceWorkerServeSummary {
                handled: 1,
                stop_reason: InterfaceWorkerServeStopReason::Shutdown,
            }
        );
    }

    #[test]
    fn interface_envelope_rejects_wrong_protocol_version() {
        let mut envelope = InterfaceWorkerEnvelope::new(1, InterfaceWorkerEvent::Shutdown);
        envelope.protocol_version = INTERFACE_WORKER_PROTOCOL_VERSION + 1;

        assert!(matches!(
            envelope.encode(),
            Err(WorkerCodecError::UnsupportedProtocolVersion { .. })
        ));
    }

    #[test]
    fn interface_envelope_rejects_oversized_payload() {
        let event = InterfaceWorkerEvent::InboundPacket {
            interface: [0x44; ADDRESS_HASH_SIZE],
            packet_wire: ByteBuf::from(vec![0u8; MAX_INTERFACE_WORKER_EVENT_BYTES + 1]),
            source: InterfaceWorkerSource::None,
        };
        let envelope = InterfaceWorkerEnvelope::new(1, event);

        assert!(matches!(envelope.encode(), Err(WorkerCodecError::MessageTooLarge { .. })));
    }
}

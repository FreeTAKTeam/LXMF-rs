use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use bzip2::write::BzEncoder;
use bzip2::Compression;

use crate::channel::{ChannelError, HandlerId, SystemMessageTypes, TypedMessage};
use crate::packet::PACKET_MDU;
use crate::transport::TransportChannel;

const STREAM_ID_MAX: u16 = 0x3FFF;
const STREAM_EOF_MASK: u16 = 0x8000;
const STREAM_COMPRESSED_MASK: u16 = 0x4000;
const STREAM_DATA_OVERHEAD: usize = 2 + 6;
const STREAM_DATA_MAX_LEN: usize = PACKET_MDU - STREAM_DATA_OVERHEAD;
const MAX_CHUNK_LEN: usize = 1024 * 16;
const COMPRESSION_TRIES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamDataMessage {
    pub stream_id: u16,
    pub data: Vec<u8>,
    pub eof: bool,
    pub compressed: bool,
}

impl StreamDataMessage {
    pub fn new(
        stream_id: u16,
        data: impl Into<Vec<u8>>,
        eof: bool,
        compressed: bool,
    ) -> Result<Self, ChannelError> {
        if stream_id > STREAM_ID_MAX {
            return Err(ChannelError::InvalidFrame);
        }

        Ok(Self { stream_id, data: data.into(), eof, compressed })
    }

    pub fn max_data_len() -> usize {
        STREAM_DATA_MAX_LEN
    }
}

impl TypedMessage for StreamDataMessage {
    const MSG_TYPE: u16 = SystemMessageTypes::StreamData as u16;

    fn is_system_type() -> bool {
        true
    }

    fn encode(&self) -> Vec<u8> {
        let mut header = self.stream_id & STREAM_ID_MAX;
        if self.eof {
            header |= STREAM_EOF_MASK;
        }
        if self.compressed {
            header |= STREAM_COMPRESSED_MASK;
        }

        let mut out = Vec::with_capacity(2 + self.data.len());
        out.extend_from_slice(&header.to_be_bytes());
        out.extend_from_slice(&self.data);
        out
    }

    fn decode(payload: &[u8]) -> Result<Self, ChannelError> {
        if payload.len() < 2 {
            return Err(ChannelError::InvalidFrame);
        }

        let header = u16::from_be_bytes([payload[0], payload[1]]);
        let eof = (header & STREAM_EOF_MASK) != 0;
        let compressed = (header & STREAM_COMPRESSED_MASK) != 0;
        let stream_id = header & STREAM_ID_MAX;
        let mut data = payload[2..].to_vec();

        if compressed {
            let compressed_data = data;
            let mut decoder = bzip2::read::BzDecoder::new(compressed_data.as_slice());
            let mut decoded = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut decoded)
                .map_err(|_| ChannelError::InvalidFrame)?;
            data = decoded;
        }

        Ok(Self { stream_id, data, eof, compressed })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReadyCallbackId(u64);

impl ReadyCallbackId {
    fn new(raw: u64) -> Self {
        Self(raw)
    }
}

type ReadyCallback = Box<dyn Fn(usize) + Send + Sync>;

#[derive(Default)]
struct ReaderState {
    buffer: Vec<u8>,
    eof: bool,
    next_callback_id: u64,
    callbacks: HashMap<ReadyCallbackId, ReadyCallback>,
}

#[derive(Clone)]
pub struct RawChannelReader {
    stream_id: u16,
    channel: TransportChannel,
    handler_id: HandlerId,
    state: Arc<Mutex<ReaderState>>,
}

impl RawChannelReader {
    pub async fn attach(
        stream_id: u16,
        channel: TransportChannel,
    ) -> Result<Self, ChannelError> {
        if stream_id > STREAM_ID_MAX {
            return Err(ChannelError::InvalidFrame);
        }

        channel.open().await?;
        let state = Arc::new(Mutex::new(ReaderState::default()));
        let state_for_handler = state.clone();
        let handler_id = channel
            .register_typed_handler::<StreamDataMessage, _>(move |message| {
                if message.stream_id != stream_id {
                    return false;
                }

                let mut state = state_for_handler.lock().expect("reader state");
                if !message.data.is_empty() {
                    state.buffer.extend_from_slice(&message.data);
                }
                if message.eof {
                    state.eof = true;
                }
                let ready = state.buffer.len();
                for callback in state.callbacks.values() {
                    callback(ready);
                }
                true
            })
            .await?;

        Ok(Self { stream_id, channel, handler_id, state })
    }

    pub fn stream_id(&self) -> u16 {
        self.stream_id
    }

    pub fn add_ready_callback<F>(&self, callback: F) -> ReadyCallbackId
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let mut state = self.state.lock().expect("reader state");
        let id = ReadyCallbackId::new(state.next_callback_id);
        state.next_callback_id = state.next_callback_id.wrapping_add(1);
        state.callbacks.insert(id, Box::new(callback));
        id
    }

    pub fn remove_ready_callback(&self, callback_id: ReadyCallbackId) -> bool {
        self.state.lock().expect("reader state").callbacks.remove(&callback_id).is_some()
    }

    pub fn read(&self, max_len: usize) -> Option<Vec<u8>> {
        let mut state = self.state.lock().expect("reader state");
        let to_read = max_len.min(state.buffer.len());
        if to_read == 0 {
            return state.eof.then(Vec::new);
        }

        let out = state.buffer.drain(..to_read).collect::<Vec<_>>();
        Some(out)
    }

    pub fn ready_len(&self) -> usize {
        self.state.lock().expect("reader state").buffer.len()
    }

    pub fn is_eof(&self) -> bool {
        let state = self.state.lock().expect("reader state");
        state.eof && state.buffer.is_empty()
    }

    pub async fn close(&self) -> Result<bool, ChannelError> {
        self.channel.remove_handler(self.handler_id).await
    }
}

pub struct RawChannelWriter {
    stream_id: u16,
    channel: TransportChannel,
    eof_sent: bool,
}

impl RawChannelWriter {
    pub fn new(stream_id: u16, channel: TransportChannel) -> Result<Self, ChannelError> {
        if stream_id > STREAM_ID_MAX {
            return Err(ChannelError::InvalidFrame);
        }

        Ok(Self { stream_id, channel, eof_sent: false })
    }

    pub async fn write(&self, bytes: &[u8]) -> Result<usize, ChannelError> {
        let (message, processed) = Self::encode_chunk(self.stream_id, bytes, false)?;
        self.channel.open().await?;
        self.channel.send_typed(&message).await?;
        Ok(processed)
    }

    pub async fn close(&mut self) -> Result<(), ChannelError> {
        if self.eof_sent {
            return Ok(());
        }

        let message = StreamDataMessage::new(self.stream_id, Vec::new(), true, false)?;
        self.channel.open().await?;
        self.channel.send_typed(&message).await?;
        self.eof_sent = true;
        Ok(())
    }

    pub fn encode_chunk(
        stream_id: u16,
        bytes: &[u8],
        eof: bool,
    ) -> Result<(StreamDataMessage, usize), ChannelError> {
        if stream_id > STREAM_ID_MAX {
            return Err(ChannelError::InvalidFrame);
        }

        let mut chunk_len = bytes.len().min(MAX_CHUNK_LEN);
        let mut compressed_data = None;
        let mut processed_length = 0usize;

        if chunk_len > 32 {
            for attempt in 1..COMPRESSION_TRIES {
                let segment_len = chunk_len / attempt;
                if segment_len == 0 {
                    break;
                }

                let mut encoder = BzEncoder::new(Vec::new(), Compression::default());
                encoder
                    .write_all(&bytes[..segment_len])
                    .map_err(|_| ChannelError::InvalidFrame)?;
                let candidate = encoder.finish().map_err(|_| ChannelError::InvalidFrame)?;
                if candidate.len() < STREAM_DATA_MAX_LEN && candidate.len() < segment_len {
                    compressed_data = Some(candidate);
                    processed_length = segment_len;
                    break;
                }
            }
        }

        if let Some(data) = compressed_data {
            let message = StreamDataMessage::new(stream_id, data, eof, true)?;
            return Ok((message, processed_length));
        }

        chunk_len = chunk_len.min(STREAM_DATA_MAX_LEN);
        let raw = bytes[..chunk_len].to_vec();
        let message = StreamDataMessage::new(stream_id, raw, eof, false)?;
        Ok((message, chunk_len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::link::{Link, LinkHandleResult};
    use crate::destination::{DestinationDesc, DestinationName};
    use crate::hash::AddressHash;
    use crate::identity::PrivateIdentity;
    use crate::transport::{Transport, TransportConfig};
    use rand_core::OsRng;
    use std::sync::Mutex as StdMutex;
    use tokio::sync::Mutex;

    #[test]
    fn stream_data_message_roundtrips_compressed_payloads() {
        let payload = vec![b'A'; 256];
        let (message, processed) =
            RawChannelWriter::encode_chunk(7, payload.as_slice(), false).expect("chunk");
        assert_eq!(processed, payload.len());
        assert!(message.compressed);

        let decoded = StreamDataMessage::decode(&message.encode()).expect("decode");
        assert_eq!(decoded.stream_id, 7);
        assert_eq!(decoded.data, payload);
        assert!(!decoded.eof);
    }

    #[tokio::test]
    async fn stream_data_message_rejects_out_of_range_stream_ids() {
        assert!(StreamDataMessage::new(STREAM_ID_MAX + 1, Vec::new(), false, false).is_err());
        let transport = test_transport();
        let channel = transport.channel(AddressHash::new_from_rand(OsRng));
        assert!(RawChannelWriter::new(STREAM_ID_MAX + 1, channel).is_err());
    }

    #[tokio::test]
    async fn raw_channel_reader_buffers_matching_stream_messages() {
        let transport = test_transport();
        let (outbound, mut inbound, iface, channel) = linked_channel(&transport).await;
        let reader = RawChannelReader::attach(23, channel).await.expect("reader");

        let ready = Arc::new(StdMutex::new(Vec::new()));
        let ready_clone = ready.clone();
        reader.add_ready_callback(move |count| ready_clone.lock().expect("lock").push(count));

        let message =
            StreamDataMessage::new(23, b"hello-channel".to_vec(), false, false).expect("message");
        let (_sequence, packet) = inbound
            .send_channel_message(StreamDataMessage::MSG_TYPE, message.encode())
            .expect("channel message");

        let result = outbound.lock().await.handle_packet(&packet, iface);
        assert!(matches!(result, LinkHandleResult::Proof(_)));
        assert_eq!(reader.ready_len(), b"hello-channel".len());
        assert_eq!(reader.read(5).expect("chunk"), b"hello".to_vec());
        assert_eq!(reader.read(32).expect("chunk"), b"-channel".to_vec());
        assert_eq!(ready.lock().expect("lock").as_slice(), &[13]);
    }

    #[tokio::test]
    async fn raw_channel_reader_close_unregisters_handler() {
        let transport = test_transport();
        let (outbound, mut inbound, iface, channel) = linked_channel(&transport).await;
        let reader = RawChannelReader::attach(9, channel).await.expect("reader");
        assert!(reader.close().await.expect("close"));

        let message = StreamDataMessage::new(9, b"after-close".to_vec(), false, false).expect("message");
        let (_sequence, packet) = inbound
            .send_channel_message(StreamDataMessage::MSG_TYPE, message.encode())
            .expect("channel message");

        let result = outbound.lock().await.handle_packet(&packet, iface);
        assert!(matches!(result, LinkHandleResult::Proof(_)));
        assert!(reader.read(64).is_none());
    }

    fn test_transport() -> Transport {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let config = TransportConfig::new("test", &identity, true);
        Transport::new(config)
    }

    async fn linked_channel(
        transport: &Transport,
    ) -> (
        Arc<Mutex<Link>>,
        Link,
        AddressHash,
        TransportChannel,
    ) {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let outbound = transport.link(destination).await;
        let request = outbound.lock().await.request();
        let (tx, _) = tokio::sync::broadcast::channel(8);
        let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.lock().await.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let link_id = *outbound.lock().await.id();
        let channel = transport.channel(link_id);

        (outbound, inbound, iface, channel)
    }
}

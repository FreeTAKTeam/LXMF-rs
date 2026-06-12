use std::collections::HashMap;

use std::io::{Read, Write};

use std::sync::atomic::{AtomicBool, Ordering};

use std::sync::{Arc, Mutex};

use std::time::Duration;

use bzip2::write::BzEncoder;

use bzip2::Compression;

use tokio::time::{sleep, Instant};

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

const CLOSE_WAIT_FALLBACK: Duration = Duration::from_secs(15);

const CLOSE_WAIT_POLL: Duration = Duration::from_millis(50);

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

    pub fn max_encoded_data_len() -> usize {
        STREAM_DATA_MAX_LEN
    }

    pub fn max_decoded_data_len() -> usize {
        MAX_CHUNK_LEN
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
            let decoder = bzip2::read::BzDecoder::new(compressed_data.as_slice());
            let mut decoded = Vec::new();
            let mut limited = decoder.take(MAX_CHUNK_LEN as u64 + 1);
            limited.read_to_end(&mut decoded).map_err(|_| ChannelError::InvalidFrame)?;
            if decoded.len() > MAX_CHUNK_LEN {
                return Err(ChannelError::InvalidFrame);
            }
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

type ReadyCallback = Arc<dyn Fn(usize) + Send + Sync>;

#[derive(Default)]
struct ReaderState {
    buffer: Vec<u8>,
    eof: bool,
    next_callback_id: u64,
    callbacks: HashMap<ReadyCallbackId, ReadyCallback>,
}

fn dispatch_ready_callbacks(callbacks: Vec<ReadyCallback>, ready: usize) {
    if callbacks.is_empty() {
        return;
    }

    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn_blocking(move || {
                for callback in callbacks {
                    callback(ready);
                }
            });
        }
        Err(err) => {
            log::warn!("channel_buffer: failed to detach ready callbacks: {}", err);
            for callback in callbacks {
                callback(ready);
            }
        }
    }
}

#[derive(Clone)]
pub struct RawChannelReader {
    stream_id: u16,
    channel: TransportChannel,
    handler_id: HandlerId,
    state: Arc<Mutex<ReaderState>>,
}

impl RawChannelReader {
    pub async fn attach(stream_id: u16, channel: TransportChannel) -> Result<Self, ChannelError> {
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
                let callbacks = state.callbacks.values().cloned().collect::<Vec<_>>();
                drop(state);
                dispatch_ready_callbacks(callbacks, ready);
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
        state.callbacks.insert(id, Arc::new(callback));
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
        let removed = self.channel.remove_handler(self.handler_id).await?;
        self.state.lock().expect("reader state").callbacks.clear();
        Ok(removed)
    }
}

pub struct RawChannelWriter {
    stream_id: u16,
    channel: TransportChannel,
    eof_sent: AtomicBool,
}

impl RawChannelWriter {
    pub fn new(stream_id: u16, channel: TransportChannel) -> Result<Self, ChannelError> {
        if stream_id > STREAM_ID_MAX {
            return Err(ChannelError::InvalidFrame);
        }

        Ok(Self { stream_id, channel, eof_sent: AtomicBool::new(false) })
    }

    pub fn stream_id(&self) -> u16 {
        self.stream_id
    }

    pub fn max_chunk_len(&self) -> usize {
        MAX_CHUNK_LEN
    }

    pub async fn write(&self, bytes: &[u8]) -> Result<usize, ChannelError> {
        if self.eof_sent.load(Ordering::Acquire) {
            return Ok(0);
        }

        let (message, processed) = Self::encode_chunk(self.stream_id, bytes, false)?;
        self.channel.open().await?;
        match self.channel.send_typed(&message).await {
            Ok(_) => Ok(processed),
            Err(ChannelError::LinkNotReady) => Ok(0),
            Err(err) => Err(err),
        }
    }

    pub async fn write_all(&self, bytes: &[u8]) -> Result<usize, ChannelError> {
        if self.eof_sent.load(Ordering::Acquire) {
            return Ok(0);
        }

        let mut total = 0usize;
        let mut remaining = bytes;

        while !remaining.is_empty() {
            let written = self.write(remaining).await?;
            if written == 0 {
                break;
            }
            total += written;
            remaining = &remaining[written..];
        }

        Ok(total)
    }

    pub async fn close(&mut self) -> Result<(), ChannelError> {
        if self.eof_sent.load(Ordering::Acquire) {
            return Ok(());
        }

        let timeout = self.channel.close_wait_hint().await.unwrap_or(CLOSE_WAIT_FALLBACK);
        let deadline = Instant::now() + timeout;

        loop {
            match self.channel.is_ready_to_send().await {
                Ok(true) => break,
                Ok(false) if Instant::now() < deadline => sleep(CLOSE_WAIT_POLL).await,
                Ok(false) | Err(ChannelError::LinkNotReady) => break,
                Err(err) => return Err(err),
            }
        }

        let message = StreamDataMessage::new(self.stream_id, Vec::new(), true, false)?;
        match self.channel.open().await {
            Ok(()) => {}
            Err(ChannelError::LinkNotReady) => {
                self.eof_sent.store(true, Ordering::Release);
                return Ok(());
            }
            Err(err) => return Err(err),
        }
        match self.channel.send_typed(&message).await {
            Ok(_) | Err(ChannelError::LinkNotReady) => {}
            Err(err) => return Err(err),
        }
        self.eof_sent.store(true, Ordering::Release);
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
            for attempt in 1..=COMPRESSION_TRIES {
                let segment_len = chunk_len / attempt;
                if segment_len == 0 {
                    break;
                }

                let mut encoder = BzEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(&bytes[..segment_len]).map_err(|_| ChannelError::InvalidFrame)?;
                let candidate = encoder.finish().map_err(|_| ChannelError::InvalidFrame)?;
                if candidate.len() <= STREAM_DATA_MAX_LEN && candidate.len() < segment_len {
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

pub struct BidirectionalChannelBuffer {
    pub reader: RawChannelReader,
    pub writer: RawChannelWriter,
}

pub struct Buffer;

impl Buffer {
    pub async fn create_reader(
        stream_id: u16,
        channel: TransportChannel,
    ) -> Result<RawChannelReader, ChannelError> {
        RawChannelReader::attach(stream_id, channel).await
    }

    pub async fn create_reader_with_callback<F>(
        stream_id: u16,
        channel: TransportChannel,
        ready_callback: F,
    ) -> Result<RawChannelReader, ChannelError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let reader = Self::create_reader(stream_id, channel).await?;
        reader.add_ready_callback(ready_callback);
        Ok(reader)
    }

    pub fn create_writer(
        stream_id: u16,
        channel: TransportChannel,
    ) -> Result<RawChannelWriter, ChannelError> {
        RawChannelWriter::new(stream_id, channel)
    }

    pub async fn create_bidirectional_buffer(
        receive_stream_id: u16,
        send_stream_id: u16,
        channel: TransportChannel,
    ) -> Result<BidirectionalChannelBuffer, ChannelError> {
        let reader = Self::create_reader(receive_stream_id, channel.clone()).await?;
        let writer = Self::create_writer(send_stream_id, channel)?;
        Ok(BidirectionalChannelBuffer { reader, writer })
    }

    pub async fn create_bidirectional_buffer_with_callback<F>(
        receive_stream_id: u16,
        send_stream_id: u16,
        channel: TransportChannel,
        ready_callback: F,
    ) -> Result<BidirectionalChannelBuffer, ChannelError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let reader =
            Self::create_reader_with_callback(receive_stream_id, channel.clone(), ready_callback)
                .await?;
        let writer = Self::create_writer(send_stream_id, channel)?;
        Ok(BidirectionalChannelBuffer { reader, writer })
    }
}

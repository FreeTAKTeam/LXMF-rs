use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::destination::link::{LinkPacketContext, LinkPacketContextSnapshot};
use crate::destination::NAME_HASH_LENGTH;
use crate::hash::{ADDRESS_HASH_SIZE, HASH_SIZE};
use crate::identity::PUBLIC_KEY_LENGTH;
use crate::resource::{ResourceCompletionOutcome, ResourceCompletionSnapshot};

pub const WORKER_PROTOCOL_VERSION: u16 = 1;
pub const MAX_WORKER_REQUEST_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_WORKER_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const WORKER_FRAME_HEADER_BYTES: usize = 4;

pub type WorkerJobFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WorkerResult, WorkerError>> + Send + 'a>>;

pub trait WorkerBackend: Send + Sync {
    fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_>;
}

pub async fn submit_outbound_encrypt_batch(
    backend: &dyn WorkerBackend,
    job_id: u64,
    items: Vec<OutboundEncryptBatchItem>,
) -> Result<Vec<PacketWireBatchItem>, WorkerError> {
    let expected_items = items.len();
    let result = backend
        .submit(WorkerJob { id: job_id, kind: WorkerJobKind::OutboundEncryptBatch { items } })
        .await?;
    let WorkerResultKind::PacketWireBatch { items } = result.kind else {
        return Err(WorkerError::InvalidJob {
            message: "worker returned non-packet batch for outbound encrypt batch".to_string(),
        });
    };
    if items.len() != expected_items {
        return Err(WorkerError::InvalidJob {
            message: format!(
                "worker returned {} packet batch items, expected {expected_items}",
                items.len()
            ),
        });
    }
    Ok(items)
}

pub async fn submit_single_destination_decrypt_batch(
    backend: &dyn WorkerBackend,
    job_id: u64,
    items: Vec<SingleDestinationDecryptBatchItem>,
) -> Result<Vec<DestinationPayloadBatchItem>, WorkerError> {
    let expected_items = items.len();
    let result = backend
        .submit(WorkerJob {
            id: job_id,
            kind: WorkerJobKind::SingleDestinationDecryptBatch { items },
        })
        .await?;
    let WorkerResultKind::DestinationPayloadBatch { items } = result.kind else {
        return Err(WorkerError::InvalidJob {
            message: "worker returned non-payload batch for single destination decrypt batch"
                .to_string(),
        });
    };
    if items.len() != expected_items {
        return Err(WorkerError::InvalidJob {
            message: format!(
                "worker returned {} payload batch items, expected {expected_items}",
                items.len()
            ),
        });
    }
    Ok(items)
}

#[derive(Clone)]
pub struct WorkerClient {
    backend: Arc<dyn WorkerBackend>,
}

impl WorkerClient {
    pub fn new(backend: Arc<dyn WorkerBackend>) -> Self {
        Self { backend }
    }

    pub async fn submit(&self, request: WorkerRequest) -> WorkerResponse {
        if let Err(err) = validate_protocol_version(request.protocol_version) {
            return WorkerResponse::failure(
                request.job.id,
                WorkerError::InvalidJob { message: format!("{err:?}") },
            );
        }

        let job_id = request.job.id;
        let deadline = Duration::from_millis(request.timeout_ms);
        match tokio::time::timeout(deadline, self.backend.submit(request.job)).await {
            Ok(Ok(result)) if result.id == job_id => WorkerResponse::success(result),
            Ok(Ok(result)) => WorkerResponse::failure(
                job_id,
                WorkerError::InvalidJob {
                    message: format!(
                        "worker returned result for job {}, expected {}",
                        result.id, job_id
                    ),
                },
            ),
            Ok(Err(err)) => WorkerResponse::failure(job_id, err),
            Err(_) => WorkerResponse::failure(
                job_id,
                WorkerError::TimedOut {
                    message: format!("worker job timed out after {deadline:?}"),
                },
            ),
        }
    }

    pub async fn submit_encoded(&self, request_bytes: &[u8]) -> Vec<u8> {
        let response = match WorkerRequest::decode(request_bytes) {
            Ok(request) => self.submit(request).await,
            Err(err) => WorkerResponse::failure(
                0,
                WorkerError::InvalidJob {
                    message: format!("invalid worker request envelope: {err:?}"),
                },
            ),
        };

        response.encode().unwrap_or_else(|err| {
            WorkerResponse::failure(
                response.job_id,
                WorkerError::BackendUnavailable {
                    message: format!("failed to encode worker response: {err:?}"),
                },
            )
            .encode()
            .unwrap_or_default()
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub protocol_version: u16,
    pub timeout_ms: u64,
    pub job: WorkerJob,
}

impl WorkerRequest {
    pub fn new(job: WorkerJob, timeout_ms: u64) -> Self {
        Self { protocol_version: WORKER_PROTOCOL_VERSION, timeout_ms, job }
    }

    pub fn encode(&self) -> Result<Vec<u8>, WorkerCodecError> {
        encode_versioned(self.protocol_version, self, MAX_WORKER_REQUEST_BYTES)
    }

    pub fn decode(data: &[u8]) -> Result<Self, WorkerCodecError> {
        validate_encoded_size(data.len(), MAX_WORKER_REQUEST_BYTES)?;
        let request: Self = rmp_serde::from_slice(data)
            .map_err(|err| WorkerCodecError::Decode { message: err.to_string() })?;
        validate_protocol_version(request.protocol_version)?;
        Ok(request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerResponse {
    pub protocol_version: u16,
    pub job_id: u64,
    pub outcome: Result<WorkerResult, WorkerError>,
}

impl WorkerResponse {
    pub fn success(result: WorkerResult) -> Self {
        Self { protocol_version: WORKER_PROTOCOL_VERSION, job_id: result.id, outcome: Ok(result) }
    }

    pub fn failure(job_id: u64, error: WorkerError) -> Self {
        Self { protocol_version: WORKER_PROTOCOL_VERSION, job_id, outcome: Err(error) }
    }

    pub fn encode(&self) -> Result<Vec<u8>, WorkerCodecError> {
        encode_versioned(self.protocol_version, self, MAX_WORKER_RESPONSE_BYTES)
    }

    pub fn decode(data: &[u8]) -> Result<Self, WorkerCodecError> {
        validate_encoded_size(data.len(), MAX_WORKER_RESPONSE_BYTES)?;
        let response: Self = rmp_serde::from_slice(data)
            .map_err(|err| WorkerCodecError::Decode { message: err.to_string() })?;
        validate_protocol_version(response.protocol_version)?;
        Ok(response)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerJob {
    pub id: u64,
    pub kind: WorkerJobKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerJobKind {
    ValidateAnnounce {
        #[serde(with = "serde_bytes")]
        packet_wire: Vec<u8>,
    },
    OutboundEncrypt {
        #[serde(with = "serde_bytes")]
        packet_wire: Vec<u8>,
        public_key: [u8; 32],
        salt: [u8; ADDRESS_HASH_SIZE],
    },
    OutboundEncryptBatch {
        items: Vec<OutboundEncryptBatchItem>,
    },
    SingleDestinationDecrypt {
        #[serde(with = "serde_bytes")]
        packet_wire: Vec<u8>,
        destination: [u8; ADDRESS_HASH_SIZE],
        private_key: ByteBuf,
    },
    SingleDestinationDecryptBatch {
        items: Vec<SingleDestinationDecryptBatchItem>,
    },
    ResourcePrepare {
        link_id: [u8; ADDRESS_HASH_SIZE],
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        metadata: Option<ByteBuf>,
        request_id: Option<ByteBuf>,
        is_response: bool,
    },
    ResourceComplete {
        link_id: [u8; ADDRESS_HASH_SIZE],
        link_context: Option<LinkPacketContextSnapshot>,
        resource_hash: [u8; HASH_SIZE],
        random_hash: [u8; crate::resource::RANDOM_HASH_SIZE],
        encrypted: bool,
        compressed: bool,
        has_metadata: bool,
        data_size: u64,
        request_id: Option<ByteBuf>,
        is_request: bool,
        is_response: bool,
        stream: ByteBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundEncryptBatchItem {
    #[serde(with = "serde_bytes")]
    pub packet_wire: Vec<u8>,
    pub public_key: [u8; 32],
    pub salt: [u8; ADDRESS_HASH_SIZE],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SingleDestinationDecryptBatchItem {
    #[serde(with = "serde_bytes")]
    pub packet_wire: Vec<u8>,
    pub destination: [u8; ADDRESS_HASH_SIZE],
    pub private_key: ByteBuf,
}

impl WorkerJobKind {
    pub fn complete_resource_with<F>(self, decrypt: F) -> Result<WorkerResultKind, WorkerError>
    where
        F: FnOnce(&[u8]) -> Result<Vec<u8>, WorkerError>,
    {
        let link_context = match &self {
            Self::ResourceComplete { link_context, .. } => link_context.clone(),
            _ => None,
        };
        let snapshot = self.into_resource_completion_snapshot()?;
        let mut decrypt_error = None;
        let outcome = snapshot
            .complete_with(|ciphertext| {
                if let Some(link_context) = link_context {
                    let link_context =
                        LinkPacketContext::from_snapshot(link_context).map_err(|err| {
                            decrypt_error = Some(WorkerError::InvalidJob {
                                message: format!("invalid link decrypt context: {err:?}"),
                            });
                        })?;
                    let mut out = vec![0u8; ciphertext.len() + 64];
                    return link_context
                        .decrypt(ciphertext, &mut out)
                        .map(|plaintext| plaintext.to_vec())
                        .map_err(|err| {
                            decrypt_error = Some(WorkerError::Crypto {
                                message: format!("resource decrypt failed: {err:?}"),
                            });
                        });
                }

                match decrypt(ciphertext) {
                    Ok(plaintext) => Ok(plaintext),
                    Err(err) => {
                        decrypt_error = Some(err);
                        Err(())
                    }
                }
            })
            .map_err(|()| {
                decrypt_error.unwrap_or_else(|| WorkerError::Packet {
                    message: "resource completion failed".to_string(),
                })
            })?;
        Ok(WorkerResultKind::resource_completed_from_outcome(outcome))
    }

    #[allow(dead_code)]
    pub(crate) fn resource_complete_from_snapshot(snapshot: ResourceCompletionSnapshot) -> Self {
        Self::ResourceComplete {
            link_id: snapshot.link_id,
            link_context: None,
            resource_hash: snapshot.resource_hash,
            random_hash: snapshot.random_hash,
            encrypted: snapshot.encrypted,
            compressed: snapshot.compressed,
            has_metadata: snapshot.has_metadata,
            data_size: snapshot.data_size,
            request_id: snapshot.request_id,
            is_request: snapshot.is_request,
            is_response: snapshot.is_response,
            stream: snapshot.stream,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn resource_complete_from_snapshot_with_link_context(
        snapshot: ResourceCompletionSnapshot,
        link_context: LinkPacketContextSnapshot,
    ) -> Self {
        let mut kind = Self::resource_complete_from_snapshot(snapshot);
        if let Self::ResourceComplete { link_context: slot, .. } = &mut kind {
            *slot = Some(link_context);
        }
        kind
    }

    #[allow(dead_code)]
    pub(crate) fn into_resource_completion_snapshot(
        self,
    ) -> Result<ResourceCompletionSnapshot, WorkerError> {
        let Self::ResourceComplete {
            link_id,
            link_context: _,
            resource_hash,
            random_hash,
            encrypted,
            compressed,
            has_metadata,
            data_size,
            request_id,
            is_request,
            is_response,
            stream,
        } = self
        else {
            return Err(WorkerError::InvalidJob {
                message: "worker job is not a resource completion job".to_string(),
            });
        };
        Ok(ResourceCompletionSnapshot {
            resource_hash,
            link_id,
            random_hash,
            encrypted,
            compressed,
            has_metadata,
            data_size,
            request_id,
            is_request,
            is_response,
            stream,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerResult {
    pub id: u64,
    pub kind: WorkerResultKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerResultKind {
    AnnounceValidated {
        destination: [u8; ADDRESS_HASH_SIZE],
        public_key: [u8; PUBLIC_KEY_LENGTH],
        verifying_key: [u8; PUBLIC_KEY_LENGTH],
        name_hash: [u8; NAME_HASH_LENGTH],
        app_data: ByteBuf,
        ratchet: Option<ByteBuf>,
    },
    PacketWire {
        #[serde(with = "serde_bytes")]
        packet_wire: Vec<u8>,
    },
    PacketWireBatch {
        items: Vec<PacketWireBatchItem>,
    },
    DestinationPayload {
        payload: ByteBuf,
        ratchet_used: bool,
    },
    DestinationPayloadBatch {
        items: Vec<DestinationPayloadBatchItem>,
    },
    ResourcePrepared {
        resource_hash: [u8; HASH_SIZE],
        #[serde(with = "serde_bytes")]
        advertisement_packet_wire: Vec<u8>,
    },
    ResourceCompleted {
        resource_hash: [u8; HASH_SIZE],
        proof: [u8; HASH_SIZE],
        data: ByteBuf,
        metadata: Option<ByteBuf>,
        request_id: Option<ByteBuf>,
        is_request: bool,
        is_response: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketWireBatchItem {
    #[serde(with = "serde_bytes")]
    pub packet_wire: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationPayloadBatchItem {
    pub payload: ByteBuf,
    pub ratchet_used: bool,
}

impl WorkerResultKind {
    #[allow(dead_code)]
    pub(crate) fn resource_completed_from_outcome(outcome: ResourceCompletionOutcome) -> Self {
        Self::ResourceCompleted {
            resource_hash: outcome.resource_hash,
            proof: outcome.proof,
            data: ByteBuf::from(outcome.data),
            metadata: outcome.metadata.map(ByteBuf::from),
            request_id: outcome.request_id.map(ByteBuf::from),
            is_request: outcome.is_request,
            is_response: outcome.is_response,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn into_resource_completion_outcome(
        self,
    ) -> Result<ResourceCompletionOutcome, WorkerError> {
        let Self::ResourceCompleted {
            resource_hash,
            proof,
            data,
            metadata,
            request_id,
            is_request,
            is_response,
        } = self
        else {
            return Err(WorkerError::InvalidJob {
                message: "worker result is not a resource completion result".to_string(),
            });
        };
        Ok(ResourceCompletionOutcome {
            resource_hash,
            proof,
            data: data.to_vec(),
            metadata: metadata.map(|value| value.to_vec()),
            request_id: request_id.map(|value| value.to_vec()),
            is_request,
            is_response,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerError {
    InvalidJob { message: String },
    Busy { message: String },
    Crypto { message: String },
    Packet { message: String },
    TimedOut { message: String },
    Cancelled { message: String },
    BackendUnavailable { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerCodecError {
    UnsupportedProtocolVersion { expected: u16, actual: u16 },
    MessageTooLarge { max_bytes: usize, actual_bytes: usize },
    IncompleteFrame { expected_bytes: usize, actual_bytes: usize },
    Io { message: String },
    Encode { message: String },
    Decode { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerServeStopReason {
    Eof,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerServeSummary {
    pub handled: usize,
    pub stop_reason: WorkerServeStopReason,
}

fn validate_protocol_version(actual: u16) -> Result<(), WorkerCodecError> {
    if actual == WORKER_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(WorkerCodecError::UnsupportedProtocolVersion {
            expected: WORKER_PROTOCOL_VERSION,
            actual,
        })
    }
}

fn encode_versioned<T: Serialize>(
    protocol_version: u16,
    value: &T,
    max_bytes: usize,
) -> Result<Vec<u8>, WorkerCodecError> {
    validate_protocol_version(protocol_version)?;
    let encoded = rmp_serde::to_vec_named(value)
        .map_err(|err| WorkerCodecError::Encode { message: err.to_string() })?;
    validate_encoded_size(encoded.len(), max_bytes)?;
    Ok(encoded)
}

fn validate_encoded_size(actual_bytes: usize, max_bytes: usize) -> Result<(), WorkerCodecError> {
    if actual_bytes <= max_bytes {
        Ok(())
    } else {
        Err(WorkerCodecError::MessageTooLarge { max_bytes, actual_bytes })
    }
}

pub fn encode_worker_frame(payload: &[u8], max_bytes: usize) -> Result<Vec<u8>, WorkerCodecError> {
    validate_encoded_size(payload.len(), max_bytes)?;
    let payload_len = u32::try_from(payload.len()).map_err(|_| {
        WorkerCodecError::MessageTooLarge { max_bytes, actual_bytes: payload.len() }
    })?;
    let mut frame = Vec::with_capacity(WORKER_FRAME_HEADER_BYTES + payload.len());
    frame.extend_from_slice(&payload_len.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

pub fn decode_worker_frame(frame: &[u8], max_bytes: usize) -> Result<&[u8], WorkerCodecError> {
    if frame.len() < WORKER_FRAME_HEADER_BYTES {
        return Err(WorkerCodecError::IncompleteFrame {
            expected_bytes: WORKER_FRAME_HEADER_BYTES,
            actual_bytes: frame.len(),
        });
    }
    let mut len_bytes = [0u8; WORKER_FRAME_HEADER_BYTES];
    len_bytes.copy_from_slice(&frame[..WORKER_FRAME_HEADER_BYTES]);
    let payload_len = u32::from_be_bytes(len_bytes) as usize;
    validate_encoded_size(payload_len, max_bytes)?;
    let expected_bytes = WORKER_FRAME_HEADER_BYTES + payload_len;
    if frame.len() < expected_bytes {
        return Err(WorkerCodecError::IncompleteFrame {
            expected_bytes,
            actual_bytes: frame.len(),
        });
    }
    Ok(&frame[WORKER_FRAME_HEADER_BYTES..expected_bytes])
}

pub async fn write_worker_frame<W>(
    writer: &mut W,
    payload: &[u8],
    max_bytes: usize,
) -> Result<(), WorkerCodecError>
where
    W: AsyncWrite + Unpin,
{
    let frame = encode_worker_frame(payload, max_bytes)?;
    writer
        .write_all(&frame)
        .await
        .map_err(|err| WorkerCodecError::Io { message: err.to_string() })?;
    writer.flush().await.map_err(|err| WorkerCodecError::Io { message: err.to_string() })
}

pub async fn read_worker_frame<R>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Vec<u8>, WorkerCodecError>
where
    R: AsyncRead + Unpin,
{
    let mut len_bytes = [0u8; WORKER_FRAME_HEADER_BYTES];
    reader
        .read_exact(&mut len_bytes)
        .await
        .map_err(|err| WorkerCodecError::Io { message: err.to_string() })?;
    let payload_len = u32::from_be_bytes(len_bytes) as usize;
    validate_encoded_size(payload_len, max_bytes)?;
    let mut payload = vec![0u8; payload_len];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|err| WorkerCodecError::Io { message: err.to_string() })?;
    Ok(payload)
}

pub async fn handle_worker_frame<S>(
    stream: &mut S,
    client: &WorkerClient,
) -> Result<(), WorkerCodecError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = read_worker_frame(stream, MAX_WORKER_REQUEST_BYTES).await?;
    let response = client.submit_encoded(&request).await;
    write_worker_frame(stream, response.as_slice(), MAX_WORKER_RESPONSE_BYTES).await
}

pub async fn serve_worker_frames<S>(
    stream: &mut S,
    client: &WorkerClient,
) -> Result<WorkerServeSummary, WorkerCodecError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut handled = 0usize;
    loop {
        match handle_worker_frame(stream, client).await {
            Ok(()) => {
                handled = handled.saturating_add(1);
            }
            Err(WorkerCodecError::Io { message })
                if message.contains("early eof") || message.contains("unexpected end of file") =>
            {
                return Ok(WorkerServeSummary { handled, stop_reason: WorkerServeStopReason::Eof });
            }
            Err(err) => return Err(err),
        }
    }
}

pub async fn serve_worker_frames_until_cancelled<S>(
    stream: &mut S,
    client: &WorkerClient,
    cancellation: CancellationToken,
) -> Result<WorkerServeSummary, WorkerCodecError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut handled = 0usize;
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                return Ok(WorkerServeSummary {
                    handled,
                    stop_reason: WorkerServeStopReason::Cancelled,
                });
            }
            result = handle_worker_frame(stream, client) => {
                match result {
                    Ok(()) => {
                        handled = handled.saturating_add(1);
                    }
                    Err(WorkerCodecError::Io { message })
                        if message.contains("early eof")
                            || message.contains("unexpected end of file") =>
                    {
                        return Ok(WorkerServeSummary {
                            handled,
                            stop_reason: WorkerServeStopReason::Eof,
                        });
                    }
                    Err(err) => return Err(err),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::link::{Link, LinkHandleResult};
    use crate::destination::{DestinationDesc, DestinationName};
    use crate::hash::AddressHash;
    use crate::identity::PrivateIdentity;
    use crate::resource::ResourceCompletionJob;
    use rand_core::OsRng;
    use sha2::Digest;

    struct EchoBackend;

    impl WorkerBackend for EchoBackend {
        fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
            Box::pin(async move {
                Ok(WorkerResult {
                    id: job.id,
                    kind: WorkerResultKind::PacketWire { packet_wire: b"ok".to_vec() },
                })
            })
        }
    }

    struct SlowBackend;

    impl WorkerBackend for SlowBackend {
        fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok(WorkerResult {
                    id: job.id,
                    kind: WorkerResultKind::PacketWire { packet_wire: b"late".to_vec() },
                })
            })
        }
    }

    struct MismatchedBackend;

    impl WorkerBackend for MismatchedBackend {
        fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
            Box::pin(async move {
                Ok(WorkerResult {
                    id: job.id + 1,
                    kind: WorkerResultKind::PacketWire { packet_wire: b"wrong".to_vec() },
                })
            })
        }
    }

    struct BatchEchoBackend {
        expected_items: Option<usize>,
    }

    impl WorkerBackend for BatchEchoBackend {
        fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
            let expected_items = self.expected_items;
            Box::pin(async move {
                match job.kind {
                    WorkerJobKind::OutboundEncryptBatch { items } => {
                        let items = items
                            .into_iter()
                            .take(expected_items.unwrap_or(usize::MAX))
                            .map(|item| PacketWireBatchItem { packet_wire: item.packet_wire })
                            .collect();
                        Ok(WorkerResult {
                            id: job.id,
                            kind: WorkerResultKind::PacketWireBatch { items },
                        })
                    }
                    WorkerJobKind::SingleDestinationDecryptBatch { items } => {
                        let items = items
                            .into_iter()
                            .take(expected_items.unwrap_or(usize::MAX))
                            .map(|item| DestinationPayloadBatchItem {
                                payload: item.private_key,
                                ratchet_used: false,
                            })
                            .collect();
                        Ok(WorkerResult {
                            id: job.id,
                            kind: WorkerResultKind::DestinationPayloadBatch { items },
                        })
                    }
                    _ => Err(WorkerError::InvalidJob {
                        message: "expected batch crypto job".to_string(),
                    }),
                }
            })
        }
    }

    fn announce_request(id: u64, timeout_ms: u64) -> WorkerRequest {
        WorkerRequest::new(
            WorkerJob {
                id,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"announce".to_vec() },
            },
            timeout_ms,
        )
    }

    #[test]
    fn worker_job_round_trips_through_msgpack() {
        let job = WorkerJob {
            id: 7,
            kind: WorkerJobKind::ResourcePrepare {
                link_id: [0x11; ADDRESS_HASH_SIZE],
                data: b"resource data".to_vec(),
                metadata: Some(ByteBuf::from(b"meta".to_vec())),
                request_id: Some(ByteBuf::from(b"request".to_vec())),
                is_response: false,
            },
        };

        let packed = rmp_serde::to_vec_named(&job).expect("pack worker job");
        let decoded: WorkerJob = rmp_serde::from_slice(&packed).expect("decode worker job");

        assert_eq!(decoded, job);
    }

    #[test]
    fn worker_batch_crypto_jobs_round_trip_through_msgpack() {
        let encrypt = WorkerJob {
            id: 70,
            kind: WorkerJobKind::OutboundEncryptBatch {
                items: vec![
                    OutboundEncryptBatchItem {
                        packet_wire: b"packet-a".to_vec(),
                        public_key: [0x11; PUBLIC_KEY_LENGTH],
                        salt: [0x22; ADDRESS_HASH_SIZE],
                    },
                    OutboundEncryptBatchItem {
                        packet_wire: b"packet-b".to_vec(),
                        public_key: [0x33; PUBLIC_KEY_LENGTH],
                        salt: [0x44; ADDRESS_HASH_SIZE],
                    },
                ],
            },
        };
        let decrypt = WorkerJob {
            id: 71,
            kind: WorkerJobKind::SingleDestinationDecryptBatch {
                items: vec![SingleDestinationDecryptBatchItem {
                    packet_wire: b"ciphertext".to_vec(),
                    destination: [0x55; ADDRESS_HASH_SIZE],
                    private_key: ByteBuf::from(vec![0x66; PUBLIC_KEY_LENGTH * 2]),
                }],
            },
        };

        for job in [encrypt, decrypt] {
            let packed = rmp_serde::to_vec_named(&job).expect("pack batch worker job");
            let decoded: WorkerJob = rmp_serde::from_slice(&packed).expect("decode batch job");

            assert_eq!(decoded, job);
        }
    }

    #[test]
    fn worker_resource_complete_job_round_trips_completion_snapshot() {
        let snapshot = ResourceCompletionSnapshot {
            link_id: [0x11; ADDRESS_HASH_SIZE],
            resource_hash: [0x22; HASH_SIZE],
            random_hash: [0x33; crate::resource::RANDOM_HASH_SIZE],
            encrypted: true,
            compressed: false,
            has_metadata: true,
            data_size: 4096,
            request_id: Some(ByteBuf::from(b"request-id".to_vec())),
            is_request: false,
            is_response: true,
            stream: ByteBuf::from(b"resource-stream".to_vec()),
        };
        let kind = WorkerJobKind::resource_complete_from_snapshot(snapshot.clone());
        let job = WorkerJob { id: 17, kind };

        let packed = rmp_serde::to_vec_named(&job).expect("pack worker job");
        let decoded: WorkerJob = rmp_serde::from_slice(&packed).expect("decode worker job");

        assert_eq!(decoded, job);
        assert_eq!(decoded.kind.into_resource_completion_snapshot().expect("snapshot"), snapshot);
    }

    #[test]
    fn worker_resource_complete_snapshot_rejects_wrong_job_kind() {
        let err = WorkerJobKind::ValidateAnnounce { packet_wire: b"announce".to_vec() }
            .into_resource_completion_snapshot()
            .expect_err("wrong job kind should fail");

        assert!(matches!(err, WorkerError::InvalidJob { .. }));
    }

    #[test]
    fn worker_resource_complete_result_round_trips_completion_outcome() {
        let outcome = ResourceCompletionOutcome {
            resource_hash: [0x22; HASH_SIZE],
            proof: [0x33; HASH_SIZE],
            data: b"payload".to_vec(),
            metadata: Some(b"metadata".to_vec()),
            request_id: Some(b"request-id".to_vec()),
            is_request: true,
            is_response: false,
        };
        let kind = WorkerResultKind::resource_completed_from_outcome(outcome.clone());
        let result = WorkerResult { id: 18, kind };

        let packed = rmp_serde::to_vec_named(&result).expect("pack worker result");
        let decoded: WorkerResult = rmp_serde::from_slice(&packed).expect("decode worker result");

        assert_eq!(decoded, result);
        assert_eq!(decoded.kind.into_resource_completion_outcome().expect("outcome"), outcome);
    }

    #[test]
    fn worker_resource_complete_outcome_rejects_wrong_result_kind() {
        let err = WorkerResultKind::PacketWire { packet_wire: b"not-complete".to_vec() }
            .into_resource_completion_outcome()
            .expect_err("wrong result kind should fail");

        assert!(matches!(err, WorkerError::InvalidJob { .. }));
    }

    #[test]
    fn worker_resource_complete_job_completes_unencrypted_payload() {
        let link_id = AddressHash::new_from_slice(b"worker completion");
        let job = ResourceCompletionJob::unencrypted_for_test(link_id, b"worker payload");
        let kind = WorkerJobKind::resource_complete_from_snapshot(job.to_snapshot());

        let result = kind
            .complete_resource_with(|_| {
                Err(WorkerError::Crypto { message: "decrypt should not run".to_string() })
            })
            .expect("complete resource");
        let outcome = result.into_resource_completion_outcome().expect("outcome");

        assert_eq!(outcome.data, b"worker payload");
        assert_eq!(outcome.resource_hash, job.to_snapshot().resource_hash);
        assert_ne!(outcome.proof, [0u8; HASH_SIZE]);
    }

    #[test]
    fn worker_resource_complete_job_decrypts_with_link_context_snapshot() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);
        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let proof = inbound.prove();
        let proof_iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(outbound.handle_packet(&proof, proof_iface), LinkHandleResult::Activated));

        let random_hash = [0x5a; crate::resource::RANDOM_HASH_SIZE];
        let payload = b"encrypted worker payload";
        let mut hasher = sha2::Sha256::new();
        hasher.update(payload);
        hasher.update(random_hash);
        let digest = hasher.finalize();
        let mut resource_hash = [0u8; HASH_SIZE];
        resource_hash.copy_from_slice(&digest[..HASH_SIZE]);
        let mut plain_stream = random_hash.to_vec();
        plain_stream.extend_from_slice(payload);
        let mut cipher_buf = vec![0u8; plain_stream.len() + 128];
        let encrypted_stream = outbound
            .packet_context()
            .encrypt(&plain_stream, &mut cipher_buf)
            .expect("encrypt resource stream")
            .to_vec();
        let mut link_id = [0u8; ADDRESS_HASH_SIZE];
        link_id.copy_from_slice(inbound.id().as_slice());
        let kind = WorkerJobKind::ResourceComplete {
            link_id,
            link_context: Some(inbound.packet_context().to_snapshot()),
            resource_hash,
            random_hash,
            encrypted: true,
            compressed: false,
            has_metadata: false,
            data_size: payload.len() as u64,
            request_id: None,
            is_request: false,
            is_response: false,
            stream: ByteBuf::from(encrypted_stream),
        };

        let result = kind
            .complete_resource_with(|_| {
                Err(WorkerError::Crypto { message: "snapshot decrypt should be used".to_string() })
            })
            .expect("complete encrypted resource");
        let outcome = result.into_resource_completion_outcome().expect("outcome");

        assert_eq!(outcome.data, payload);
        assert_eq!(outcome.resource_hash, resource_hash);
        assert_ne!(outcome.proof, [0u8; HASH_SIZE]);
    }

    #[test]
    fn worker_result_round_trips_through_msgpack() {
        let result = WorkerResult {
            id: 8,
            kind: WorkerResultKind::ResourceCompleted {
                resource_hash: [0x22; HASH_SIZE],
                proof: [0x33; HASH_SIZE],
                data: ByteBuf::from(b"payload".to_vec()),
                metadata: Some(ByteBuf::from(b"metadata".to_vec())),
                request_id: Some(ByteBuf::from(b"request-id".to_vec())),
                is_request: false,
                is_response: true,
            },
        };

        let packed = rmp_serde::to_vec_named(&result).expect("pack worker result");
        let decoded: WorkerResult = rmp_serde::from_slice(&packed).expect("decode worker result");

        assert_eq!(decoded, result);
    }

    #[test]
    fn worker_batch_crypto_results_round_trip_through_msgpack() {
        let packet_result = WorkerResult {
            id: 80,
            kind: WorkerResultKind::PacketWireBatch {
                items: vec![
                    PacketWireBatchItem { packet_wire: b"packet-a".to_vec() },
                    PacketWireBatchItem { packet_wire: b"packet-b".to_vec() },
                ],
            },
        };
        let payload_result = WorkerResult {
            id: 81,
            kind: WorkerResultKind::DestinationPayloadBatch {
                items: vec![DestinationPayloadBatchItem {
                    payload: ByteBuf::from(b"plain".to_vec()),
                    ratchet_used: false,
                }],
            },
        };

        for result in [packet_result, payload_result] {
            let packed = rmp_serde::to_vec_named(&result).expect("pack batch worker result");
            let decoded: WorkerResult =
                rmp_serde::from_slice(&packed).expect("decode batch worker result");

            assert_eq!(decoded, result);
        }
    }

    #[tokio::test]
    async fn submit_outbound_encrypt_batch_returns_ordered_packet_items() {
        let backend = BatchEchoBackend { expected_items: None };
        let items = submit_outbound_encrypt_batch(
            &backend,
            90,
            vec![
                OutboundEncryptBatchItem {
                    packet_wire: b"packet-a".to_vec(),
                    public_key: [0x11; PUBLIC_KEY_LENGTH],
                    salt: [0x22; ADDRESS_HASH_SIZE],
                },
                OutboundEncryptBatchItem {
                    packet_wire: b"packet-b".to_vec(),
                    public_key: [0x33; PUBLIC_KEY_LENGTH],
                    salt: [0x44; ADDRESS_HASH_SIZE],
                },
            ],
        )
        .await
        .expect("packet batch");

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].packet_wire, b"packet-a");
        assert_eq!(items[1].packet_wire, b"packet-b");
    }

    #[tokio::test]
    async fn submit_single_destination_decrypt_batch_returns_ordered_payload_items() {
        let backend = BatchEchoBackend { expected_items: None };
        let items = submit_single_destination_decrypt_batch(
            &backend,
            91,
            vec![
                SingleDestinationDecryptBatchItem {
                    packet_wire: b"cipher-a".to_vec(),
                    destination: [0x11; ADDRESS_HASH_SIZE],
                    private_key: ByteBuf::from(b"payload-a".to_vec()),
                },
                SingleDestinationDecryptBatchItem {
                    packet_wire: b"cipher-b".to_vec(),
                    destination: [0x22; ADDRESS_HASH_SIZE],
                    private_key: ByteBuf::from(b"payload-b".to_vec()),
                },
            ],
        )
        .await
        .expect("payload batch");

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].payload.as_ref(), b"payload-a");
        assert_eq!(items[1].payload.as_ref(), b"payload-b");
    }

    #[tokio::test]
    async fn submit_batch_helpers_reject_short_worker_results() {
        let backend = BatchEchoBackend { expected_items: Some(1) };
        let error = submit_outbound_encrypt_batch(
            &backend,
            92,
            vec![
                OutboundEncryptBatchItem {
                    packet_wire: b"packet-a".to_vec(),
                    public_key: [0x11; PUBLIC_KEY_LENGTH],
                    salt: [0x22; ADDRESS_HASH_SIZE],
                },
                OutboundEncryptBatchItem {
                    packet_wire: b"packet-b".to_vec(),
                    public_key: [0x33; PUBLIC_KEY_LENGTH],
                    salt: [0x44; ADDRESS_HASH_SIZE],
                },
            ],
        )
        .await
        .expect_err("short batch should fail");

        assert!(matches!(error, WorkerError::InvalidJob { .. }));
    }

    #[test]
    fn worker_error_round_trips_through_msgpack() {
        let error = WorkerError::Busy { message: "queue full".to_string() };

        let packed = rmp_serde::to_vec_named(&error).expect("pack worker error");
        let decoded: WorkerError = rmp_serde::from_slice(&packed).expect("decode worker error");

        assert_eq!(decoded, error);
    }

    #[test]
    fn worker_request_envelope_round_trips_through_msgpack() {
        let request = WorkerRequest::new(
            WorkerJob {
                id: 9,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"announce".to_vec() },
            },
            250,
        );

        let packed = request.encode().expect("pack worker request");
        let decoded = WorkerRequest::decode(&packed).expect("decode worker request");

        assert_eq!(decoded.protocol_version, WORKER_PROTOCOL_VERSION);
        assert_eq!(decoded.timeout_ms, 250);
        assert_eq!(decoded, request);
    }

    #[test]
    fn worker_response_envelope_round_trips_failures_through_msgpack() {
        let response =
            WorkerResponse::failure(10, WorkerError::TimedOut { message: "deadline".into() });

        let packed = response.encode().expect("pack worker response");
        let decoded = WorkerResponse::decode(&packed).expect("decode worker response");

        assert_eq!(decoded.protocol_version, WORKER_PROTOCOL_VERSION);
        assert_eq!(decoded.job_id, 10);
        assert_eq!(decoded, response);
    }

    #[test]
    fn worker_request_decode_rejects_unsupported_protocol_version() {
        let mut request = WorkerRequest::new(
            WorkerJob {
                id: 11,
                kind: WorkerJobKind::ValidateAnnounce { packet_wire: b"announce".to_vec() },
            },
            250,
        );
        request.protocol_version = WORKER_PROTOCOL_VERSION + 1;
        let packed = rmp_serde::to_vec_named(&request).expect("pack worker request");

        let err = WorkerRequest::decode(&packed).expect_err("version mismatch should fail");

        assert_eq!(
            err,
            WorkerCodecError::UnsupportedProtocolVersion {
                expected: WORKER_PROTOCOL_VERSION,
                actual: WORKER_PROTOCOL_VERSION + 1,
            }
        );
    }

    #[test]
    fn worker_response_encode_rejects_unsupported_protocol_version() {
        let mut response =
            WorkerResponse::failure(12, WorkerError::Cancelled { message: "cancelled".into() });
        response.protocol_version = WORKER_PROTOCOL_VERSION + 1;

        let err = response.encode().expect_err("version mismatch should fail");

        assert_eq!(
            err,
            WorkerCodecError::UnsupportedProtocolVersion {
                expected: WORKER_PROTOCOL_VERSION,
                actual: WORKER_PROTOCOL_VERSION + 1,
            }
        );
    }

    #[tokio::test]
    async fn worker_client_returns_successful_backend_result() {
        let client = WorkerClient::new(Arc::new(EchoBackend));

        let response = client.submit(announce_request(13, 100)).await;

        assert_eq!(
            response.outcome,
            Ok(WorkerResult {
                id: 13,
                kind: WorkerResultKind::PacketWire { packet_wire: b"ok".to_vec() },
            })
        );
    }

    #[tokio::test]
    async fn worker_client_converts_deadline_to_timeout_error() {
        let client = WorkerClient::new(Arc::new(SlowBackend));

        let response = client.submit(announce_request(14, 1)).await;

        assert!(matches!(response.outcome, Err(WorkerError::TimedOut { .. })));
    }

    #[tokio::test]
    async fn worker_client_rejects_mismatched_result_id() {
        let client = WorkerClient::new(Arc::new(MismatchedBackend));

        let response = client.submit(announce_request(15, 100)).await;

        assert!(matches!(response.outcome, Err(WorkerError::InvalidJob { .. })));
    }

    #[tokio::test]
    async fn worker_client_processes_encoded_request_bytes() {
        let client = WorkerClient::new(Arc::new(EchoBackend));
        let request = announce_request(16, 100).encode().expect("encode request");

        let response_bytes = client.submit_encoded(&request).await;
        let response = WorkerResponse::decode(&response_bytes).expect("decode response");

        assert_eq!(
            response.outcome,
            Ok(WorkerResult {
                id: 16,
                kind: WorkerResultKind::PacketWire { packet_wire: b"ok".to_vec() },
            })
        );
    }

    #[tokio::test]
    async fn worker_client_encoded_path_returns_invalid_job_for_bad_request_bytes() {
        let client = WorkerClient::new(Arc::new(EchoBackend));

        let response_bytes = client.submit_encoded(b"not msgpack").await;
        let response = WorkerResponse::decode(&response_bytes).expect("decode error response");

        assert_eq!(response.job_id, 0);
        assert!(matches!(response.outcome, Err(WorkerError::InvalidJob { .. })));
    }

    #[test]
    fn worker_request_decode_rejects_oversized_envelopes() {
        let oversized = vec![0u8; MAX_WORKER_REQUEST_BYTES + 1];

        let err = WorkerRequest::decode(&oversized).expect_err("oversized request should fail");

        assert_eq!(
            err,
            WorkerCodecError::MessageTooLarge {
                max_bytes: MAX_WORKER_REQUEST_BYTES,
                actual_bytes: MAX_WORKER_REQUEST_BYTES + 1,
            }
        );
    }

    #[test]
    fn worker_response_encode_rejects_oversized_envelopes() {
        let response = WorkerResponse::success(WorkerResult {
            id: 17,
            kind: WorkerResultKind::PacketWire {
                packet_wire: vec![0u8; MAX_WORKER_RESPONSE_BYTES + 1],
            },
        });

        let err = response.encode().expect_err("oversized response should fail");

        assert!(matches!(
            err,
            WorkerCodecError::MessageTooLarge { max_bytes: MAX_WORKER_RESPONSE_BYTES, .. }
        ));
    }

    #[test]
    fn worker_frame_round_trips_payload() {
        let payload = b"worker envelope";

        let frame = encode_worker_frame(payload, MAX_WORKER_REQUEST_BYTES).expect("encode frame");
        let decoded = decode_worker_frame(&frame, MAX_WORKER_REQUEST_BYTES).expect("decode frame");

        assert_eq!(decoded, payload);
    }

    #[test]
    fn worker_frame_decode_rejects_short_header() {
        let err = decode_worker_frame(&[0, 1], MAX_WORKER_REQUEST_BYTES)
            .expect_err("short header should fail");

        assert_eq!(
            err,
            WorkerCodecError::IncompleteFrame {
                expected_bytes: WORKER_FRAME_HEADER_BYTES,
                actual_bytes: 2,
            }
        );
    }

    #[test]
    fn worker_frame_decode_rejects_incomplete_payload() {
        let mut frame = Vec::new();
        frame.extend_from_slice(&8u32.to_be_bytes());
        frame.extend_from_slice(b"short");

        let err = decode_worker_frame(&frame, MAX_WORKER_REQUEST_BYTES)
            .expect_err("incomplete payload should fail");

        assert_eq!(
            err,
            WorkerCodecError::IncompleteFrame {
                expected_bytes: WORKER_FRAME_HEADER_BYTES + 8,
                actual_bytes: WORKER_FRAME_HEADER_BYTES + 5,
            }
        );
    }

    #[test]
    fn worker_frame_decode_rejects_oversized_payload_length() {
        let mut frame = Vec::new();
        frame.extend_from_slice(&((MAX_WORKER_REQUEST_BYTES + 1) as u32).to_be_bytes());

        let err = decode_worker_frame(&frame, MAX_WORKER_REQUEST_BYTES)
            .expect_err("oversized frame should fail");

        assert_eq!(
            err,
            WorkerCodecError::MessageTooLarge {
                max_bytes: MAX_WORKER_REQUEST_BYTES,
                actual_bytes: MAX_WORKER_REQUEST_BYTES + 1,
            }
        );
    }

    #[tokio::test]
    async fn worker_frame_async_io_round_trips_payload() {
        let (mut client, mut worker) = tokio::io::duplex(64);
        let payload = b"framed worker envelope".to_vec();
        let write_payload = payload.clone();

        let writer = tokio::spawn(async move {
            write_worker_frame(&mut client, &write_payload, MAX_WORKER_REQUEST_BYTES)
                .await
                .expect("write frame");
        });

        let decoded =
            read_worker_frame(&mut worker, MAX_WORKER_REQUEST_BYTES).await.expect("read frame");
        writer.await.expect("writer task");

        assert_eq!(decoded, payload);
    }

    #[tokio::test]
    async fn worker_frame_async_read_rejects_oversized_length_before_payload_alloc() {
        let (mut client, mut worker) = tokio::io::duplex(64);
        client
            .write_all(&((MAX_WORKER_REQUEST_BYTES + 1) as u32).to_be_bytes())
            .await
            .expect("write length");

        let err = read_worker_frame(&mut worker, MAX_WORKER_REQUEST_BYTES)
            .await
            .expect_err("oversized frame should fail");

        assert_eq!(
            err,
            WorkerCodecError::MessageTooLarge {
                max_bytes: MAX_WORKER_REQUEST_BYTES,
                actual_bytes: MAX_WORKER_REQUEST_BYTES + 1,
            }
        );
    }

    #[tokio::test]
    async fn worker_frame_handler_processes_one_framed_request_response() {
        let (mut caller, mut worker) = tokio::io::duplex(256);
        let service = WorkerClient::new(Arc::new(EchoBackend));
        let request = announce_request(18, 100).encode().expect("encode request");

        let worker_task = tokio::spawn(async move {
            handle_worker_frame(&mut worker, &service).await.expect("handle frame");
        });

        write_worker_frame(&mut caller, &request, MAX_WORKER_REQUEST_BYTES)
            .await
            .expect("write request");
        let response_bytes =
            read_worker_frame(&mut caller, MAX_WORKER_RESPONSE_BYTES).await.expect("read response");
        worker_task.await.expect("worker task");
        let response = WorkerResponse::decode(&response_bytes).expect("decode response");

        assert_eq!(
            response.outcome,
            Ok(WorkerResult {
                id: 18,
                kind: WorkerResultKind::PacketWire { packet_wire: b"ok".to_vec() },
            })
        );
    }

    #[tokio::test]
    async fn worker_frame_server_processes_until_eof() {
        let (mut caller, mut worker) = tokio::io::duplex(512);
        let service = WorkerClient::new(Arc::new(EchoBackend));

        let worker_task = tokio::spawn(async move {
            serve_worker_frames(&mut worker, &service).await.expect("serve frames")
        });

        for id in [19, 20] {
            let request = announce_request(id, 100).encode().expect("encode request");
            write_worker_frame(&mut caller, &request, MAX_WORKER_REQUEST_BYTES)
                .await
                .expect("write request");
            let response_bytes = read_worker_frame(&mut caller, MAX_WORKER_RESPONSE_BYTES)
                .await
                .expect("read response");
            let response = WorkerResponse::decode(&response_bytes).expect("decode response");
            assert_eq!(response.job_id, id);
            assert!(response.outcome.is_ok());
        }
        drop(caller);

        let summary = worker_task.await.expect("worker task");
        assert_eq!(
            summary,
            WorkerServeSummary { handled: 2, stop_reason: WorkerServeStopReason::Eof }
        );
    }

    #[tokio::test]
    async fn worker_frame_server_stops_on_cancellation() {
        let (mut caller, mut worker) = tokio::io::duplex(512);
        let service = WorkerClient::new(Arc::new(EchoBackend));
        let cancellation = CancellationToken::new();
        let worker_cancellation = cancellation.clone();

        let worker_task = tokio::spawn(async move {
            serve_worker_frames_until_cancelled(&mut worker, &service, worker_cancellation)
                .await
                .expect("serve frames")
        });

        let request = announce_request(21, 100).encode().expect("encode request");
        write_worker_frame(&mut caller, &request, MAX_WORKER_REQUEST_BYTES)
            .await
            .expect("write request");
        let response_bytes =
            read_worker_frame(&mut caller, MAX_WORKER_RESPONSE_BYTES).await.expect("read response");
        let response = WorkerResponse::decode(&response_bytes).expect("decode response");
        assert_eq!(response.job_id, 21);

        cancellation.cancel();
        let summary = worker_task.await.expect("worker task");
        assert_eq!(
            summary,
            WorkerServeSummary { handled: 1, stop_reason: WorkerServeStopReason::Cancelled }
        );
    }

    #[tokio::test]
    async fn worker_frame_server_can_cancel_before_first_frame() {
        let (_caller, mut worker) = tokio::io::duplex(512);
        let service = WorkerClient::new(Arc::new(EchoBackend));
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let summary = serve_worker_frames_until_cancelled(&mut worker, &service, cancellation)
            .await
            .expect("serve frames");

        assert_eq!(
            summary,
            WorkerServeSummary { handled: 0, stop_reason: WorkerServeStopReason::Cancelled }
        );
    }
}

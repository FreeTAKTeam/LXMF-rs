use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use super::worker_boundary::{
    submit_outbound_encrypt_batch, submit_single_destination_decrypt_batch,
    DestinationPayloadBatchItem, OutboundEncryptBatchItem, PacketWireBatchItem,
    SingleDestinationDecryptBatchItem, WorkerBackend, WorkerError,
};

pub(super) const CRYPTO_BATCH_LANE_CAPACITY: usize = 256;
pub(super) const CRYPTO_BATCH_MAX_ITEMS: usize = 64;

#[derive(Clone)]
pub(super) struct OutboundCryptoBatchLane {
    tx: mpsc::Sender<OutboundEncryptCommand>,
}

#[derive(Clone)]
pub(super) struct InboundCryptoBatchLane {
    tx: mpsc::Sender<SingleDestinationDecryptCommand>,
}

struct OutboundEncryptCommand {
    item: OutboundEncryptBatchItem,
    reply: oneshot::Sender<Result<PacketWireBatchItem, WorkerError>>,
}

struct SingleDestinationDecryptCommand {
    item: SingleDestinationDecryptBatchItem,
    reply: oneshot::Sender<Result<DestinationPayloadBatchItem, WorkerError>>,
}

impl OutboundCryptoBatchLane {
    pub fn spawn(backend: Arc<dyn WorkerBackend>) -> Self {
        Self::spawn_with_limits(backend, CRYPTO_BATCH_LANE_CAPACITY, CRYPTO_BATCH_MAX_ITEMS)
    }

    #[cfg(test)]
    pub(super) fn spawn_with_limits(
        backend: Arc<dyn WorkerBackend>,
        capacity: usize,
        max_batch_items: usize,
    ) -> Self {
        Self::spawn_inner(backend, capacity, max_batch_items)
    }

    #[cfg(not(test))]
    fn spawn_with_limits(
        backend: Arc<dyn WorkerBackend>,
        capacity: usize,
        max_batch_items: usize,
    ) -> Self {
        Self::spawn_inner(backend, capacity, max_batch_items)
    }

    fn spawn_inner(
        backend: Arc<dyn WorkerBackend>,
        capacity: usize,
        max_batch_items: usize,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel(capacity);
        tokio::spawn(async move {
            let mut next_job_id = 1u64;
            while let Some(first) = rx.recv().await {
                let mut batch = Vec::with_capacity(max_batch_items.max(1));
                batch.push(first);
                tokio::task::yield_now().await;
                while batch.len() < max_batch_items.max(1) {
                    match rx.try_recv() {
                        Ok(command) => batch.push(command),
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => break,
                    }
                }

                let job_id = next_job_id;
                next_job_id = next_job_id.saturating_add(1);
                submit_outbound_batch(backend.as_ref(), job_id, batch).await;
            }
        });
        Self { tx }
    }

    pub async fn encrypt(
        &self,
        item: OutboundEncryptBatchItem,
    ) -> Result<PacketWireBatchItem, WorkerError> {
        let (reply, rx) = oneshot::channel();
        self.tx.try_send(OutboundEncryptCommand { item, reply }).map_err(|err| match err {
            mpsc::error::TrySendError::Full(_) => {
                WorkerError::Busy { message: "outbound crypto batch lane is full".to_string() }
            }
            mpsc::error::TrySendError::Closed(_) => WorkerError::BackendUnavailable {
                message: "outbound crypto batch lane is closed".to_string(),
            },
        })?;
        rx.await.map_err(|_| WorkerError::BackendUnavailable {
            message: "outbound crypto batch lane worker stopped".to_string(),
        })?
    }
}

impl InboundCryptoBatchLane {
    pub fn spawn(backend: Arc<dyn WorkerBackend>) -> Self {
        Self::spawn_with_limits(backend, CRYPTO_BATCH_LANE_CAPACITY, CRYPTO_BATCH_MAX_ITEMS)
    }

    #[cfg(test)]
    pub(super) fn spawn_with_limits(
        backend: Arc<dyn WorkerBackend>,
        capacity: usize,
        max_batch_items: usize,
    ) -> Self {
        Self::spawn_inner(backend, capacity, max_batch_items)
    }

    #[cfg(not(test))]
    fn spawn_with_limits(
        backend: Arc<dyn WorkerBackend>,
        capacity: usize,
        max_batch_items: usize,
    ) -> Self {
        Self::spawn_inner(backend, capacity, max_batch_items)
    }

    fn spawn_inner(
        backend: Arc<dyn WorkerBackend>,
        capacity: usize,
        max_batch_items: usize,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel(capacity);
        tokio::spawn(async move {
            let mut next_job_id = 1u64;
            while let Some(first) = rx.recv().await {
                let mut batch = Vec::with_capacity(max_batch_items.max(1));
                batch.push(first);
                tokio::task::yield_now().await;
                while batch.len() < max_batch_items.max(1) {
                    match rx.try_recv() {
                        Ok(command) => batch.push(command),
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => break,
                    }
                }

                let job_id = next_job_id;
                next_job_id = next_job_id.saturating_add(1);
                submit_single_destination_decrypt_job_batch(backend.as_ref(), job_id, batch).await;
            }
        });
        Self { tx }
    }

    pub async fn decrypt(
        &self,
        item: SingleDestinationDecryptBatchItem,
    ) -> Result<DestinationPayloadBatchItem, WorkerError> {
        let (reply, rx) = oneshot::channel();
        self.tx.try_send(SingleDestinationDecryptCommand { item, reply }).map_err(
            |err| match err {
                mpsc::error::TrySendError::Full(_) => {
                    WorkerError::Busy { message: "inbound crypto batch lane is full".to_string() }
                }
                mpsc::error::TrySendError::Closed(_) => WorkerError::BackendUnavailable {
                    message: "inbound crypto batch lane is closed".to_string(),
                },
            },
        )?;
        rx.await.map_err(|_| WorkerError::BackendUnavailable {
            message: "inbound crypto batch lane worker stopped".to_string(),
        })?
    }
}

async fn submit_outbound_batch(
    backend: &dyn WorkerBackend,
    job_id: u64,
    batch: Vec<OutboundEncryptCommand>,
) {
    let expected_items = batch.len();
    let mut items = Vec::with_capacity(expected_items);
    let mut replies = Vec::with_capacity(expected_items);
    for command in batch {
        items.push(command.item);
        replies.push(command.reply);
    }

    match submit_outbound_encrypt_batch(backend, job_id, items).await {
        Ok(results) => {
            for (reply, result) in replies.into_iter().zip(results) {
                let _ = reply.send(Ok(result));
            }
        }
        Err(err) => {
            for reply in replies {
                let _ = reply.send(Err(err.clone()));
            }
        }
    }
}

async fn submit_single_destination_decrypt_job_batch(
    backend: &dyn WorkerBackend,
    job_id: u64,
    batch: Vec<SingleDestinationDecryptCommand>,
) {
    let expected_items = batch.len();
    let mut items = Vec::with_capacity(expected_items);
    let mut replies = Vec::with_capacity(expected_items);
    for command in batch {
        items.push(command.item);
        replies.push(command.reply);
    }

    match submit_single_destination_decrypt_batch(backend, job_id, items).await {
        Ok(results) => {
            for (reply, result) in replies.into_iter().zip(results) {
                let _ = reply.send(Ok(result));
            }
        }
        Err(err) => {
            for reply in replies {
                let _ = reply.send(Err(err.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::identity::PUBLIC_KEY_LENGTH;
    use crate::transport::worker_boundary::{
        WorkerJob, WorkerJobFuture, WorkerJobKind, WorkerResult, WorkerResultKind,
    };

    struct CapturingBatchBackend {
        calls: AtomicUsize,
        last_batch_len: AtomicUsize,
        truncate_to: Option<usize>,
    }

    impl CapturingBatchBackend {
        fn new(truncate_to: Option<usize>) -> Self {
            Self { calls: AtomicUsize::new(0), last_batch_len: AtomicUsize::new(0), truncate_to }
        }
    }

    impl WorkerBackend for CapturingBatchBackend {
        fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let truncate_to = self.truncate_to;
            Box::pin(async move {
                match job.kind {
                    WorkerJobKind::OutboundEncryptBatch { items } => {
                        self.last_batch_len.store(items.len(), Ordering::SeqCst);
                        let items = items
                            .into_iter()
                            .take(truncate_to.unwrap_or(usize::MAX))
                            .map(|item| PacketWireBatchItem { packet_wire: item.packet_wire })
                            .collect();
                        Ok(WorkerResult {
                            id: job.id,
                            kind: WorkerResultKind::PacketWireBatch { items },
                        })
                    }
                    WorkerJobKind::SingleDestinationDecryptBatch { items } => {
                        self.last_batch_len.store(items.len(), Ordering::SeqCst);
                        let items = items
                            .into_iter()
                            .take(truncate_to.unwrap_or(usize::MAX))
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

    fn encrypt_item(payload: &[u8]) -> OutboundEncryptBatchItem {
        OutboundEncryptBatchItem {
            packet_wire: payload.to_vec(),
            public_key: [0x11; PUBLIC_KEY_LENGTH],
            salt: [0x22; crate::hash::ADDRESS_HASH_SIZE],
        }
    }

    fn decrypt_item(payload: &[u8]) -> SingleDestinationDecryptBatchItem {
        SingleDestinationDecryptBatchItem {
            packet_wire: payload.to_vec(),
            destination: [0x33; crate::hash::ADDRESS_HASH_SIZE],
            private_key: serde_bytes::ByteBuf::from(payload.to_vec()),
        }
    }

    #[tokio::test]
    async fn outbound_crypto_batch_lane_coalesces_queued_jobs() {
        let backend = Arc::new(CapturingBatchBackend::new(None));
        let lane = OutboundCryptoBatchLane::spawn_with_limits(backend.clone(), 8, 4);

        let first = lane.encrypt(encrypt_item(b"packet-a"));
        let second = lane.encrypt(encrypt_item(b"packet-b"));
        let third = lane.encrypt(encrypt_item(b"packet-c"));
        let (first, second, third) = tokio::join!(first, second, third);

        assert_eq!(first.expect("first").packet_wire, b"packet-a");
        assert_eq!(second.expect("second").packet_wire, b"packet-b");
        assert_eq!(third.expect("third").packet_wire, b"packet-c");
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
        assert_eq!(backend.last_batch_len.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn outbound_crypto_batch_lane_rejects_when_queue_is_full() {
        let backend = Arc::new(CapturingBatchBackend::new(None));
        let lane = OutboundCryptoBatchLane::spawn_with_limits(backend, 1, 1);

        let _permit = lane.tx.reserve().await.expect("reserve queue slot");
        let err = lane.encrypt(encrypt_item(b"packet")).await.expect_err("full queue should fail");

        assert!(matches!(err, WorkerError::Busy { .. }));
    }

    #[tokio::test]
    async fn outbound_crypto_batch_lane_fans_out_batch_errors() {
        let backend = Arc::new(CapturingBatchBackend::new(Some(1)));
        let lane = OutboundCryptoBatchLane::spawn_with_limits(backend, 8, 4);

        let first = lane.encrypt(encrypt_item(b"packet-a"));
        let second = lane.encrypt(encrypt_item(b"packet-b"));
        let (first, second) = tokio::join!(first, second);

        assert!(matches!(first, Err(WorkerError::InvalidJob { .. })));
        assert!(matches!(second, Err(WorkerError::InvalidJob { .. })));
    }

    #[tokio::test]
    async fn inbound_crypto_batch_lane_coalesces_queued_jobs() {
        let backend = Arc::new(CapturingBatchBackend::new(None));
        let lane = InboundCryptoBatchLane::spawn_with_limits(backend.clone(), 8, 4);

        let first = lane.decrypt(decrypt_item(b"payload-a"));
        let second = lane.decrypt(decrypt_item(b"payload-b"));
        let third = lane.decrypt(decrypt_item(b"payload-c"));
        let (first, second, third) = tokio::join!(first, second, third);

        assert_eq!(first.expect("first").payload.as_ref(), b"payload-a");
        assert_eq!(second.expect("second").payload.as_ref(), b"payload-b");
        assert_eq!(third.expect("third").payload.as_ref(), b"payload-c");
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
        assert_eq!(backend.last_batch_len.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn inbound_crypto_batch_lane_rejects_when_queue_is_full() {
        let backend = Arc::new(CapturingBatchBackend::new(None));
        let lane = InboundCryptoBatchLane::spawn_with_limits(backend, 1, 1);

        let _permit = lane.tx.reserve().await.expect("reserve queue slot");
        let err = lane.decrypt(decrypt_item(b"payload")).await.expect_err("full queue should fail");

        assert!(matches!(err, WorkerError::Busy { .. }));
    }

    #[tokio::test]
    async fn inbound_crypto_batch_lane_fans_out_batch_errors() {
        let backend = Arc::new(CapturingBatchBackend::new(Some(1)));
        let lane = InboundCryptoBatchLane::spawn_with_limits(backend, 8, 4);

        let first = lane.decrypt(decrypt_item(b"payload-a"));
        let second = lane.decrypt(decrypt_item(b"payload-b"));
        let (first, second) = tokio::join!(first, second);

        assert!(matches!(first, Err(WorkerError::InvalidJob { .. })));
        assert!(matches!(second, Err(WorkerError::InvalidJob { .. })));
    }
}

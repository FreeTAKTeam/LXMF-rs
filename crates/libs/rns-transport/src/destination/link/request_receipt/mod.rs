use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

use crate::error::RnsError;
use crate::hash::{AddressHash, Hash};

mod book;
pub(super) use book::RequestBook;

/// Numeric phases match the frozen RNS RequestReceipt protocol surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestStatus {
    Failed = 0,
    Sent = 1,
    Delivered = 2,
    Receiving = 3,
    Ready = 4,
}

impl RequestStatus {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Failed | Self::Ready)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RequestFailure {
    #[error("request timed out")]
    Timeout,
    #[error("response was rejected")]
    Rejected,
    #[error("response envelope is invalid")]
    InvalidResponse,
    #[error("request or response transfer failed")]
    TransferFailed,
    #[error("request Link generation closed")]
    LinkClosed,
    #[error("request cancelled")]
    Cancelled,
    #[error("request handoff failed")]
    DispatchFailed,
    #[error("request outcome owner unavailable")]
    OwnerUnavailable,
}

/// Exact response bytes. Packet responses and ordinary Resource responses carry
/// the packed response envelope; metadata-bearing Resource responses may be raw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestResponse {
    pub data: Vec<u8>,
    pub metadata: Option<Vec<u8>>,
    pub is_resource: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestSnapshot {
    pub status: RequestStatus,
    pub failure: Option<RequestFailure>,
    pub response: Option<Arc<RequestResponse>>,
    /// PacketReceipt delivery, proved by a signed ACK or an authenticated response.
    pub packet_acknowledged: bool,
    pub sent_at: Option<Instant>,
    pub delivered_at: Option<Instant>,
    /// Native active-segment progress; split Resources can reset these counters.
    pub received_bytes: u64,
    pub total_bytes: u64,
}

impl RequestSnapshot {
    fn sent() -> Self {
        Self {
            status: RequestStatus::Sent,
            failure: None,
            response: None,
            packet_acknowledged: false,
            sent_at: None,
            delivered_at: None,
            received_bytes: 0,
            total_bytes: 0,
        }
    }
}

struct RequestIdentity {
    link_id: AddressHash,
    request_id: Vec<u8>,
}

/// Retained outcome for one exact Link generation and request registration.
/// Waiting does not consume a result or cancel work. A closed producer without a
/// terminal snapshot reports unavailable rather than manufacturing success.
#[derive(Clone)]
pub struct RequestReceipt {
    identity: Arc<RequestIdentity>,
    receiver: watch::Receiver<RequestSnapshot>,
}

impl std::fmt::Debug for RequestReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestReceipt")
            .field("link_id", &self.link_id())
            .field("request_id", &self.request_id())
            .field("status", &self.status())
            .finish()
    }
}

impl RequestReceipt {
    pub fn link_id(&self) -> AddressHash {
        self.identity.link_id
    }
    pub fn request_id(&self) -> &[u8] {
        &self.identity.request_id
    }
    pub fn current(&self) -> RequestSnapshot {
        self.receiver.borrow().clone()
    }
    pub fn status(&self) -> RequestStatus {
        self.receiver.borrow().status
    }

    pub async fn changed(&mut self) -> Result<RequestSnapshot, RequestFailure> {
        self.receiver.changed().await.map_err(|_| RequestFailure::OwnerUnavailable)?;
        Ok(self.current())
    }

    pub async fn wait(&mut self) -> Result<Arc<RequestResponse>, RequestFailure> {
        loop {
            let snapshot = self.current();
            match snapshot.status {
                RequestStatus::Ready => {
                    return snapshot.response.ok_or(RequestFailure::OwnerUnavailable)
                }
                RequestStatus::Failed => {
                    return Err(snapshot.failure.unwrap_or(RequestFailure::OwnerUnavailable))
                }
                _ => {
                    self.changed().await?;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RequestSource {
    Legacy,
    Packet(Hash),
    Resource(Hash),
}

pub(crate) struct RequestCleanup {
    pub link_id: AddressHash,
    pub request_id: Vec<u8>,
    pub outbound: Option<Hash>,
    pub inbound: HashSet<Hash>,
}

struct LiveRequest {
    identity: Arc<RequestIdentity>,
    source: RequestSource,
    timeout: Option<Duration>,
    limit: Option<usize>,
    response_hashes: HashSet<Hash>,
    sender: watch::Sender<RequestSnapshot>,
}

enum Record {
    Live(LiveRequest),
    /// Small per-generation ID fence. Final payload lives only in observers.
    Terminal,
}

/// Exists only while a registered send has not returned. Abandoning an await
/// seals its own registration without acquiring Link/handler locks or spawning.
pub(crate) struct RequestDispatchGuard {
    identity: Arc<RequestIdentity>,
    sender: watch::Sender<RequestSnapshot>,
    returned: bool,
}

impl RequestDispatchGuard {
    pub(crate) fn confirm_handoff(&self, accepted: bool) {
        self.sender.send_if_modified(|snapshot| {
            if snapshot.status.is_terminal() {
                return false;
            }
            if accepted {
                if snapshot.sent_at.is_some() {
                    return false;
                }
                snapshot.sent_at = Some(Instant::now());
            } else {
                snapshot.status = RequestStatus::Failed;
                snapshot.failure = Some(RequestFailure::DispatchFailed);
            }
            true
        });
    }

    pub(crate) fn returned(&mut self) {
        self.returned = true;
    }
}

impl Drop for RequestDispatchGuard {
    fn drop(&mut self) {
        if !self.returned {
            self.sender.send_if_modified(|snapshot| {
                if snapshot.status.is_terminal() {
                    return false;
                }
                snapshot.status = RequestStatus::Failed;
                snapshot.failure = Some(RequestFailure::Cancelled);
                true
            });
        }
    }
}

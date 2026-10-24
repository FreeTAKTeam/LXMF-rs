use crate::delivery::{self, forced_representation, transport_error};
use crate::owned_link_delivery::{send_owned, OwnedSend};
use crate::{DeliveryMethod, DeliveryRepresentation, InProcessBackendConfig, InProcessSendReport};
use lxmf_core::{decide_delivery, MessageMethod, TransportMethod, WireMessage};
use lxmf_propagation::{
    Error, MessageKind, OwnedStore, PreparedMessage, RecoveryLease, Status, StoreLimits,
};
use lxmf_sdk::{MessageId, SdkError, SendRequest};
use rns_transport::{destination::DestinationDesc, hash::AddressHash, packet::LXMF_MAX_PAYLOAD};
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// One signed wire and recipient transient throughout direct attempts and fallback.
pub struct PreparedOutgoing {
    wire: WireMessage,
    remote: DestinationDesc,
    message: PreparedMessage,
}
impl PreparedOutgoing {
    pub fn message_id(&self) -> Result<String, SdkError> {
        self.wire.try_message_id().map(hex::encode).map_err(wire_error)
    }
    pub fn destination(&self) -> String {
        self.remote.address_hash.to_hex_string()
    }
}

/// Native bearer overrides, shared by direct and relay recovery.
#[derive(Clone, Copy, Debug, Default)]
pub struct PropagationRouteLimits {
    pub max_packet_bytes: Option<usize>,
    pub link_connect_timeout: Option<Duration>,
}

/// Native bearer limits for one immutable propagation upload.
#[derive(Clone, Copy, Debug)]
pub struct PropagationUploadLimits {
    pub max_transfer_bytes: usize,
    pub route: PropagationRouteLimits,
}

/// Existing native transport/lifecycle and a single serialized custody owner.
/// The embedding runtime supplies events; this service starts no poller.
#[derive(Clone)]
pub struct LocalPropagation {
    config: Arc<InProcessBackendConfig>,
    store: Arc<Mutex<OwnedStore>>,
    owner: lxmf_core::identity::PrivateIdentity,
    transfers: Arc<Semaphore>,
}
impl LocalPropagation {
    pub fn open(
        config: InProcessBackendConfig,
        path: impl AsRef<Path>,
        limits: StoreLimits,
    ) -> Result<Self, Error> {
        let owner = lxmf_core::identity::PrivateIdentity::from_private_key_bytes(
            &config.identity.to_private_key_bytes(),
        )
        .map_err(|_| Error::InvalidState("invalid persisted identity"))?;
        let store = OwnedStore::open(path, owner.clone(), limits)?;
        Ok(Self {
            config: Arc::new(config),
            store: Arc::new(Mutex::new(store)),
            owner,
            transfers: Arc::new(Semaphore::new(1)),
        })
    }
    pub fn try_transfer(&self) -> Result<OwnedSemaphorePermit, SdkError> {
        self.transfers
            .clone()
            .try_acquire_owned()
            .map_err(|_| transport_error("propagation transfer already active"))
    }
    pub async fn prepare(&self, request: &SendRequest) -> Result<PreparedOutgoing, SdkError> {
        let destination = AddressHash::new_from_hex_string(request.destination.trim())
            .map_err(|_| transport_error("invalid destination"))?;
        let remote = delivery::resolve_destination(
            &self.config.transport,
            destination,
            "delivery",
            self.config.link_connect_timeout,
        )
        .await
        .map_err(|error| error.with_cause_code("UnresolvedRecipient"))?;
        let wire = delivery::encode_wire(
            &self.config.identity,
            self.config.source_destination,
            &remote,
            request,
        )?;
        let wire = WireMessage::unpack(&wire).map_err(wire_error)?;
        let recipient = lxmf_core::identity::Identity::new_from_slices(
            remote.identity.public_key_bytes(),
            remote.identity.verifying_key_bytes(),
        );
        let message = PreparedMessage::new(&wire, &self.owner, Some(&recipient))
            .and_then(|message| message.with_application_key(request.correlation_id.as_deref()))
            .map_err(store_error)?;
        Ok(PreparedOutgoing { wire, remote, message })
    }
    pub async fn send_direct(
        &self,
        prepared: &PreparedOutgoing,
        request: &SendRequest,
        timeout: Duration,
        permit: impl FnOnce() -> bool + Send + Sync + 'static,
    ) -> Result<InProcessSendReport, SdkError> {
        let admission = self.try_transfer()?;
        let wire = prepared.wire.pack().map_err(wire_error)?;
        // Establish a Link when necessary; no open Link does not imply an offline recipient.
        let decision = decide_delivery(
            TransportMethod::Direct,
            false,
            wire.len().saturating_sub(lxmf_core::constants::LXMF_OVERHEAD),
        )
        .map_err(wire_error)?;
        let representation =
            forced_representation(request, decision.method, decision.representation, wire.len());
        send_owned(OwnedSend {
            starting: Some(Box::new(move || {
                if permit() {
                    Ok(())
                } else {
                    Err(transport_error("outgoing message permission revoked"))
                }
            })),
            config: self.config.clone(),
            destination: prepared.remote,
            payload: wire,
            message_id: MessageId(prepared.message_id()?),
            method: DeliveryMethod::Direct,
            representation,
            timeout,
            link_connect_timeout: delivery::request_link_timeout(
                request,
                self.config.link_connect_timeout,
            ),
            link_connect_attempts: delivery::request_link_attempts(
                request,
                self.config.link_connect_attempts,
            ),
            propagated_recipient: None,
            admission: Box::new(admission),
            completed: None,
        })
        .await
    }
    pub fn deposit_own(
        &self,
        prepared: PreparedOutgoing,
        kind: MessageKind,
        now: i64,
        earlier_expiry: Option<i64>,
    ) -> Result<Status, Error> {
        self.lock()?.deposit_own(prepared.message, kind, now, earlier_expiry)
    }
    pub fn application_messages(&self, key: &str) -> Result<Vec<Status>, Error> {
        self.lock()?.application_messages(key)
    }
    pub fn message(&self, id: &str) -> Result<Option<Status>, Error> {
        self.lock()?.message(id)
    }
    pub fn status(&self, now: i64) -> Result<Vec<Status>, Error> {
        self.lock()?.status(now)
    }
    pub fn acknowledge_status(&self, id: &str) -> Result<(), Error> {
        self.lock()?.acknowledge_status(id)
    }
    pub fn delivered(
        &self,
        id: &str,
        message_id: &str,
        acknowledged: bool,
        now: i64,
    ) -> Result<bool, Error> {
        self.lock()?.delivered(id, message_id, acknowledged, now)
    }
    pub fn stop(&self) -> Result<(), Error> {
        self.transfers.close();
        self.lock()?.stop();
        Ok(())
    }

    pub async fn recover_direct(
        &self,
        id: &str,
        now: i64,
        retry_at: i64,
        limits: PropagationRouteLimits,
        mut permit: impl FnMut(&Status, &[u8]) -> bool + Send + Sync + 'static,
    ) -> Result<Option<InProcessSendReport>, SdkError> {
        let admission = self.try_transfer()?;
        let started = std::time::Instant::now();
        let Some(recovery) = self
            .lock()
            .map_err(store_error)?
            .recover(id, now, retry_at, |_| true)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        let lease = LeaseGuard { store: self.store.clone(), lease: recovery.lease };
        if !permit(&recovery.status, &recovery.direct_wire) {
            return Ok(None);
        }
        let hash = AddressHash::new_from_hex_string(&recovery.status.destination)
            .map_err(|_| transport_error("invalid stored destination"))?;
        let remote = delivery::resolve_destination(
            &self.config.transport,
            hash,
            "delivery",
            limits.link_connect_timeout.unwrap_or(self.config.link_connect_timeout),
        )
        .await?;
        let policy_status = recovery.status.clone();
        let policy_wire = recovery.direct_wire.clone();
        let starting = Box::new(move || {
            if permit(&policy_status, &policy_wire) {
                Ok(())
            } else {
                Err(transport_error("stored message permission revoked"))
            }
        });
        let representation = representation(&recovery.direct_wire, limits.max_packet_bytes);
        let store = self.store.clone();
        let transient = id.to_owned();
        let message = recovery.status.message_id.clone();
        let completion_lease = lease.lease.clone();
        let completed = Box::new(move |result: &Result<InProcessSendReport, SdkError>| {
            if result
                .as_ref()
                .is_ok_and(|report| report.representation == DeliveryRepresentation::Resource)
            {
                store
                    .lock()
                    .map_err(|_| transport_error("custody lock poisoned"))?
                    .delivered(
                        &transient,
                        &message,
                        false,
                        now.saturating_add(started.elapsed().as_secs().min(i64::MAX as u64) as i64),
                    )
                    .map_err(store_error)?;
            }
            store
                .lock()
                .map_err(|_| transport_error("custody lock poisoned"))?
                .finish_recovery(&completion_lease)
                .map_err(store_error)
        });
        send_owned(OwnedSend {
            starting: Some(starting),
            config: self.config.clone(),
            destination: remote,
            payload: recovery.direct_wire,
            message_id: MessageId(recovery.status.message_id),
            method: DeliveryMethod::Direct,
            representation,
            timeout: limits.link_connect_timeout.unwrap_or(self.config.link_connect_timeout)
                + self.config.resource_transfer_timeout,
            link_connect_timeout: limits
                .link_connect_timeout
                .unwrap_or(self.config.link_connect_timeout),
            link_connect_attempts: self.config.link_connect_attempts,
            propagated_recipient: None,
            admission: Box::new((admission, lease)),
            completed: Some(completed),
        })
        .await
        .map(Some)
    }

    pub async fn recover_upload(
        &self,
        id: &str,
        relay: AddressHash,
        now: i64,
        retry_at: i64,
        limits: PropagationUploadLimits,
        mut permit: impl FnMut(&Status, &[u8]) -> bool + Send + Sync + 'static,
    ) -> Result<Option<InProcessSendReport>, SdkError> {
        let admission = self.try_transfer()?;
        let started = std::time::Instant::now();
        let Some(recovery) = self
            .lock()
            .map_err(store_error)?
            .recover(id, now, retry_at, |_| true)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        let lease = LeaseGuard { store: self.store.clone(), lease: recovery.lease };
        if recovery.status.uploaded || !permit(&recovery.status, &recovery.direct_wire) {
            return Ok(None);
        }
        // Budget check precedes path discovery and expensive proof-of-work.
        if recovery.recipient_transient.len().saturating_add(64) > limits.max_transfer_bytes {
            return Err(transport_error("propagation transfer budget exceeded"));
        }
        let remote = delivery::resolve_destination(
            &self.config.transport,
            relay,
            "propagation",
            limits.route.link_connect_timeout.unwrap_or(self.config.link_connect_timeout),
        )
        .await?;
        let transient = hex::decode(&recovery.status.transient_id)
            .map_err(|_| transport_error("invalid transient id"))?;
        let transient: [u8; 32] =
            transient.try_into().map_err(|_| transport_error("invalid transient length"))?;
        let stamp = tokio::task::spawn_blocking(move || {
            lxmf_core::stamp::generate_propagation_stamp(
                &transient,
                lxmf_core::stamp::DEFAULT_PROPAGATION_STAMP_COST,
            )
        })
        .await
        .map_err(|error| transport_error(format!("propagation stamp worker: {error}")))?
        .ok_or_else(|| transport_error("propagation stamp generation exhausted"))?;
        let envelope = WireMessage::pack_propagation_envelope(
            now as f64,
            &recovery.recipient_transient,
            Some(&stamp),
        )
        .map_err(wire_error)?;
        if envelope.len() > limits.max_transfer_bytes {
            return Err(transport_error("propagation transfer budget exceeded"));
        }
        let policy_status = recovery.status.clone();
        let policy_wire = recovery.direct_wire.clone();
        let store = self.store.clone();
        let upload_lease = lease.lease.clone();
        let starting = Box::new(move || {
            if !permit(&policy_status, &policy_wire) {
                return Err(transport_error("stored message permission revoked"));
            }
            let actual_now =
                now.saturating_add(started.elapsed().as_secs().min(i64::MAX as u64) as i64);
            store
                .lock()
                .map_err(|_| transport_error("custody lock poisoned"))?
                .begin_upload(&upload_lease, &relay.to_hex_string(), actual_now)
                .map_err(store_error)
        });
        let representation = representation(&envelope, limits.route.max_packet_bytes);
        let completion_store = self.store.clone();
        let completion_lease = lease.lease.clone();
        let completed = Box::new(move |_: &Result<InProcessSendReport, SdkError>| {
            completion_store
                .lock()
                .map_err(|_| transport_error("custody lock poisoned"))?
                .finish_recovery(&completion_lease)
                .map_err(store_error)
        });
        // The retained owner normalizes Uploading to unverified before releasing its lease,
        // including when its caller is cancelled after native dispatch.
        send_owned(OwnedSend {
            starting: Some(starting),
            config: self.config.clone(),
            destination: remote,
            payload: envelope,
            message_id: MessageId(recovery.status.message_id),
            method: DeliveryMethod::Propagated,
            representation,
            timeout: limits.route.link_connect_timeout.unwrap_or(self.config.link_connect_timeout)
                + self.config.resource_transfer_timeout,
            link_connect_timeout: limits
                .route
                .link_connect_timeout
                .unwrap_or(self.config.link_connect_timeout),
            link_connect_attempts: self.config.link_connect_attempts,
            propagated_recipient: Some(recovery.status.destination),
            admission: Box::new((admission, lease)),
            completed: Some(completed),
        })
        .await
        .map(Some)
    }
    fn lock(&self) -> Result<MutexGuard<'_, OwnedStore>, Error> {
        self.store.lock().map_err(|_| Error::InvalidState("propagation store lock poisoned"))
    }
}
struct LeaseGuard {
    store: Arc<Mutex<OwnedStore>>,
    lease: RecoveryLease,
}
impl Drop for LeaseGuard {
    fn drop(&mut self) {
        if let Ok(mut store) = self.store.lock() {
            if let Err(error) = store.finish_recovery(&self.lease) {
                log::error!("[local-propagation] recovery finalization deferred: {error}");
            }
        }
    }
}
fn representation(bytes: &[u8], max_packet_bytes: Option<usize>) -> MessageMethod {
    if bytes.len() > max_packet_bytes.unwrap_or(LXMF_MAX_PAYLOAD) {
        MessageMethod::Resource
    } else {
        MessageMethod::Packet
    }
}
fn wire_error(error: lxmf_core::LxmfError) -> SdkError {
    transport_error(error.to_string())
}
fn store_error(error: Error) -> SdkError {
    let cause = match &error {
        Error::UnresolvedRecipient => "UnresolvedRecipient",
        Error::PayloadTooLarge => "PayloadTooLarge",
        Error::StoreFull => "StoreFull",
        Error::Expired => "Expired",
        _ => "LocalPropagation",
    };
    transport_error(error.to_string()).with_cause_code(cause)
}
#[cfg(test)]
#[path = "propagation_tests.rs"]
mod tests;

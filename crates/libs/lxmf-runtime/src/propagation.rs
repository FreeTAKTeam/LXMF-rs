use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use lxmf_core::{decide_delivery, MessageMethod, TransportMethod, WireMessage};
use lxmf_propagation::{
    Error, MessageKind, OwnedStore, PreparedMessage, RecoveryLease, Status, StoreLimits,
};
use lxmf_sdk::{MessageId, SdkError, SendRequest};
use rns_transport::destination::DestinationDesc;
use rns_transport::hash::AddressHash;
use rns_transport::packet::LXMF_MAX_PAYLOAD;

use crate::delivery::{self, forced_representation, transport_error};
use crate::link_delivery::send_link_payload;
use crate::{DeliveryMethod, InProcessBackendConfig, InProcessSendReport};

/// One preparation is used throughout the bounded direct attempt and fallback.
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

/// Existing native transport and lifecycle, plus one serialized durable store.
/// Recovery is event driven by the embedding runtime; no additional poller is started.
#[derive(Clone)]
pub struct LocalPropagation {
    config: Arc<InProcessBackendConfig>,
    store: Arc<Mutex<OwnedStore>>,
    owner: lxmf_core::identity::PrivateIdentity,
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
        Ok(Self { config: Arc::new(config), store: Arc::new(Mutex::new(store)), owner })
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
        let message =
            PreparedMessage::new(&wire, &self.owner, Some(&recipient)).map_err(store_error)?;
        Ok(PreparedOutgoing { wire, remote, message })
    }

    pub async fn send_direct(
        &self,
        prepared: &PreparedOutgoing,
        request: &SendRequest,
        timeout: Duration,
    ) -> Result<InProcessSendReport, SdkError> {
        let wire = prepared.wire.pack().map_err(wire_error)?;
        let method =
            if self.config.transport.delivery_link_available(&prepared.remote.address_hash).await {
                TransportMethod::Direct
            } else {
                TransportMethod::Opportunistic
            };
        let decision = decide_delivery(
            method,
            false,
            wire.len().saturating_sub(lxmf_core::constants::LXMF_OVERHEAD),
        )
        .map_err(wire_error)?;
        let id = MessageId(prepared.message_id()?);
        if decision.method == TransportMethod::Opportunistic {
            return delivery::send_opportunistic(
                &self.config.transport,
                prepared.remote.address_hash,
                &wire,
                id,
            )
            .await;
        }
        let representation =
            forced_representation(request, decision.method, decision.representation, wire.len());
        send_link_payload(
            &self.config.transport,
            prepared.remote,
            &wire,
            id,
            DeliveryMethod::Direct,
            representation,
            timeout,
            1,
            self.config.resource_transfer_timeout,
            None,
        )
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
        self.lock()?.stop();
        Ok(())
    }

    pub async fn recover_direct(
        &self,
        id: &str,
        now: i64,
        retry_at: i64,
        mut permit: impl FnMut(&Status) -> bool,
    ) -> Result<Option<InProcessSendReport>, SdkError> {
        let Some(recovery) = self
            .lock()
            .map_err(store_error)?
            .recover(id, now, retry_at, |status| permit(status))
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        let _lease = LeaseGuard { store: self.store.clone(), lease: recovery.lease };
        let hash = AddressHash::new_from_hex_string(&recovery.status.destination)
            .map_err(|_| transport_error("invalid stored destination"))?;
        let remote = delivery::resolve_destination(
            &self.config.transport,
            hash,
            "delivery",
            self.config.link_connect_timeout,
        )
        .await?;
        let representation = if recovery.direct_wire.len() > LXMF_MAX_PAYLOAD {
            MessageMethod::Resource
        } else {
            MessageMethod::Packet
        };
        if !permit(&recovery.status) {
            return Ok(None);
        }
        let started = std::time::Instant::now();
        let report = send_link_payload(
            &self.config.transport,
            remote,
            &recovery.direct_wire,
            MessageId(recovery.status.message_id),
            DeliveryMethod::Direct,
            representation,
            self.config.link_connect_timeout,
            1,
            self.config.resource_transfer_timeout,
            None,
        )
        .await?;
        if report.representation == crate::DeliveryRepresentation::Resource {
            self.lock()
                .map_err(store_error)?
                .delivered(
                    id,
                    &report.message_id.0,
                    false,
                    now.saturating_add(started.elapsed().as_secs().min(i64::MAX as u64) as i64),
                )
                .map_err(store_error)?;
        }
        // Packet dispatch still waits for the validated recipient receipt listener.
        Ok(Some(report))
    }

    pub async fn recover_upload(
        &self,
        id: &str,
        relay: AddressHash,
        now: i64,
        retry_at: i64,
        max_transfer_bytes: usize,
        mut permit: impl FnMut(&Status) -> bool,
    ) -> Result<Option<InProcessSendReport>, SdkError> {
        let Some(recovery) = self
            .lock()
            .map_err(store_error)?
            .recover(id, now, retry_at, |status| permit(status))
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        let guard = LeaseGuard { store: self.store.clone(), lease: recovery.lease };
        if recovery.status.uploaded {
            return Ok(None);
        }
        let remote = delivery::resolve_destination(
            &self.config.transport,
            relay,
            "propagation",
            self.config.link_connect_timeout,
        )
        .await?;
        let transient_id = hex::decode(&recovery.status.transient_id)
            .map_err(|_| transport_error("invalid transient id"))?;
        let transient_id: [u8; 32] =
            transient_id.try_into().map_err(|_| transport_error("invalid transient length"))?;
        let stamp = tokio::task::spawn_blocking(move || {
            lxmf_core::stamp::generate_propagation_stamp(
                &transient_id,
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
        if envelope.len() > max_transfer_bytes {
            return Err(transport_error("propagation transfer budget exceeded"));
        }
        if !permit(&recovery.status) {
            return Ok(None);
        }
        self.lock()
            .map_err(store_error)?
            .begin_upload(&guard.lease, &relay.to_hex_string(), now)
            .map_err(store_error)?;
        let representation = if envelope.len() > LXMF_MAX_PAYLOAD {
            MessageMethod::Resource
        } else {
            MessageMethod::Packet
        };
        let result = send_link_payload(
            &self.config.transport,
            remote,
            &envelope,
            MessageId(recovery.status.message_id),
            DeliveryMethod::Propagated,
            representation,
            self.config.link_connect_timeout,
            1,
            self.config.resource_transfer_timeout,
            Some(recovery.status.destination),
        )
        .await;
        // Failure after dispatch is ambiguous too; retain custody without an upload loop.
        self.lock()
            .map_err(store_error)?
            .uploaded_unverified(&guard.lease, now)
            .map_err(store_error)?;
        result.map(Some)
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
            store.finish_recovery(&self.lease)
        }
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

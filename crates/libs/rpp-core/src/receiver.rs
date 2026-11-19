//! Receiver engine: pulls one Package over its canonical Destination.

use std::collections::{HashMap, HashSet};

use rand_core::{OsRng, RngCore};

use crate::control::{
    AcceptDecision, Body, Control, Header, ReceiptStatus, RequestId, RppResult, Status,
    MAX_BUNDLE_BYTES, MAX_WINDOW,
};
use crate::error::{Result, RppError};
use crate::identity::{sha256, Digest, PublicIdentity};
use crate::manifest::{OwnerBinding, VerifiedManifest, MAX_SIGNED_MANIFEST};
use crate::publication::Publication;
use crate::selection::{Selection, MAX_SELECTION_ENTRIES};
use crate::store::ChunkStore;

#[derive(Debug, Clone, Copy)]
pub struct PullConfig {
    pub max_manifest_bytes: u64,
    pub max_bundle: u64,
    pub window: u64,
    pub request_lifetime: u64,
}

impl Default for PullConfig {
    fn default() -> Self {
        Self {
            max_manifest_bytes: MAX_SIGNED_MANIFEST as u64,
            max_bundle: MAX_BUNDLE_BYTES,
            window: MAX_WINDOW,
            request_lifetime: 120,
        }
    }
}

/// Readable transfer state (RPP §14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub has_manifest: bool,
    pub verified_chunks: u32,
    pub total_chunks: Option<u32>,
    pub complete_verified: bool,
    pub receipt_acknowledged: bool,
}

enum Pending {
    Manifest,
    Missing { selected: Vec<u32>, max_bundle: u64, request: Header },
    Accept { plan_id: RequestId },
    Receipt,
}

enum ExpectedKind {
    Manifest { max: u64 },
    Bundle { indexes: Vec<u32>, payload_len: u64, digest: Digest },
}

/// A reserved inbound Resource. Its bytes stay quarantined until the
/// matching request was APPLIED, since the Resource may race the response.
struct Expected {
    kind: ExpectedKind,
    applied: bool,
    quarantined: Option<Vec<u8>>,
}

pub struct Pull<S: ChunkStore> {
    store: S,
    inbox: crate::Destination,
    binding: OwnerBinding,
    publication: Publication,
    config: PullConfig,
    manifest: Option<VerifiedManifest>,
    present: Vec<bool>,
    in_flight: HashSet<u32>,
    pending: HashMap<RequestId, Pending>,
    expected: HashMap<RequestId, Expected>,
    /// Digests of Resources already committed, for idempotent duplicates.
    committed: HashMap<RequestId, Digest>,
    complete_verified: bool,
    receipt_acknowledged: bool,
}

impl<S: ChunkStore> Pull<S> {
    /// `publication` is the owner-signed Package or Manifest reference the
    /// caller verified (see [`Publication::verify`]) for `binding`.
    pub fn new(
        mut store: S,
        me: &PublicIdentity,
        binding: OwnerBinding,
        publication: Publication,
        config: PullConfig,
    ) -> Result<Self> {
        if publication.package_destination != binding.package_destination
            || publication.manifest_destination != binding.manifest_destination
        {
            return Err(RppError::InvalidPublication("reference is for another object"));
        }
        let mut manifest = None;
        if let Some(bytes) = store.load_manifest(&publication.manifest_hash)? {
            let verified = VerifiedManifest::verify(&bytes, &binding)
                .and_then(|verified| publication.check_manifest(&verified).map(|()| verified));
            match verified {
                Ok(verified) => manifest = Some(verified),
                // Cache failed verification: fetch the manifest again.
                Err(error) => {
                    log::warn!("rpp: discarding cached manifest: {error}");
                    store.discard_manifest(&publication.manifest_hash)?;
                }
            }
        }
        let mut pull = Self {
            store,
            inbox: me.receive_destination(),
            binding,
            publication,
            config,
            manifest: None,
            present: Vec::new(),
            in_flight: HashSet::new(),
            pending: HashMap::new(),
            expected: HashMap::new(),
            committed: HashMap::new(),
            complete_verified: false,
            receipt_acknowledged: false,
        };
        if let Some(manifest) = manifest {
            pull.adopt(manifest)?;
        }
        Ok(pull)
    }

    fn adopt(&mut self, manifest: VerifiedManifest) -> Result<()> {
        self.complete_verified = self.store.is_promoted(&manifest.hash);
        self.present = if self.complete_verified {
            vec![true; manifest.manifest.chunks.len()]
        } else {
            self.store.verified_chunks(&manifest)?
        };
        self.manifest = Some(manifest);
        Ok(())
    }

    pub fn progress(&self) -> Progress {
        Progress {
            has_manifest: self.manifest.is_some(),
            verified_chunks: self.present.iter().filter(|present| **present).count() as u32,
            total_chunks: self.manifest.as_ref().map(VerifiedManifest::chunk_count),
            complete_verified: self.complete_verified,
            receipt_acknowledged: self.receipt_acknowledged,
        }
    }

    pub fn manifest(&self) -> Option<&VerifiedManifest> {
        self.manifest.as_ref()
    }

    fn header(&self, now: u64) -> Header {
        let mut request_id = [0u8; 16];
        OsRng.fill_bytes(&mut request_id);
        Header {
            package_id: self.publication.package_id,
            manifest_hash: self.publication.manifest_hash,
            package_destination: self.binding.package_destination,
            recipient: self.inbox,
            issued: now,
            expires: now + self.config.request_lifetime,
            request_id,
        }
    }

    fn reserve(&mut self, id: RequestId, kind: ExpectedKind) {
        self.expected.insert(id, Expected { kind, applied: false, quarantined: None });
    }

    /// Marks a reservation APPLIED and processes bytes that arrived early.
    fn applied(&mut self, id: RequestId) -> Result<()> {
        let Some(expected) = self.expected.get_mut(&id) else { return Ok(()) };
        expected.applied = true;
        match expected.quarantined.take() {
            Some(payload) => self.on_resource(&id, &payload),
            None => Ok(()),
        }
    }

    /// Drops a refused reservation and frees its chunk indexes.
    fn release(&mut self, id: &RequestId) {
        if let Some(Expected { kind: ExpectedKind::Bundle { indexes, .. }, .. }) =
            self.expected.remove(id)
        {
            indexes.iter().for_each(|index| {
                self.in_flight.remove(index);
            });
        }
    }

    fn request(
        &mut self,
        body: Body,
        pending: impl FnOnce(Header) -> Pending,
        now: u64,
    ) -> Vec<u8> {
        let header = self.header(now);
        self.pending.insert(header.request_id, pending(header));
        Control { header, body }.encode()
    }

    /// Requests to send now on the Package Link (path `/rpp/3`).
    pub fn next_requests(&mut self, now: u64) -> Result<Vec<Vec<u8>>> {
        if self.receipt_acknowledged || self.pending.values().any(|p| matches!(p, Pending::Receipt))
        {
            return Ok(Vec::new());
        }
        let Some(manifest) = &self.manifest else {
            if self.pending.values().any(|p| matches!(p, Pending::Manifest))
                || self.expected.values().any(|e| matches!(e.kind, ExpectedKind::Manifest { .. }))
            {
                return Ok(Vec::new());
            }
            let max = self.config.max_manifest_bytes;
            let header = self.header(now);
            // Capacity is reserved before the request leaves (RC1).
            self.reserve(header.request_id, ExpectedKind::Manifest { max });
            self.pending.insert(header.request_id, Pending::Manifest);
            let body = Body::ManifestRequest { max_bytes: max, capability: None };
            return Ok(vec![Control { header, body }.encode()]);
        };
        let total = manifest.chunk_count();
        let missing: Vec<u32> = (0..total)
            .filter(|i| !self.present[*i as usize] && !self.in_flight.contains(i))
            .collect();
        if missing.is_empty() {
            if self.in_flight.is_empty() && self.expected.is_empty() {
                return Ok(vec![self.receipt(now)?]);
            }
            return Ok(Vec::new());
        }
        let busy = self.expected.len()
            + self.pending.values().filter(|p| matches!(p, Pending::Missing { .. })).count();
        if busy as u64 >= self.config.window {
            return Ok(Vec::new());
        }
        let selection = Selection::from_sorted(missing, MAX_SELECTION_ENTRIES);
        let selected: Vec<u32> = selection.indexes().collect();
        self.in_flight.extend(selected.iter().copied());
        let max_bundle = self.config.max_bundle;
        let body = Body::MissingChunks {
            selection,
            max_bundle,
            window: self.config.window,
            capability: None,
        };
        Ok(vec![self.request(
            body,
            |request| Pending::Missing { selected, max_bundle, request },
            now,
        )])
    }

    /// Promotes the verified package and builds the TransferReceipt. A
    /// receipt can only say `complete_verified` after promotion succeeded.
    fn receipt(&mut self, now: u64) -> Result<Vec<u8>> {
        let manifest = self.manifest.as_ref().ok_or(RppError::Incomplete)?;
        let verified = self.present.iter().filter(|present| **present).count() as u64;
        if verified != u64::from(manifest.chunk_count()) {
            return Err(RppError::Incomplete);
        }
        let expired = manifest.manifest.expires.is_some_and(|expires| expires <= now);
        let status = if expired {
            ReceiptStatus::RejectedAfterVerification
        } else if self.complete_verified {
            ReceiptStatus::CompleteVerified
        } else {
            match self.store.promote(manifest) {
                Ok(()) => {
                    self.complete_verified = true;
                    ReceiptStatus::CompleteVerified
                }
                Err(error) => {
                    log::warn!("rpp: promotion failed: {error}");
                    ReceiptStatus::RejectedAfterVerification
                }
            }
        };
        let body = Body::TransferReceipt {
            status,
            verified_count: verified,
            missing: Selection::default(),
            capability: None,
        };
        Ok(self.request(body, |_| Pending::Receipt, now))
    }

    /// Handles a Link response; returns follow-up requests (ResourceAccept).
    pub fn on_response(&mut self, response: &[u8], now: u64) -> Result<Vec<Vec<u8>>> {
        let result = RppResult::decode(response)?;
        let Some(pending) = self.pending.remove(&result.request_id) else {
            return Err(RppError::Malformed("response to unknown request"));
        };
        let refused = |status: Status| match status {
            Status::NotAuthorized => RppError::NotAuthorized,
            Status::NotFound => RppError::NotFound,
            Status::Error => RppError::Storage("publisher error".into()),
            _ => RppError::LimitExceeded,
        };
        match pending {
            Pending::Manifest if result.status == Status::Applied => {
                self.applied(result.request_id)?;
                Ok(Vec::new())
            }
            Pending::Manifest => {
                self.expected.remove(&result.request_id);
                Err(refused(result.status))
            }
            Pending::Missing { selected, max_bundle, request } => {
                selected.iter().for_each(|index| {
                    self.in_flight.remove(index);
                });
                if result.status != Status::Applied {
                    return Err(refused(result.status));
                }
                let Some(reply) = result.value else { return Ok(Vec::new()) };
                let (plan_id, indexes, payload_len, digest) =
                    self.check_plan(&reply, &request, &selected, max_bundle)?;
                self.in_flight.extend(indexes.iter().copied());
                self.reserve(plan_id, ExpectedKind::Bundle { indexes, payload_len, digest });
                let body = Body::ResourceAccept {
                    plan_id,
                    decision: AcceptDecision::Accept,
                    capability: None,
                };
                Ok(vec![self.request(body, |_| Pending::Accept { plan_id }, now)])
            }
            Pending::Receipt if result.status == Status::Applied => {
                self.receipt_acknowledged = true;
                Ok(Vec::new())
            }
            Pending::Accept { plan_id } if result.status == Status::Applied => {
                self.applied(plan_id)?;
                Ok(Vec::new())
            }
            Pending::Accept { plan_id } => {
                self.release(&plan_id);
                Err(refused(result.status))
            }
            Pending::Receipt => Err(refused(result.status)),
        }
    }

    fn check_plan(
        &self,
        reply: &[u8],
        request: &Header,
        selected: &[u32],
        max_bundle: u64,
    ) -> Result<(RequestId, Vec<u32>, u64, Digest)> {
        let manifest = self.manifest.as_ref().ok_or(RppError::NoAcceptedPlan)?;
        let plan = Control::decode(reply).map_err(|error| error.error)?;
        let Body::ResourcePlan { indexes, payload_len, bundle_digest, plan_id } = plan.body else {
            return Err(RppError::Malformed("MissingChunks reply is not a ResourcePlan"));
        };
        if plan.header != *request || plan_id != request.request_id {
            return Err(RppError::Malformed("plan does not echo its request"));
        }
        if indexes.iter().any(|index| !selected.contains(index)) {
            return Err(RppError::InvalidSelection);
        }
        if payload_len > max_bundle {
            return Err(RppError::LimitExceeded);
        }
        let signed: u64 = indexes.iter().map(|i| manifest.manifest.chunks[*i as usize].len).sum();
        if signed != payload_len {
            return Err(RppError::ChunkMismatch);
        }
        Ok((plan_id, indexes, payload_len, bundle_digest))
    }

    /// Admission check before an inbound Resource is accepted: metadata must
    /// name an accepted plan or applied ManifestRequest with room for `size`.
    pub fn admits_resource(&self, metadata: &RequestId, size: u64) -> bool {
        match self.expected.get(metadata).map(|expected| &expected.kind) {
            Some(ExpectedKind::Manifest { max }) => size <= *max,
            Some(ExpectedKind::Bundle { payload_len, .. }) => size == *payload_len,
            None => false,
        }
    }

    /// Verifies and stores a completed native Resource.
    pub fn on_resource(&mut self, metadata: &RequestId, payload: &[u8]) -> Result<()> {
        if let Some(expected) = self.expected.get_mut(metadata) {
            if !expected.applied {
                if !matches!(&expected.kind, ExpectedKind::Manifest { max } if payload.len() as u64 <= *max)
                    && !matches!(&expected.kind, ExpectedKind::Bundle { payload_len, .. } if payload.len() as u64 == *payload_len)
                {
                    return Err(RppError::LimitExceeded);
                }
                expected.quarantined = Some(payload.to_vec());
                return Ok(());
            }
        }
        let Some(expected) = self.expected.remove(metadata) else {
            return match self.committed.get(metadata) {
                Some(digest) if *digest == sha256(payload) => Ok(()), // idempotent duplicate
                Some(_) => Err(RppError::ConflictingDuplicate),
                None => Err(RppError::NoAcceptedPlan),
            };
        };
        match expected.kind {
            ExpectedKind::Manifest { max } => {
                if payload.len() as u64 > max {
                    return Err(RppError::LimitExceeded);
                }
                let verified = VerifiedManifest::verify(payload, &self.binding)?;
                if verified.hash != self.publication.manifest_hash {
                    return Err(RppError::InvalidPublication(
                        "manifest hash differs from reference",
                    ));
                }
                self.publication.check_manifest(&verified)?;
                self.store.save_manifest(&verified)?;
                self.adopt(verified)?;
            }
            ExpectedKind::Bundle { indexes, payload_len, digest } => {
                indexes.iter().for_each(|index| {
                    self.in_flight.remove(index);
                });
                let manifest = self.manifest.as_ref().ok_or(RppError::NoAcceptedPlan)?;
                if payload.len() as u64 != payload_len || sha256(payload) != digest {
                    return Err(RppError::ChunkMismatch);
                }
                let mut offset = 0usize;
                let mut chunks = Vec::with_capacity(indexes.len());
                for index in &indexes {
                    let entry = &manifest.manifest.chunks[*index as usize];
                    let chunk = &payload[offset..offset + entry.len as usize];
                    if sha256(chunk) != entry.digest {
                        return Err(RppError::ChunkMismatch);
                    }
                    offset += entry.len as usize;
                    chunks.push((*index, chunk));
                }
                for (index, chunk) in chunks {
                    self.store.write_chunk(manifest, index, chunk)?;
                    self.present[index as usize] = true;
                }
            }
        }
        self.committed.insert(*metadata, sha256(payload));
        Ok(())
    }

    /// Link loss: drop link-local requests and plans, keep verified bytes.
    /// The next [`Self::next_requests`] uses fresh IDs and recomputes gaps.
    pub fn link_closed(&mut self) {
        self.pending.clear();
        self.expected.clear();
        self.in_flight.clear();
    }
}

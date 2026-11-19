//! Publisher engine: serves `/rpp/3` requests on canonical Package
//! Destinations and decides which native Resources to send.

use std::collections::HashMap;

use crate::control::{
    AcceptDecision, Body, Control, ReceiptStatus, RequestId, RppResult, Status, MAX_PLAN_INDEXES,
    MAX_WINDOW,
};
use crate::error::{Result, RppError};
use crate::identity::ReceiveDestination;
use crate::identity::{sha256, Destination, PublicIdentity};
use crate::manifest::VerifiedManifest;
use crate::replay::{ReplayCache, ReplayKey};
use crate::store::PackageSource;

/// Identifier of the RNS Link a request arrived on.
pub type LinkId = [u8; 16];

/// What the caller's policy is asked before any disclosure or byte send.
pub struct AuthRequest<'a> {
    pub requester: &'a PublicIdentity,
    pub control: &'a Control,
    pub manifest: &'a VerifiedManifest,
    pub now: u64,
}

pub type Policy = Box<dyn Fn(&AuthRequest<'_>) -> bool + Send + Sync>;

/// A native Resource the caller must send on the same Link, uncompressed,
/// with [`crate::pack_resource_metadata`] of `metadata` as RNS metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outbound {
    pub metadata: RequestId,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    /// Encoded `[1, id, status, null, value]` Link response.
    pub response: Vec<u8>,
    pub outbound: Option<Outbound>,
}

/// A recipient's TransferReceipt as recorded by the publisher. It is the
/// recipient's assertion, not proof of remote storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptRecord {
    pub requester: Destination,
    pub package: Destination,
    pub status: ReceiptStatus,
    pub verified_count: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct PublisherLimits {
    pub max_window: u64,
    pub max_bundle: u64,
    pub replay_capacity: usize,
}

impl Default for PublisherLimits {
    fn default() -> Self {
        Self {
            max_window: MAX_WINDOW,
            max_bundle: crate::control::MAX_BUNDLE_BYTES,
            replay_capacity: 4096,
        }
    }
}

struct Plan {
    package: Destination,
    requester: Destination,
    indexes: Vec<u32>,
    digest: [u8; 32],
    expires: u64,
    consumed: bool,
}

struct Package<S> {
    manifest: VerifiedManifest,
    source: S,
}

pub struct Publisher<S: PackageSource> {
    packages: HashMap<Destination, Package<S>>,
    plans: HashMap<(LinkId, RequestId), Plan>,
    outstanding: HashMap<LinkId, u64>,
    replay: ReplayCache,
    policy: Policy,
    limits: PublisherLimits,
    receipts: Vec<ReceiptRecord>,
}

impl<S: PackageSource> Publisher<S> {
    pub fn new(policy: Policy, limits: PublisherLimits, started_at: u64) -> Self {
        Self {
            packages: HashMap::new(),
            plans: HashMap::new(),
            outstanding: HashMap::new(),
            replay: ReplayCache::new(started_at, limits.replay_capacity),
            policy,
            limits,
            receipts: Vec::new(),
        }
    }

    /// Serves `manifest` at its canonical Package Destination.
    pub fn publish(&mut self, manifest: VerifiedManifest, source: S) {
        self.packages.insert(manifest.manifest.package_destination, Package { manifest, source });
    }

    pub fn receipts(&self) -> &[ReceiptRecord] {
        &self.receipts
    }

    /// Releases one outstanding Resource slot after its native outcome.
    pub fn resource_concluded(&mut self, link: LinkId) {
        if let Some(count) = self.outstanding.get_mut(&link) {
            *count = count.saturating_sub(1);
        }
    }

    /// Drops link-local plans and slots; verified state is unaffected.
    pub fn link_closed(&mut self, link: LinkId) {
        self.plans.retain(|(plan_link, _), _| *plan_link != link);
        self.outstanding.remove(&link);
    }

    /// Handles one request arriving on `link`, whose fixed Destination is
    /// `link_destination`. `None` means drop silently (unidentified or
    /// undecodable traffic).
    pub fn handle(
        &mut self,
        link: LinkId,
        link_destination: Destination,
        requester: Option<&PublicIdentity>,
        request: &[u8],
        now: u64,
    ) -> Option<Served> {
        let requester = requester?;
        let control = match Control::decode(request) {
            Ok(control) => control,
            Err(error) => {
                let request_id = error.request_id?;
                return Some(refusal(request_id, &error.error));
            }
        };
        let request_id = control.header.request_id;
        let key = ReplayKey::new(requester.hash(), &control.header);
        let served = self
            .admit(link_destination, requester, &control, request, &key, now)
            .and_then(|cached| match cached {
                Some(response) => Ok(Served { response, outbound: None }),
                None => self.execute(link, requester, &control, now),
            });
        let served = match served {
            Ok(served) => served,
            Err(error) => return Some(refusal(request_id, &error)),
        };
        if served.outbound.is_some() {
            *self.outstanding.entry(link).or_default() += 1;
        }
        if !self.replay.contains(&key) {
            if let Err(error) =
                self.replay.record(key, request, &served.response, control.header.expires, now)
            {
                log::warn!("rpp: refusing request without replay state: {error}");
                return Some(refusal(request_id, &error));
            }
        }
        Some(served)
    }

    /// Authorization, freshness and replay checks shared by every command.
    fn admit(
        &self,
        link_destination: Destination,
        requester: &PublicIdentity,
        control: &Control,
        request: &[u8],
        key: &ReplayKey,
        now: u64,
    ) -> Result<Option<Vec<u8>>> {
        let header = &control.header;
        // Unknown packages and foreign recipients look alike: no inventory leak.
        let package =
            self.packages.get(&header.package_destination).ok_or(RppError::NotAuthorized)?;
        if header.package_destination != link_destination
            || header.recipient != requester.receive_destination()
            || header.package_id != package.manifest.manifest.package_id
            || header.manifest_hash != package.manifest.hash
        {
            return Err(RppError::NotAuthorized);
        }
        let authorized =
            (self.policy)(&AuthRequest { requester, control, manifest: &package.manifest, now });
        if !authorized {
            return Err(RppError::NotAuthorized);
        }
        header.check_time(now)?;
        self.replay.lookup(key, request, header.issued)
    }

    fn execute(
        &mut self,
        link: LinkId,
        requester: &PublicIdentity,
        control: &Control,
        now: u64,
    ) -> Result<Served> {
        let header = control.header;
        let package = self.packages.get(&header.package_destination).ok_or(RppError::NotFound)?;
        let manifest = &package.manifest;
        let applied = |value: Option<Vec<u8>>, outbound| Served {
            response: RppResult { request_id: header.request_id, status: Status::Applied, value }
                .encode(),
            outbound,
        };
        match &control.body {
            Body::ManifestRequest { max_bytes, .. } => {
                if manifest.bytes.len() as u64 > *max_bytes {
                    return Err(RppError::LimitExceeded);
                }
                self.reserve(link)?;
                let outbound =
                    Outbound { metadata: header.request_id, payload: manifest.bytes.clone() };
                Ok(applied(None, Some(outbound)))
            }
            Body::MissingChunks { selection, max_bundle, window, .. } => {
                selection.check_bounds(manifest.chunk_count())?;
                if selection.is_empty() {
                    return Ok(applied(None, None));
                }
                // Expired or consumed plans no longer hold a window slot.
                self.plans.retain(|_, plan| !plan.consumed && plan.expires > now);
                let window = (*window).min(self.limits.max_window);
                let busy = self.outstanding.get(&link).copied().unwrap_or(0)
                    + self.plans.keys().filter(|(plan_link, _)| *plan_link == link).count() as u64;
                if busy >= window {
                    return Err(RppError::LimitExceeded);
                }
                let budget = (*max_bundle).min(self.limits.max_bundle);
                let mut indexes = Vec::new();
                let mut payload_len = 0u64;
                for index in selection.indexes() {
                    let len = manifest.manifest.chunks[index as usize].len;
                    if payload_len + len > budget || indexes.len() == MAX_PLAN_INDEXES {
                        break;
                    }
                    payload_len += len;
                    indexes.push(index);
                }
                if indexes.is_empty() {
                    return Err(RppError::LimitExceeded); // indivisible chunk over budget
                }
                let payload = read_bundle(&package.source, manifest, &indexes)?;
                let digest = sha256(&payload);
                self.plans.insert(
                    (link, header.request_id),
                    Plan {
                        package: header.package_destination,
                        requester: requester.hash(),
                        indexes: indexes.clone(),
                        digest,
                        expires: header.expires,
                        consumed: false,
                    },
                );
                let plan = Control {
                    header,
                    body: Body::ResourcePlan {
                        indexes,
                        payload_len,
                        bundle_digest: digest,
                        plan_id: header.request_id,
                    },
                };
                Ok(applied(Some(plan.encode()), None))
            }
            Body::ResourceAccept { plan_id, decision, .. } => {
                let plan = self.plans.get_mut(&(link, *plan_id)).ok_or(RppError::NoAcceptedPlan)?;
                if plan.consumed
                    || plan.expires <= now
                    || plan.requester != requester.hash()
                    || plan.package != header.package_destination
                {
                    return Err(RppError::NoAcceptedPlan);
                }
                if *decision != AcceptDecision::Accept {
                    self.plans.remove(&(link, *plan_id));
                    return Ok(applied(None, None));
                }
                plan.consumed = true;
                let indexes = plan.indexes.clone();
                let expected = plan.digest;
                let payload = read_bundle(&package.source, manifest, &indexes)?;
                if sha256(&payload) != expected {
                    return Err(RppError::Storage("published bytes changed".into()));
                }
                Ok(applied(None, Some(Outbound { metadata: *plan_id, payload })))
            }
            Body::TransferReceipt { status, verified_count, missing, .. } => {
                missing.check_bounds(manifest.chunk_count())?;
                if *status == ReceiptStatus::CompleteVerified
                    && *verified_count != u64::from(manifest.chunk_count())
                {
                    return Err(RppError::Incomplete);
                }
                self.receipts.push(ReceiptRecord {
                    requester: requester.hash(),
                    package: header.package_destination,
                    status: *status,
                    verified_count: *verified_count,
                });
                Ok(applied(None, None))
            }
            // Offers and replies are not requests a Package serves.
            _ => Err(RppError::Malformed("command not served on a Package Destination")),
        }
    }

    fn reserve(&self, link: LinkId) -> Result<()> {
        if self.outstanding.get(&link).copied().unwrap_or(0) >= self.limits.max_window {
            return Err(RppError::LimitExceeded);
        }
        Ok(())
    }
}

/// Reads and re-verifies plan chunks; corrupt local data is never sent.
fn read_bundle<S: PackageSource>(
    source: &S,
    manifest: &VerifiedManifest,
    indexes: &[u32],
) -> Result<Vec<u8>> {
    let mut payload = Vec::new();
    for &index in indexes {
        let chunk = source.read_chunk(manifest, index)?;
        let expected = &manifest.manifest.chunks[index as usize];
        if chunk.len() as u64 != expected.len || sha256(&chunk) != expected.digest {
            return Err(RppError::Storage(format!(
                "published chunk {index} does not match manifest"
            )));
        }
        payload.extend_from_slice(&chunk);
    }
    Ok(payload)
}

pub(crate) fn refusal(request_id: RequestId, error: &RppError) -> Served {
    Served {
        response: RppResult { request_id, status: error.status(), value: None }.encode(),
        outbound: None,
    }
}

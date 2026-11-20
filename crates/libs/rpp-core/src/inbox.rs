//! Recipient inbox (`rpp.receive`): answers addressed Offers.

use rand_core::{OsRng, RngCore};

use crate::control::{Body, Control, Header, OfferDecision, RppResult, Status};
use crate::error::RppError;
use crate::identity::ReceiveDestination;
use crate::identity::{Destination, PublicIdentity};
use crate::manifest::VerifiedManifest;
use crate::publisher::refusal;
use crate::replay::{ReplayCache, ReplayKey};

/// Builds an Offer from the owner of `manifest` to `recipient`'s inbox.
pub fn build_offer(
    manifest: &VerifiedManifest,
    recipient: &PublicIdentity,
    now: u64,
    lifetime: u64,
) -> Vec<u8> {
    let mut request_id = [0u8; 16];
    OsRng.fill_bytes(&mut request_id);
    Control {
        header: Header {
            package_id: manifest.manifest.package_id,
            manifest_hash: manifest.hash,
            package_destination: manifest.manifest.package_destination,
            recipient: recipient.receive_destination(),
            issued: now,
            expires: now + lifetime,
            request_id,
        },
        body: Body::Offer {
            manifest_destination: manifest.manifest.manifest_destination,
            capability: None,
        },
    }
    .encode()
}

/// Answers one Offer arriving on this identity's inbox Link.
///
/// `resolve_owner` returns the independently resolved (RDO) owner of a
/// Package Destination; the identified sender must equal it. `decide` is
/// local policy. Returns `None` for traffic that must be dropped.
pub fn answer_offer(
    replay: &mut ReplayCache,
    me: &PublicIdentity,
    sender: Option<&PublicIdentity>,
    request: &[u8],
    now: u64,
    resolve_owner: impl Fn(&Destination) -> Option<PublicIdentity>,
    decide: impl FnOnce(&Control) -> OfferDecision,
) -> Option<Vec<u8>> {
    let sender = sender?;
    let control = match Control::decode(request) {
        Ok(control) => control,
        Err(error) => return Some(refusal(error.request_id?, &error.error).response),
    };
    let header = control.header;
    let refuse = |error: RppError| Some(refusal(header.request_id, &error).response);
    if !matches!(control.body, Body::Offer { .. }) {
        return refuse(RppError::Malformed("inbox accepts only Offer"));
    }
    if header.recipient != me.receive_destination()
        || resolve_owner(&header.package_destination) != Some(*sender)
    {
        return refuse(RppError::NotAuthorized);
    }
    if let Err(error) = header.check_time(now) {
        return refuse(error);
    }
    let key = ReplayKey::new(sender.hash(), &header);
    match replay.lookup(&key, request, header.issued) {
        Ok(Some(response)) => return Some(response),
        Ok(None) => {}
        Err(error) => return refuse(error),
    }
    let decision = decide(&control);
    let ack = Control { header, body: Body::OfferAck { decision } };
    let response = RppResult {
        request_id: header.request_id,
        status: Status::Applied,
        value: Some(ack.encode()),
    }
    .encode();
    if let Err(error) = replay.record(key, request, &response, header.expires, now) {
        return refuse(error);
    }
    Some(response)
}

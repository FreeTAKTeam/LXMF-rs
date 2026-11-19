//! The seven RC1 control commands and the RDO v1 result wrapper.

use crate::cbor::{self, Fields, Value};
use crate::error::{Result, RppError};
use crate::identity::{Destination, Digest};
use crate::manifest::{check_extensions, MAX_SIGNED_MANIFEST, VERSION};
use crate::selection::Selection;

pub const MAX_CONTROL: usize = 4096;
pub const MAX_LIFETIME_SECS: u64 = 300;
pub const MAX_CAPABILITY: usize = 256;
pub const MAX_BUNDLE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_WINDOW: u64 = 8;
pub const MAX_PLAN_INDEXES: usize = 256;

pub type RequestId = [u8; 16];

/// RDO v1 result status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Applied = 0,
    Rejected = 1,
    Pending = 2,
    Stale = 3,
    NotAuthorized = 4,
    NotFound = 5,
    Error = 6,
}

impl Status {
    fn from_u64(value: u64) -> Result<Self> {
        Ok(match value {
            0 => Self::Applied,
            1 => Self::Rejected,
            2 => Self::Pending,
            3 => Self::Stale,
            4 => Self::NotAuthorized,
            5 => Self::NotFound,
            6 => Self::Error,
            _ => return Err(RppError::Malformed("unknown result status")),
        })
    }
}

/// OfferAck decision (key 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferDecision {
    Rejected = 0,
    Accepted = 1,
    AlreadyHave = 2,
    Defer = 3,
    RequestManifest = 4,
    RequestPlan = 5,
}

/// ResourceAccept decision (key 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptDecision {
    Accept = 0,
    Reject = 1,
    Defer = 2,
}

/// TransferReceipt status (key 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptStatus {
    Failed = 0,
    Partial = 1,
    Complete = 2,
    CompleteVerified = 3,
    RejectedAfterVerification = 4,
}

const OFFER_DECISIONS: [OfferDecision; 6] = [
    OfferDecision::Rejected,
    OfferDecision::Accepted,
    OfferDecision::AlreadyHave,
    OfferDecision::Defer,
    OfferDecision::RequestManifest,
    OfferDecision::RequestPlan,
];
const ACCEPT_DECISIONS: [AcceptDecision; 3] =
    [AcceptDecision::Accept, AcceptDecision::Reject, AcceptDecision::Defer];
const RECEIPT_STATUSES: [ReceiptStatus; 5] = [
    ReceiptStatus::Failed,
    ReceiptStatus::Partial,
    ReceiptStatus::Complete,
    ReceiptStatus::CompleteVerified,
    ReceiptStatus::RejectedAfterVerification,
];

fn pick<T: Copy>(table: &[T], value: u64) -> Result<T> {
    usize::try_from(value)
        .ok()
        .and_then(|index| table.get(index).copied())
        .ok_or(RppError::Malformed("unassigned enumeration value"))
}

/// Header shared by requests and positive replies (keys 0–6, 12, 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub package_id: Digest,
    pub manifest_hash: Digest,
    pub package_destination: Destination,
    pub recipient: Destination,
    pub issued: u64,
    pub expires: u64,
    pub request_id: RequestId,
}

impl Header {
    /// `issued ≤ now < expires`.
    pub fn check_time(&self, now: u64) -> Result<()> {
        if self.issued <= now && now < self.expires {
            Ok(())
        } else {
            Err(RppError::Expired)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Offer {
        manifest_destination: Destination,
        capability: Option<Vec<u8>>,
    },
    OfferAck {
        decision: OfferDecision,
    },
    ManifestRequest {
        max_bytes: u64,
        capability: Option<Vec<u8>>,
    },
    ResourcePlan {
        indexes: Vec<u32>,
        payload_len: u64,
        bundle_digest: Digest,
        plan_id: RequestId,
    },
    ResourceAccept {
        plan_id: RequestId,
        decision: AcceptDecision,
        capability: Option<Vec<u8>>,
    },
    TransferReceipt {
        status: ReceiptStatus,
        verified_count: u64,
        missing: Selection,
        capability: Option<Vec<u8>>,
    },
    MissingChunks {
        selection: Selection,
        max_bundle: u64,
        window: u64,
        capability: Option<Vec<u8>>,
    },
}

impl Body {
    pub fn command(&self) -> u64 {
        match self {
            Self::Offer { .. } => 1,
            Self::OfferAck { .. } => 2,
            Self::ManifestRequest { .. } => 3,
            Self::ResourcePlan { .. } => 4,
            Self::ResourceAccept { .. } => 5,
            Self::TransferReceipt { .. } => 6,
            Self::MissingChunks { .. } => 7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub header: Header,
    pub body: Body,
}

/// A refused control. `request_id` is present only when the header was
/// decodable and supported, i.e. when the refusal deserves an outer status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlError {
    pub request_id: Option<RequestId>,
    pub error: RppError,
}

impl Control {
    pub fn decode(bytes: &[u8]) -> std::result::Result<Self, ControlError> {
        let unsupported = |error| ControlError { request_id: None, error };
        let map = cbor::decode(bytes, MAX_CONTROL, false)
            .and_then(Value::into_map)
            .map_err(unsupported)?;
        let mut fields = Fields::new(map);
        let (command, header) = decode_header(&mut fields).map_err(unsupported)?;
        let request_id = Some(header.request_id);
        let body = check_extensions(&mut fields)
            .and_then(|()| decode_body(command, &mut fields))
            .and_then(|body| fields.finish().map(|()| body))
            .and_then(|body| {
                let lifetime = header.expires.checked_sub(header.issued);
                if lifetime.is_some_and(|secs| (1..=MAX_LIFETIME_SECS).contains(&secs)) {
                    Ok(body)
                } else {
                    Err(RppError::Expired)
                }
            })
            .map_err(|error| ControlError { request_id, error })?;
        Ok(Self { header, body })
    }

    pub fn encode(&self) -> Vec<u8> {
        let h = &self.header;
        let bytes = |b: &[u8]| Some(Value::Bytes(b.to_vec()));
        let mut entries = vec![
            (0, Some(Value::Uint(self.body.command()))),
            (1, Some(Value::Uint(VERSION))),
            (2, bytes(&h.package_id)),
            (3, bytes(&h.manifest_hash)),
            (4, bytes(&h.package_destination)),
            (5, bytes(&h.recipient)),
            (6, Some(Value::Uint(h.issued))),
            (12, Some(Value::Uint(h.expires))),
            (13, bytes(&h.request_id)),
        ];
        let capability = |c: &Option<Vec<u8>>| c.clone().map(Value::Bytes);
        match &self.body {
            Body::Offer { manifest_destination, capability: c } => {
                entries.extend([(9, bytes(manifest_destination)), (16, capability(c))])
            }
            Body::OfferAck { decision } => entries.push((17, Some(Value::Uint(*decision as u64)))),
            Body::ManifestRequest { max_bytes, capability: c } => {
                entries.extend([(16, capability(c)), (18, Some(Value::Uint(*max_bytes)))])
            }
            Body::ResourcePlan { indexes, payload_len, bundle_digest, plan_id } => {
                entries.extend([
                    (19, Some(cbor::uint_array(indexes.iter().map(|i| u64::from(*i))))),
                    (20, Some(Value::Uint(*payload_len))),
                    (21, bytes(bundle_digest)),
                    (22, bytes(plan_id)),
                ])
            }
            Body::ResourceAccept { plan_id, decision, capability: c } => entries.extend([
                (16, capability(c)),
                (17, Some(Value::Uint(*decision as u64))),
                (22, bytes(plan_id)),
            ]),
            Body::TransferReceipt { status, verified_count, missing, capability: c } => entries
                .extend([
                    (16, capability(c)),
                    (17, Some(Value::Uint(*status as u64))),
                    (23, Some(missing.encode())),
                    (24, Some(Value::Uint(*verified_count))),
                ]),
            Body::MissingChunks { selection, max_bundle, window, capability: c } => {
                entries.extend([
                    (16, capability(c)),
                    (18, Some(Value::Uint(*max_bundle))),
                    (23, Some(selection.encode())),
                    (30, Some(Value::Uint(*window))),
                ])
            }
        }
        cbor::encode(&cbor::map(entries))
    }
}

fn decode_header(fields: &mut Fields) -> Result<(u64, Header)> {
    let command = fields.uint(0)?;
    if !(1..=7).contains(&command) {
        return Err(RppError::Malformed("unknown command"));
    }
    let version = fields.uint(1)?;
    if version != VERSION {
        return Err(RppError::UnsupportedVersion(version));
    }
    let header = Header {
        package_id: fields.fixed(2)?,
        manifest_hash: fields.fixed(3)?,
        package_destination: fields.fixed(4)?,
        recipient: fields.fixed(5)?,
        issued: fields.uint(6)?,
        expires: fields.uint(12)?,
        request_id: fields.fixed(13)?,
    };
    Ok((command, header))
}

fn decode_body(command: u64, fields: &mut Fields) -> Result<Body> {
    let capability = |fields: &mut Fields| fields.opt_bytes(16, MAX_CAPABILITY);
    let bounded = |value: u64, max: u64| {
        if (1..=max).contains(&value) {
            Ok(value)
        } else {
            Err(RppError::LimitExceeded)
        }
    };
    Ok(match command {
        1 => {
            Body::Offer { manifest_destination: fields.fixed(9)?, capability: capability(fields)? }
        }
        2 => Body::OfferAck { decision: pick(&OFFER_DECISIONS, fields.uint(17)?)? },
        3 => Body::ManifestRequest {
            max_bytes: bounded(fields.uint(18)?, MAX_SIGNED_MANIFEST as u64)?,
            capability: capability(fields)?,
        },
        4 => {
            let raw = fields.array(19)?;
            if raw.is_empty() || raw.len() > MAX_PLAN_INDEXES {
                return Err(RppError::InvalidSelection);
            }
            let mut indexes = Vec::with_capacity(raw.len());
            for value in raw {
                let index =
                    u32::try_from(value.as_uint()?).map_err(|_| RppError::InvalidSelection)?;
                if indexes.contains(&index) {
                    return Err(RppError::InvalidSelection);
                }
                indexes.push(index);
            }
            Body::ResourcePlan {
                indexes,
                payload_len: fields.uint(20)?,
                bundle_digest: fields.fixed(21)?,
                plan_id: fields.fixed(22)?,
            }
        }
        5 => Body::ResourceAccept {
            plan_id: fields.fixed(22)?,
            decision: pick(&ACCEPT_DECISIONS, fields.uint(17)?)?,
            capability: capability(fields)?,
        },
        6 => {
            let status = pick(&RECEIPT_STATUSES, fields.uint(17)?)?;
            let missing = Selection::decode(fields.array(23)?)?;
            if status == ReceiptStatus::CompleteVerified && !missing.is_empty() {
                return Err(RppError::Incomplete);
            }
            Body::TransferReceipt {
                status,
                verified_count: fields.uint(24)?,
                missing,
                capability: capability(fields)?,
            }
        }
        _ => Body::MissingChunks {
            selection: Selection::decode(fields.array(23)?)?,
            max_bundle: bounded(fields.uint(18)?, MAX_BUNDLE_BYTES)?,
            window: bounded(fields.uint(30)?, MAX_WINDOW)?,
            capability: capability(fields)?,
        },
    })
}

/// The RDO v1 result array `[1, id, status, null, value]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RppResult {
    pub request_id: RequestId,
    pub status: Status,
    /// Encoded reply control on success, `None` otherwise.
    pub value: Option<Vec<u8>>,
}

impl RppResult {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Uint(1),
            Value::Bytes(self.request_id.to_vec()),
            Value::Uint(self.status as u64),
            Value::Null,
            self.value.clone().map_or(Value::Null, Value::Bytes),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let items = cbor::decode(bytes, MAX_CONTROL, true)?.into_array()?;
        let [version, id, status, version_digest, value]: [Value; 5] =
            items.try_into().map_err(|_| RppError::Malformed("result must have five items"))?;
        if version.as_uint()? != 1 || version_digest != Value::Null {
            return Err(RppError::Malformed("unsupported result wrapper"));
        }
        let value = match value {
            Value::Null => None,
            other => Some(other.into_bytes()?),
        };
        Ok(Self {
            request_id: id.into_fixed()?,
            status: Status::from_u64(status.as_uint()?)?,
            value,
        })
    }
}

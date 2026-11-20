//! Owner operations on Link request path `/rdo/1/op`: READ and PROPOSE with
//! single-owner commits and duplicate-request replay.

use std::collections::{BTreeMap, HashMap};

use crate::cbor::{self, Value};
use crate::error::{RdoError, Result};
use crate::identity::{Destination, Digest, PublicIdentity, SigningIdentity};
use crate::revision::Revision;

/// Native RNS Link request path for owner operations.
pub const REQUEST_PATH: &str = "/rdo/1/op";
/// Whole request/result limit.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// Longest accepted request lifetime, in seconds.
pub const MAX_LIFETIME: u64 = 300;

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
    pub fn from_u64(value: u64) -> Result<Self> {
        Ok(match value {
            0 => Self::Applied,
            1 => Self::Rejected,
            2 => Self::Pending,
            3 => Self::Stale,
            4 => Self::NotAuthorized,
            5 => Self::NotFound,
            6 => Self::Error,
            _ => return Err(RdoError::Malformed("unknown result status")),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Read,
    Propose,
}

/// `[1, id, object, op, expected, args, expires]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: RequestId,
    pub object: Destination,
    /// Raw operation number; unassigned numbers are rejected by the owner.
    pub op: u64,
    pub expected: Option<Digest>,
    /// Null, or the proposed replacement body map.
    pub args: Option<BTreeMap<u64, Value>>,
    pub expires: u64,
}

impl Request {
    pub fn read(id: RequestId, object: Destination, expires: u64) -> Self {
        Self { id, object, op: 0, expected: None, args: None, expires }
    }

    pub fn propose(
        id: RequestId,
        object: Destination,
        expected: Digest,
        body: BTreeMap<u64, Value>,
        expires: u64,
    ) -> Self {
        Self { id, object, op: 1, expected: Some(expected), args: Some(body), expires }
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Uint(1),
            Value::Bytes(self.id.to_vec()),
            Value::Bytes(self.object.to_vec()),
            Value::Uint(self.op),
            self.expected.map_or(Value::Null, |digest| Value::Bytes(digest.to_vec())),
            self.args.clone().map_or(Value::Null, Value::Map),
            Value::Uint(self.expires),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let items = cbor::decode(bytes, MAX_MESSAGE, true)?.into_array()?;
        let [version, id, object, op, expected, args, expires]: [Value; 7] =
            items.try_into().map_err(|_| RdoError::Malformed("request must have seven items"))?;
        if version.as_uint()? != 1 {
            return Err(RdoError::UnsupportedVersion(version.as_uint()?));
        }
        Ok(Self {
            id: id.into_fixed()?,
            object: object.into_fixed()?,
            op: op.as_uint()?,
            expected: nullable(expected, Value::into_fixed)?,
            args: nullable(args, Value::into_map)?,
            expires: expires.as_uint()?,
        })
    }
}

/// `[1, id, status, version, value]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpResult {
    pub id: RequestId,
    pub status: Status,
    pub version: Option<Digest>,
    /// Exact signed revision bytes when specified.
    pub value: Option<Vec<u8>>,
}

impl OpResult {
    fn bare(id: RequestId, status: Status) -> Self {
        Self { id, status, version: None, value: None }
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Uint(1),
            Value::Bytes(self.id.to_vec()),
            Value::Uint(self.status as u64),
            self.version.map_or(Value::Null, |digest| Value::Bytes(digest.to_vec())),
            self.value.clone().map_or(Value::Null, Value::Bytes),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let items = cbor::decode(bytes, MAX_MESSAGE, true)?.into_array()?;
        let [version, id, status, digest, value]: [Value; 5] =
            items.try_into().map_err(|_| RdoError::Malformed("result must have five items"))?;
        if version.as_uint()? != 1 {
            return Err(RdoError::UnsupportedVersion(version.as_uint()?));
        }
        Ok(Self {
            id: id.into_fixed()?,
            status: Status::from_u64(status.as_uint()?)?,
            version: nullable(digest, Value::into_fixed)?,
            value: nullable(value, Value::into_bytes)?,
        })
    }
}

fn nullable<T>(value: Value, convert: impl FnOnce(Value) -> Result<T>) -> Result<Option<T>> {
    match value {
        Value::Null => Ok(None),
        other => convert(other).map(Some),
    }
}

/// The owner's access policy, applied to the authenticated Link initiator.
/// Applications keep their own rules (mission, team) behind this trait.
pub trait Policy {
    fn may_read(&self, initiator: &PublicIdentity) -> bool;
    fn may_propose(&self, initiator: &PublicIdentity) -> bool;
}

struct Retained {
    request: Vec<u8>,
    result: Vec<u8>,
    expires: u64,
}

/// The single owner of one object: its accepted history and replay records.
pub struct Owner {
    identity: SigningIdentity,
    history: Vec<Revision>,
    replay: HashMap<(Destination, RequestId), Retained>,
}

impl Owner {
    /// Starts from an accepted chain (genesis first), verifying every link.
    pub fn new(identity: SigningIdentity, history: Vec<Revision>) -> Result<Self> {
        let Some(genesis) = history.first() else {
            return Err(RdoError::InvalidRevision("history is empty"));
        };
        if genesis.parent.is_some() {
            return Err(RdoError::InvalidRevision("history must start at genesis"));
        }
        for pair in history.windows(2) {
            pair[0].check_successor(&pair[1])?;
        }
        for revision in &history {
            Revision::verify(&revision.bytes, &identity.public())?;
        }
        Ok(Self { identity, history, replay: HashMap::new() })
    }

    pub fn object(&self) -> Destination {
        self.head().object
    }

    pub fn head(&self) -> &Revision {
        self.history.last().expect("history is never empty")
    }

    /// Every accepted revision, oldest first. Old bytes are retained.
    pub fn history(&self) -> &[Revision] {
        &self.history
    }

    /// Serves one request from the authenticated Link initiator received on
    /// `link_destination`. `None` means drop: undecodable or unidentified.
    pub fn handle(
        &mut self,
        initiator: Option<&PublicIdentity>,
        link_destination: &Destination,
        bytes: &[u8],
        now: u64,
        policy: &dyn Policy,
    ) -> Option<Vec<u8>> {
        let initiator = initiator?;
        let request = Request::decode(bytes).ok()?;
        self.replay.retain(|_, retained| retained.expires > now);

        let allowed = match request.op {
            1 => policy.may_propose(initiator),
            _ => policy.may_read(initiator),
        };
        if !allowed {
            return Some(OpResult::bare(request.id, Status::NotAuthorized).encode());
        }
        let key = (initiator.hash(), request.id);
        if let Some(retained) = self.replay.get(&key) {
            let status = if retained.request == bytes { None } else { Some(Status::Rejected) };
            return Some(match status {
                None => retained.result.clone(),
                Some(status) => OpResult::bare(request.id, status).encode(),
            });
        }
        let result = self.execute(&request, link_destination, now).encode();
        self.replay.insert(
            key,
            Retained { request: bytes.to_vec(), result: result.clone(), expires: request.expires },
        );
        Some(result)
    }

    fn execute(&mut self, request: &Request, link_destination: &Destination, now: u64) -> OpResult {
        let reject = OpResult::bare(request.id, Status::Rejected);
        let fresh = now < request.expires && request.expires <= now.saturating_add(MAX_LIFETIME);
        if !fresh || request.object != *link_destination || request.object != self.object() {
            return reject;
        }
        match (request.op, request.expected, &request.args) {
            (0, None, None) => self.applied(request.id),
            (1, Some(expected), Some(_)) if expected != self.head().id => OpResult {
                id: request.id,
                status: Status::Stale,
                version: Some(self.head().id),
                value: None,
            },
            (1, Some(_), Some(body)) => {
                let head = self.head();
                match Revision::sign(
                    &self.identity,
                    head.object,
                    Some(head.id),
                    &head.type_iri,
                    body.clone(),
                ) {
                    Ok(successor) => {
                        self.history.push(successor);
                        self.applied(request.id)
                    }
                    Err(_) => reject,
                }
            }
            _ => reject,
        }
    }

    fn applied(&self, id: RequestId) -> OpResult {
        OpResult {
            id,
            status: Status::Applied,
            version: Some(self.head().id),
            value: Some(self.head().bytes.clone()),
        }
    }
}

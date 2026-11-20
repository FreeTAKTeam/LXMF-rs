//! Signed, immutable RDO revisions: `[1, object, parent, type, body, signature]`.

use std::collections::BTreeMap;

use crate::cbor::{self, Value};
use crate::error::{RdoError, Result};
use crate::identity::{sha256, Destination, Digest, PublicIdentity, SigningIdentity};

/// Whole encoded revision limit.
pub const MAX_REVISION: usize = 48 * 1024;
pub const ONTOLOGY: &str = "https://freetakteam.github.io/rdo/ontology#";
const SIGNING_DOMAIN: &str = "rdo.revision.v1";

/// A revision whose structure and owner signature have been checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub object: Destination,
    /// Revision ID of the exact predecessor, `None` for genesis.
    pub parent: Option<Digest>,
    /// Vocabulary IRI of the object type.
    pub type_iri: String,
    pub body: BTreeMap<u64, Value>,
    /// Revision ID: SHA-256 of the whole encoded revision, signature included.
    pub id: Digest,
    /// The exact signed bytes, retained for READ results and offline replicas.
    pub bytes: Vec<u8>,
}

impl Revision {
    /// Serializes and signs a revision as the object's owner.
    pub fn sign(
        owner: &SigningIdentity,
        object: Destination,
        parent: Option<Digest>,
        type_iri: &str,
        body: BTreeMap<u64, Value>,
    ) -> Result<Self> {
        check_body(type_iri, &body)?;
        let fields = fields(object, parent, type_iri, body.clone());
        let signature = owner.sign(&signed_bytes(&fields));
        let bytes = encode(fields, &signature);
        if bytes.len() > MAX_REVISION {
            return Err(RdoError::LimitExceeded);
        }
        Ok(Self { object, parent, type_iri: type_iri.to_owned(), body, id: sha256(&bytes), bytes })
    }

    /// Decodes a revision and verifies it against the resolved owner Identity.
    pub fn verify(bytes: &[u8], owner: &PublicIdentity) -> Result<Self> {
        let items = cbor::decode(bytes, MAX_REVISION, true)?.into_array()?;
        let [version, object, parent, type_iri, body, signature]: [Value; 6] =
            items.try_into().map_err(|_| RdoError::Malformed("revision must have six items"))?;
        if version.as_uint()? != 1 {
            return Err(RdoError::UnsupportedVersion(version.as_uint()?));
        }
        let object: Destination = object.into_fixed()?;
        let parent = match parent {
            Value::Null => None,
            other => Some(other.into_fixed::<32>()?),
        };
        let type_iri = type_iri.into_text()?;
        let body = body.into_map()?;
        let fields = fields(object, parent, &type_iri, body.clone());
        owner.verify(&signed_bytes(&fields), &signature.into_fixed()?)?;
        check_body(&type_iri, &body)?;
        Ok(Self { object, parent, type_iri, body, id: sha256(bytes), bytes: bytes.to_vec() })
    }

    /// A valid successor keeps object and type and names this exact revision.
    pub fn check_successor(&self, successor: &Revision) -> Result<()> {
        if successor.object != self.object || successor.type_iri != self.type_iri {
            return Err(RdoError::InvalidRevision("successor changes object or type"));
        }
        if successor.parent != Some(self.id) {
            return Err(RdoError::InvalidRevision("successor does not name the accepted head"));
        }
        Ok(())
    }
}

fn fields(
    object: Destination,
    parent: Option<Digest>,
    type_iri: &str,
    body: BTreeMap<u64, Value>,
) -> [Value; 4] {
    [
        Value::Bytes(object.to_vec()),
        parent.map_or(Value::Null, |parent| Value::Bytes(parent.to_vec())),
        Value::Text(type_iri.to_owned()),
        Value::Map(body),
    ]
}

fn signed_bytes(fields: &[Value; 4]) -> Vec<u8> {
    let head = [Value::Text(SIGNING_DOMAIN.into()), Value::Uint(1)];
    cbor::encode(&Value::Array(head.into_iter().chain(fields.iter().cloned()).collect()))
}

fn encode(fields: [Value; 4], signature: &[u8; 64]) -> Vec<u8> {
    let mut items = vec![Value::Uint(1)];
    items.extend(fields);
    items.push(Value::Bytes(signature.to_vec()));
    cbor::encode(&Value::Array(items))
}

/// Structural checks for the M3 base types. Other types (Object, Class and
/// M2 types such as the RPP Package) only need a uint-keyed map here; their
/// own profiles validate the body.
pub fn check_body(type_iri: &str, body: &BTreeMap<u64, Value>) -> Result<()> {
    let base = type_iri.strip_prefix(ONTOLOGY);
    if !matches!(base, Some("Property") | Some("Relationship")) {
        return Ok(());
    }
    let mut fields = cbor::Fields::new(body.clone());
    let _source: Destination = fields.fixed(0)?;
    fields.required(1)?.into_text()?;
    let value = fields.required(2)?;
    fields.finish()?;
    if base == Some("Relationship") {
        value.into_fixed::<16>()?;
        return Ok(());
    }
    match value.into_array()?.as_slice() {
        [Value::Uint(0), Value::Bytes(target)] if target.len() == 16 => Ok(()),
        [Value::Uint(1), Value::Text(_), Value::Text(_)] => Ok(()),
        _ => Err(RdoError::InvalidRevision("Property value must be a reference or typed literal")),
    }
}

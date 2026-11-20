//! Object Destination derivation and signed-announce lookup.

use crate::cbor::{self, Value};
use crate::error::{RdoError, Result};
use crate::identity::{destination_hash, Destination, PublicIdentity};

pub const APP_NAME: &str = "rdo";
/// `CBOR([1, token])` is exactly this long.
pub const APP_DATA_LEN: usize = 19;

pub type Token = [u8; 16];

/// A verified (Destination, token, owner Identity) tuple, safe to cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectBinding {
    pub destination: Destination,
    pub token: Token,
    pub owner: PublicIdentity,
}

/// The aspects after the app name: `object.<32 lowercase hex digits>`.
pub fn object_aspect(token: &Token) -> String {
    let hex: String = token.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("object.{hex}")
}

/// The canonical SINGLE Destination of the object with this token.
pub fn object_destination(owner: &PublicIdentity, token: &Token) -> Destination {
    destination_hash(&owner.0, &format!("{APP_NAME}.{}", object_aspect(token)))
}

/// The announce application payload, `CBOR([1, token])`.
pub fn announce_app_data(token: &Token) -> Vec<u8> {
    cbor::encode(&Value::Array(vec![Value::Uint(1), Value::Bytes(token.to_vec())]))
}

/// Parses the exact 19-byte announce payload and returns its token.
pub fn parse_announce_app_data(app_data: &[u8]) -> Result<Token> {
    if app_data.len() != APP_DATA_LEN {
        return Err(RdoError::InvalidBinding("app data is not CBOR([1, token])"));
    }
    match cbor::decode(app_data, APP_DATA_LEN, false)?.into_array()?.as_slice() {
        [Value::Uint(1), Value::Bytes(token)] => {
            token.as_slice().try_into().map_err(|_| RdoError::InvalidBinding("token length"))
        }
        _ => Err(RdoError::InvalidBinding("app data is not CBOR([1, token])")),
    }
}

/// Checks an already signature-validated announce: the token in its app data
/// and its public Identity must reconstruct both the advertised Destination
/// and the separately requested one.
pub fn bind(
    owner: PublicIdentity,
    advertised: &Destination,
    app_data: &[u8],
    wanted: &Destination,
) -> Result<ObjectBinding> {
    let token = parse_announce_app_data(app_data)?;
    let destination = object_destination(&owner, &token);
    if destination != *advertised || destination != *wanted {
        return Err(RdoError::InvalidBinding("announce is for another object"));
    }
    Ok(ObjectBinding { destination, token, owner })
}

/// Validates a complete raw RNS announce packet (Destination and signature)
/// and binds it to the requested object Destination.
#[cfg(feature = "rns")]
pub fn verify_announce(raw: &[u8], wanted: &Destination) -> Result<ObjectBinding> {
    use rns_core::destination::DestinationAnnounce;
    use rns_core::packet::Packet;

    let packet =
        Packet::from_bytes(raw).map_err(|_| RdoError::InvalidBinding("not an RNS packet"))?;
    let info = DestinationAnnounce::validate(&packet)
        .map_err(|_| RdoError::InvalidBinding("announce signature or Destination"))?;
    let identity = info.destination.desc.identity;
    let mut public = [0u8; 64];
    public[..32].copy_from_slice(identity.public_key_bytes());
    public[32..].copy_from_slice(identity.verifying_key_bytes());
    let advertised: Destination = packet
        .destination
        .as_slice()
        .try_into()
        .map_err(|_| RdoError::InvalidBinding("Destination length"))?;
    // shortcut: a stamped announce must have its stamp stripped by RNS before
    // this point; raw stamped packets are not parsed here yet.
    bind(PublicIdentity(public), &advertised, info.app_data, wanted)
}

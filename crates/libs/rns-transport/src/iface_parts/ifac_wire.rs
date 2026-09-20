use crate::error::RnsError;
use crate::packet::PacketIfac;
use crate::transport::{IfacContext, IFAC_MIN_SIZE};

/// RNS uses a 16-byte IFAC tag unless an interface explicitly overrides the
/// configured size. The TOML/RNS setting is in bits and is converted to bytes
/// before it reaches the wire codec.
pub const DEFAULT_IFAC_SIZE_BYTES: usize = 16;
pub const MAX_IFAC_SIZE_BYTES: usize = 64;

/// Shared live IFAC state for a carrier and its interface-manager metadata.
/// Keeping the state, violation counter, and carrier default together avoids
/// passing loosely related wire-policy arguments through runtime constructors.
#[derive(Clone)]
pub(crate) struct IfacRuntime {
    pub(crate) state: IfacState,
    pub(crate) violations: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub(crate) default_size_bytes: usize,
}

impl IfacRuntime {
    pub(crate) fn disabled(default_size_bytes: usize) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::RwLock::new(None)),
            violations: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            default_size_bytes,
        }
    }

    pub(crate) fn from_parts(
        state: IfacState,
        violations: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        Self {
            state,
            violations,
            default_size_bytes: DEFAULT_IFAC_SIZE_BYTES,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IfacWireError {
    #[error("IFAC flag is missing on an authenticated interface")]
    MissingFlag,
    #[error("IFAC flag is set on an unauthenticated interface")]
    UnexpectedFlag,
    #[error("IFAC tag or frame is invalid")]
    InvalidTag,
    #[error("IFAC codec failed: {0}")]
    Codec(#[from] RnsError),
}

impl InterfaceSharedConfig {
    /// Build the exact RNS IFAC context represented by this shared config.
    ///
    /// `ifac_size` follows Python Reticulum's configuration contract: it is a
    /// number of bits, while `IfacContext` consumes bytes. A size without credentials is
    /// rejected so a configuration cannot look authenticated while silently
    /// running plaintext.
    pub fn ifac_context(&self) -> Result<Option<IfacContext>, RnsError> {
        self.ifac_context_with_default_size(DEFAULT_IFAC_SIZE_BYTES)
    }

    /// Build an IFAC context using a carrier-specific default when the config
    /// omits `ifac_size`. Python Reticulum assigns this default from the
    /// concrete interface class (for example, 8 bytes for serial/KISS and
    /// 16 bytes for TCP/UDP).
    pub fn ifac_context_with_default_size(
        &self,
        default_size_bytes: usize,
    ) -> Result<Option<IfacContext>, RnsError> {
        let network_name = self.network_name.as_deref().filter(|value| !value.is_empty());
        let passphrase = self.passphrase.as_deref().filter(|value| !value.is_empty());

        if network_name.is_none() && passphrase.is_none() {
            return if self.ifac_size.is_some() {
                Err(RnsError::InvalidArgument)
            } else {
                Ok(None)
            };
        }

        let ifac_size = match self.ifac_size {
            Some(bits) if bits >= (IFAC_MIN_SIZE as u64) * 8 && bits % 8 == 0 => {
                usize::try_from(bits / 8).map_err(|_| RnsError::InvalidArgument)?
            }
            Some(_) => return Err(RnsError::InvalidArgument),
            None => default_size_bytes,
        };
        if !(IFAC_MIN_SIZE..=MAX_IFAC_SIZE_BYTES).contains(&ifac_size) {
            return Err(RnsError::InvalidArgument);
        }

        IfacContext::from_network_credentials(ifac_size, network_name, passphrase).map(Some)
    }
}

/// Apply IFAC to a serialized packet before a carrier-specific framing layer.
pub fn encode_ifac(state: &IfacState, raw: &[u8]) -> Result<Vec<u8>, IfacWireError> {
    let guard = state.read().map_err(|_| RnsError::ConnectionError)?;
    match guard.as_ref() {
        Some(context) => context.encode(raw).map_err(IfacWireError::Codec),
        None => {
            if raw.first().is_some_and(|byte| byte & 0x80 != 0) {
                Err(IfacWireError::UnexpectedFlag)
            } else {
                Ok(raw.to_vec())
            }
        }
    }
}

/// Authenticate and remove IFAC before packet deserialization.
pub fn decode_ifac(state: &IfacState, raw: &[u8]) -> Result<Vec<u8>, IfacWireError> {
    let authenticated = raw.first().is_some_and(|byte| byte & 0x80 != 0);
    let guard = state.read().map_err(|_| RnsError::ConnectionError)?;
    match guard.as_ref() {
        Some(context) => {
            if !authenticated {
                return Err(IfacWireError::MissingFlag);
            }
            context
                .decode(raw)
                .map_err(IfacWireError::Codec)?
                .ok_or(IfacWireError::InvalidTag)
        }
        None => {
            if authenticated {
                Err(IfacWireError::UnexpectedFlag)
            } else {
                Ok(raw.to_vec())
            }
        }
    }
}

/// Serialize a packet and apply the interface's live IFAC policy.
pub fn encode_packet_ifac(
    state: &IfacState,
    packet: &Packet,
) -> Result<Vec<u8>, IfacWireError> {
    let raw = packet.to_bytes().map_err(IfacWireError::Codec)?;
    encode_ifac(state, &raw)
}

/// Authenticate a carrier payload and deserialize only the authenticated
/// packet bytes. This is deliberately the only packet admission helper used
/// by carrier receive loops.
pub fn decode_packet_ifac(state: &IfacState, raw: &[u8]) -> Result<Packet, IfacWireError> {
    let authenticated = state
        .read()
        .map_err(|_| RnsError::ConnectionError)?
        .is_some();
    let raw = decode_ifac(state, raw)?;
    let mut packet = Packet::from_bytes(&raw).map_err(IfacWireError::Codec)?;
    if authenticated {
        // `Packet::header.ifac_flag` describes the packet bytes after IFAC has
        // been removed. Keep the wire-authentication fact as non-serialized
        // metadata so transport admission does not mistake a verified frame
        // for a missing IFAC header.
        packet.ifac = Some(PacketIfac::new_from_slice(&[]));
    }
    Ok(packet)
}

pub fn is_ifac_violation(error: &IfacWireError) -> bool {
    matches!(
        error,
        IfacWireError::MissingFlag | IfacWireError::UnexpectedFlag | IfacWireError::InvalidTag
    )
}

/// Record a carrier-level IFAC rejection while keeping the payload private.
pub fn record_ifac_violation(
    counter: &std::sync::atomic::AtomicU64,
    error: &IfacWireError,
) {
    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    log::debug!("IFAC wire frame rejected: {error}");
}

#[cfg(test)]
mod ifac_wire_tests {
    use super::*;

    fn state(config: InterfaceSharedConfig) -> IfacState {
        std::sync::Arc::new(std::sync::RwLock::new(
            config.ifac_context().expect("IFAC config").clone(),
        ))
    }

    #[test]
    fn configured_size_matches_python_bit_contract() {
        let config = InterfaceSharedConfig {
            ifac_size: Some(16),
            network_name: Some("field-net".to_string()),
            ..InterfaceSharedConfig::default()
        };
        assert_eq!(config.ifac_context().expect("context").expect("enabled").ifac_size(), 2);
    }

    #[test]
    fn missing_credentials_or_non_byte_size_fails_closed() {
        assert!(InterfaceSharedConfig {
            ifac_size: Some(16),
            ..InterfaceSharedConfig::default()
        }
        .ifac_context()
        .is_err());
        assert!(InterfaceSharedConfig {
            ifac_size: Some(9),
            network_name: Some("field-net".to_string()),
            ..InterfaceSharedConfig::default()
        }
        .ifac_context()
        .is_err());
    }

    #[test]
    fn authenticated_state_rejects_plaintext_and_bad_tag() {
        let config = InterfaceSharedConfig {
            network_name: Some("field-net".to_string()),
            ..InterfaceSharedConfig::default()
        };
        let state = state(config);
        let raw = [0x01_u8; 32];
        assert!(matches!(decode_ifac(&state, &raw), Err(IfacWireError::MissingFlag)));
        let mut framed = encode_ifac(&state, &raw).expect("encode");
        framed[2] ^= 1;
        assert!(matches!(decode_ifac(&state, &framed), Err(IfacWireError::InvalidTag)));
    }

    #[test]
    fn unauthenticated_state_rejects_authenticated_frames() {
        let authenticated = state(InterfaceSharedConfig {
            network_name: Some("field-net".to_string()),
            ..InterfaceSharedConfig::default()
        });
        let plain = std::sync::Arc::new(std::sync::RwLock::new(None));
        let framed = encode_ifac(&authenticated, &[0x01_u8; 32]).expect("encode");
        assert!(matches!(decode_ifac(&plain, &framed), Err(IfacWireError::UnexpectedFlag)));
    }
}

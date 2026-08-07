pub use primitives::{
    group_decrypt, group_encrypt, Direction, Group, Input, Output, Plain, Single, Type,
};

pub use ratchet::RATCHET_LENGTH;

use ratchet::{try_decrypt_with_ratchets, RatchetState};

pub const NAME_HASH_LENGTH: usize = 10;

pub const RAND_HASH_LENGTH: usize = 10;

pub const PATH_RESPONSE_TAG_WINDOW: u64 = 30;

pub const PATH_RESPONSE_TAG_CAP: usize = 64;

pub const MIN_ANNOUNCE_DATA_LENGTH: usize =
    PUBLIC_KEY_LENGTH * 2 + NAME_HASH_LENGTH + RAND_HASH_LENGTH + SIGNATURE_LENGTH;

/// Mirrors Python Reticulum's `RNS.Destination.PROVE_NONE`/`PROVE_APP`/
/// `PROVE_ALL` (`RNS/Destination.py`) — whether this destination
/// automatically generates a delivery proof for a plain opportunistic
/// `Data` packet it receives (`transport::wire::handle_data`). `None` (the
/// default, matching Python's own default) never proves; `All` always
/// proves; `App` defers to `proof_requested_callback`, called per-packet.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ProofStrategy {
    #[default]
    None,
    App,
    All,
}

/// Mirrors Python Reticulum's `Destination.set_proof_requested_callback` —
/// registered via `Destination::set_proof_requested_callback`, consulted by
/// `transport::wire::handle_data` only when `proof_strategy ==
/// ProofStrategy::App`. Implemented generically for any `Fn(&Packet) ->
/// bool + Send + Sync` closure below, so callers can pass a closure
/// directly; a named trait (rather than a bare `dyn Fn` field) matches this
/// crate's existing `ReceiptHandler` convention (`transport::core`).
///
/// Runs synchronously, inline, while both the transport handler's lock and
/// this destination's own lock are held (see `handle_data`) — must not
/// block and must not attempt to re-lock the destination it was registered
/// on; `tokio::sync::Mutex` is not reentrant.
pub trait ProofRequestedHandler: Send + Sync {
    fn proof_requested(&self, packet: &Packet) -> bool;
}

impl<F: Fn(&Packet) -> bool + Send + Sync> ProofRequestedHandler for F {
    fn proof_requested(&self, packet: &Packet) -> bool {
        self(packet)
    }
}

#[derive(Copy, Clone)]
pub struct DestinationName {
    pub hash: Hash,
}

impl DestinationName {
    pub fn new(app_name: &str, aspects: &str) -> Self {
        let hash = Hash::new(
            Hash::generator()
                .chain_update(app_name.as_bytes())
                .chain_update(".".as_bytes())
                .chain_update(aspects.as_bytes())
                .finalize()
                .into(),
        );

        Self { hash }
    }

    pub fn new_from_hash_slice(hash_slice: &[u8]) -> Self {
        let mut hash = [0u8; 32];
        hash[..hash_slice.len()].copy_from_slice(hash_slice);

        Self { hash: Hash::new(hash) }
    }

    pub fn as_name_hash_slice(&self) -> &[u8] {
        &self.hash.as_slice()[..NAME_HASH_LENGTH]
    }
}

#[derive(Copy, Clone)]
pub struct DestinationDesc {
    pub identity: Identity,
    pub address_hash: AddressHash,
    pub name: DestinationName,
}

impl fmt::Display for DestinationDesc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.address_hash)?;

        Ok(())
    }
}

pub type DestinationAnnounce = Packet;

pub struct AnnounceInfo<'a> {
    pub destination: SingleOutputDestination,
    pub app_data: &'a [u8],
    pub ratchet: Option<[u8; RATCHET_LENGTH]>,
    pub random_blob: [u8; RAND_HASH_LENGTH],
}

impl DestinationAnnounce {
    pub fn validate(packet: &Packet) -> Result<AnnounceInfo<'_>, RnsError> {
        if packet.header.packet_type != PacketType::Announce {
            return Err(RnsError::PacketError);
        }

        let announce_data = packet.data.as_slice();

        if announce_data.len() < MIN_ANNOUNCE_DATA_LENGTH {
            return Err(RnsError::OutOfMemory);
        }

        let mut offset = 0usize;

        let public_key = {
            let mut key_data = [0u8; PUBLIC_KEY_LENGTH];
            key_data.copy_from_slice(&announce_data[offset..(offset + PUBLIC_KEY_LENGTH)]);
            offset += PUBLIC_KEY_LENGTH;
            PublicKey::from(key_data)
        };

        let verifying_key = {
            let mut key_data = [0u8; PUBLIC_KEY_LENGTH];
            key_data.copy_from_slice(&announce_data[offset..(offset + PUBLIC_KEY_LENGTH)]);
            offset += PUBLIC_KEY_LENGTH;

            VerifyingKey::from_bytes(&key_data).map_err(|_| RnsError::CryptoError)?
        };

        let identity = Identity::new(public_key, verifying_key);

        let name_hash = &announce_data[offset..(offset + NAME_HASH_LENGTH)];
        offset += NAME_HASH_LENGTH;
        let rand_hash = &announce_data[offset..(offset + RAND_HASH_LENGTH)];
        let mut random_blob = [0u8; RAND_HASH_LENGTH];
        random_blob.copy_from_slice(rand_hash);
        offset += RAND_HASH_LENGTH;
        let destination = &packet.destination;
        let expected_hash =
            create_address_hash(&identity, &DestinationName::new_from_hash_slice(name_hash));
        if expected_hash != *destination {
            return Err(RnsError::IncorrectHash);
        }

        let verify_announce =
            |ratchet: Option<&[u8]>, signature: &[u8], app_data: &[u8]| -> Result<(), RnsError> {
                // Keeping signed data on stack is only option for now.
                // Verification function doesn't support prehashed message.
                let mut signed_data = PacketDataBuffer::new();
                signed_data
                    .chain_write(destination.as_slice())?
                    .chain_write(public_key.as_bytes())?
                    .chain_write(verifying_key.as_bytes())?
                    .chain_write(name_hash)?
                    .chain_write(rand_hash)?;
                if let Some(ratchet) = ratchet {
                    signed_data.chain_write(ratchet)?;
                }
                if !app_data.is_empty() {
                    signed_data.chain_write(app_data)?;
                }
                let signature =
                    Signature::from_slice(signature).map_err(|_| RnsError::CryptoError)?;
                identity
                    .verify(signed_data.as_slice(), &signature)
                    .map_err(|_| RnsError::IncorrectSignature)
            };

        let remaining = announce_data.len().saturating_sub(offset);
        if remaining < SIGNATURE_LENGTH {
            return Err(RnsError::OutOfMemory);
        }

        let has_ratchet_flag = packet.header.context_flag == ContextFlag::Set;

        let parse_with_ratchet = || -> Result<AnnounceInfo<'_>, RnsError> {
            if remaining < SIGNATURE_LENGTH + RATCHET_LENGTH {
                return Err(RnsError::OutOfMemory);
            }
            let ratchet = &announce_data[offset..offset + RATCHET_LENGTH];
            let sig_start = offset + RATCHET_LENGTH;
            let sig_end = sig_start + SIGNATURE_LENGTH;
            let signature = &announce_data[sig_start..sig_end];
            let app_data = &announce_data[sig_end..];
            verify_announce(Some(ratchet), signature, app_data)?;
            let mut ratchet_bytes = [0u8; RATCHET_LENGTH];
            ratchet_bytes.copy_from_slice(ratchet);
            Ok(AnnounceInfo {
                destination: SingleOutputDestination::new(
                    identity,
                    DestinationName::new_from_hash_slice(name_hash),
                ),
                app_data,
                ratchet: Some(ratchet_bytes),
                random_blob,
            })
        };

        let parse_without_ratchet = || -> Result<AnnounceInfo<'_>, RnsError> {
            let signature = &announce_data[offset..(offset + SIGNATURE_LENGTH)];
            let app_data = &announce_data[(offset + SIGNATURE_LENGTH)..];
            verify_announce(None, signature, app_data)?;

            Ok(AnnounceInfo {
                destination: SingleOutputDestination::new(
                    identity,
                    DestinationName::new_from_hash_slice(name_hash),
                ),
                app_data,
                ratchet: None,
                random_blob,
            })
        };

        if has_ratchet_flag {
            parse_with_ratchet()
        } else {
            parse_without_ratchet()
        }
    }
}

pub struct Destination<I: HashIdentity, D: Direction, T: Type> {
    pub direction: PhantomData<D>,
    pub r#type: PhantomData<T>,
    pub identity: I,
    pub desc: DestinationDesc,
    max_request_size: Option<usize>,
    ratchet_state: RatchetState,
    path_responses: BTreeMap<Vec<u8>, (Instant, Packet)>,
    path_response_queue: VecDeque<(Vec<u8>, Instant)>,
    pub(crate) proof_strategy: ProofStrategy,
    pub(crate) proof_requested_callback: Option<Arc<dyn ProofRequestedHandler>>,
}

impl<I: HashIdentity, D: Direction, T: Type> Destination<I, D, T> {
    pub fn destination_type(&self) -> packet::DestinationType {
        <T as Type>::destination_type()
    }
}

// impl<I: DecryptIdentity + HashIdentity, T: Type> Destination<I, Input, T> {
//     pub fn decrypt<'b, R: CryptoRngCore + Copy>(
//         &self,
//         rng: R,
//         data: &[u8],
//         out_buf: &'b mut [u8],
//     ) -> Result<&'b [u8], RnsError> {
//         self.identity.decrypt(rng, data, out_buf)
//     }
// }

// impl<I: EncryptIdentity + HashIdentity, D: Direction, T: Type> Destination<I, D, T> {
//     pub fn encrypt<'b, R: CryptoRngCore + Copy>(
//         &self,
//         rng: R,
//         text: &[u8],
//         out_buf: &'b mut [u8],
//     ) -> Result<&'b [u8], RnsError> {
//         // self.identity.encrypt(
//         //     rng,
//         //     text,
//         //     Some(self.identity.as_address_hash_slice()),
//         //     out_buf,
//         // )
//     }
// }

pub enum DestinationHandleStatus {
    None,
    LinkProof,
}

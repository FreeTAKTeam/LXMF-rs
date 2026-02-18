pub mod link;
pub mod link_map;

use ed25519_dalek::{Signature, SigningKey, VerifyingKey, SIGNATURE_LENGTH};
use rand_core::{CryptoRngCore, OsRng};
use x25519_dalek::PublicKey;

use core::{fmt, marker::PhantomData};
use std::path::{Path, PathBuf};

use crate::{
    crypt::fernet::{Fernet, PlainText, Token},
    error::RnsError,
    hash::{AddressHash, Hash},
    identity::{EmptyIdentity, HashIdentity, Identity, PrivateIdentity, PUBLIC_KEY_LENGTH},
    packet::{
        self, ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
        PacketDataBuffer, PacketType, PropagationType,
    },
    ratchets::{decrypt_with_identity, decrypt_with_private_key, now_secs},
};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use sha2::Digest;
use x25519_dalek::StaticSecret;

//***************************************************************************//

pub trait Direction {}

pub struct Input;
pub struct Output;

impl Direction for Input {}
impl Direction for Output {}

//***************************************************************************//

pub trait Type {
    fn destination_type() -> DestinationType;
}

pub struct Single;
pub struct Plain;
pub struct Group;

impl Type for Single {
    fn destination_type() -> DestinationType {
        DestinationType::Single
    }
}

impl Type for Plain {
    fn destination_type() -> DestinationType {
        DestinationType::Plain
    }
}

impl Type for Group {
    fn destination_type() -> DestinationType {
        DestinationType::Group
    }
}

pub fn group_encrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, RnsError> {
    let fernet = Fernet::new_from_slices(key, key, OsRng);
    let mut out_buf = vec![0u8; data.len() + 64];
    let token = fernet.encrypt(PlainText::from(data), &mut out_buf)?;
    Ok(token.as_bytes().to_vec())
}

pub fn group_decrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, RnsError> {
    let fernet = Fernet::new_from_slices(key, key, OsRng);
    let token = Token::from(data);
    let verified = fernet.verify(token)?;
    let mut out_buf = vec![0u8; data.len()];
    let plaintext = fernet.decrypt(verified, &mut out_buf)?;
    Ok(plaintext.as_bytes().to_vec())
}

pub const NAME_HASH_LENGTH: usize = 10;
pub const RAND_HASH_LENGTH: usize = 10;
pub const RATCHET_LENGTH: usize = PUBLIC_KEY_LENGTH;
pub const MIN_ANNOUNCE_DATA_LENGTH: usize =
    PUBLIC_KEY_LENGTH * 2 + NAME_HASH_LENGTH + RAND_HASH_LENGTH + SIGNATURE_LENGTH;
const DEFAULT_RATCHET_INTERVAL_SECS: u64 = 30 * 60;
const DEFAULT_RETAINED_RATCHETS: usize = 512;

#[derive(Clone)]
struct RatchetState {
    enabled: bool,
    ratchets: Vec<[u8; RATCHET_LENGTH]>,
    ratchets_path: Option<PathBuf>,
    ratchet_interval_secs: u64,
    retained_ratchets: usize,
    latest_ratchet_time: Option<f64>,
    enforce_ratchets: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedRatchets {
    signature: ByteBuf,
    ratchets: ByteBuf,
}

impl Default for RatchetState {
    fn default() -> Self {
        Self {
            enabled: false,
            ratchets: Vec::new(),
            ratchets_path: None,
            ratchet_interval_secs: DEFAULT_RATCHET_INTERVAL_SECS,
            retained_ratchets: DEFAULT_RETAINED_RATCHETS,
            latest_ratchet_time: None,
            enforce_ratchets: false,
        }
    }
}

impl RatchetState {
    fn enable(&mut self, identity: &PrivateIdentity, path: PathBuf) -> Result<(), RnsError> {
        self.latest_ratchet_time = Some(0.0);
        self.reload(identity, &path)?;
        self.enabled = true;
        self.ratchets_path = Some(path);
        Ok(())
    }

    fn reload(&mut self, identity: &PrivateIdentity, path: &Path) -> Result<(), RnsError> {
        if path.exists() {
            let data = std::fs::read(path).map_err(|_| RnsError::PacketError)?;
            let persisted: PersistedRatchets =
                rmp_serde::from_slice(&data).map_err(|_| RnsError::PacketError)?;
            let signature = Signature::from_slice(persisted.signature.as_ref())
                .map_err(|_| RnsError::CryptoError)?;
            identity
                .verify(persisted.ratchets.as_ref(), &signature)
                .map_err(|_| RnsError::IncorrectSignature)?;
            let decoded: Vec<ByteBuf> = rmp_serde::from_slice(persisted.ratchets.as_ref())
                .map_err(|_| RnsError::PacketError)?;
            let mut ratchets = Vec::new();
            for ratchet in decoded {
                if ratchet.len() == RATCHET_LENGTH {
                    let mut bytes = [0u8; RATCHET_LENGTH];
                    bytes.copy_from_slice(ratchet.as_ref());
                    ratchets.push(bytes);
                }
            }
            self.ratchets = ratchets;
            return Ok(());
        }

        self.ratchets = Vec::new();
        self.persist(identity, path)?;
        Ok(())
    }

    fn persist(&self, identity: &PrivateIdentity, path: &Path) -> Result<(), RnsError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| RnsError::PacketError)?;
        }
        let packed = pack_ratchets(&self.ratchets)?;
        let signature = identity.sign(&packed).to_bytes();
        let persisted = PersistedRatchets {
            signature: ByteBuf::from(signature.to_vec()),
            ratchets: ByteBuf::from(packed),
        };
        let encoded = rmp_serde::to_vec(&persisted).map_err(|_| RnsError::PacketError)?;
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, encoded).map_err(|_| RnsError::PacketError)?;
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        std::fs::rename(&tmp_path, path).map_err(|_| RnsError::PacketError)?;
        Ok(())
    }

    fn rotate_if_needed(&mut self, identity: &PrivateIdentity, now: f64) -> Result<(), RnsError> {
        if !self.enabled {
            return Ok(());
        }
        let last = self.latest_ratchet_time.unwrap_or(0.0);
        if self.ratchets.is_empty() || now > last + self.ratchet_interval_secs as f64 {
            let secret = StaticSecret::random_from_rng(OsRng);
            self.ratchets.insert(0, secret.to_bytes());
            self.latest_ratchet_time = Some(now);
            if self.ratchets.len() > self.retained_ratchets {
                self.ratchets.truncate(self.retained_ratchets);
            }
            if let Some(path) = self.ratchets_path.clone() {
                self.persist(identity, &path)?;
            }
        }
        Ok(())
    }

    fn current_ratchet_public(&self) -> Option<[u8; RATCHET_LENGTH]> {
        let ratchet = self.ratchets.first()?;
        let secret = StaticSecret::from(*ratchet);
        let public = PublicKey::from(&secret);
        let mut bytes = [0u8; RATCHET_LENGTH];
        bytes.copy_from_slice(public.as_bytes());
        Some(bytes)
    }
}

fn pack_ratchets(ratchets: &[[u8; RATCHET_LENGTH]]) -> Result<Vec<u8>, RnsError> {
    let list: Vec<ByteBuf> = ratchets.iter().map(|bytes| ByteBuf::from(bytes.to_vec())).collect();
    rmp_serde::to_vec(&list).map_err(|_| RnsError::PacketError)
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
        offset += RAND_HASH_LENGTH;
        let destination = &packet.destination;
        let expected_hash =
            create_address_hash(&identity, &DestinationName::new_from_hash_slice(name_hash));
        if expected_hash != *destination {
            eprintln!("[announce] dest mismatch expected={} got={}", expected_hash, destination);
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
            })
        };

        if has_ratchet_flag {
            return parse_with_ratchet();
        }

        // Compatibility: some Python announces may include ratchet bytes even when
        // this header flag is not set. Prefer no-ratchet parsing first, then fall
        // back to ratchet parsing if signature verification fails.
        match parse_without_ratchet() {
            Ok(info) => Ok(info),
            Err(err_without_ratchet) => {
                if remaining >= SIGNATURE_LENGTH + RATCHET_LENGTH {
                    parse_with_ratchet().or(Err(err_without_ratchet))
                } else {
                    Err(err_without_ratchet)
                }
            }
        }
    }
}

pub struct Destination<I: HashIdentity, D: Direction, T: Type> {
    pub direction: PhantomData<D>,
    pub r#type: PhantomData<T>,
    pub identity: I,
    pub desc: DestinationDesc,
    ratchet_state: RatchetState,
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

impl Destination<PrivateIdentity, Input, Single> {
    pub fn new(identity: PrivateIdentity, name: DestinationName) -> Self {
        let address_hash = create_address_hash(&identity, &name);
        let pub_identity = *identity.as_identity();

        Self {
            direction: PhantomData,
            r#type: PhantomData,
            identity,
            desc: DestinationDesc { identity: pub_identity, name, address_hash },
            ratchet_state: RatchetState::default(),
        }
    }

    pub fn enable_ratchets<P: AsRef<Path>>(&mut self, path: P) -> Result<(), RnsError> {
        let path = path.as_ref().to_path_buf();
        self.ratchet_state.enable(&self.identity, path)
    }

    pub fn set_retained_ratchets(&mut self, retained: usize) -> Result<(), RnsError> {
        if retained == 0 {
            return Err(RnsError::InvalidArgument);
        }
        self.ratchet_state.retained_ratchets = retained;
        if self.ratchet_state.ratchets.len() > retained {
            self.ratchet_state.ratchets.truncate(retained);
        }
        Ok(())
    }

    pub fn set_ratchet_interval_secs(&mut self, secs: u64) -> Result<(), RnsError> {
        if secs == 0 {
            return Err(RnsError::InvalidArgument);
        }
        self.ratchet_state.ratchet_interval_secs = secs;
        Ok(())
    }

    pub fn enforce_ratchets(&mut self, enforce: bool) {
        self.ratchet_state.enforce_ratchets = enforce;
    }

    pub fn decrypt_with_ratchets(
        &mut self,
        ciphertext: &[u8],
    ) -> Result<(Vec<u8>, bool), RnsError> {
        let salt = self.identity.as_identity().address_hash.as_slice();
        if self.ratchet_state.enabled && !self.ratchet_state.ratchets.is_empty() {
            if let Some(plaintext) =
                try_decrypt_with_ratchets(&self.ratchet_state, salt, ciphertext)
            {
                return Ok((plaintext, true));
            }
            if let Some(path) = self.ratchet_state.ratchets_path.clone() {
                if self.ratchet_state.reload(&self.identity, &path).is_ok() {
                    if let Some(plaintext) =
                        try_decrypt_with_ratchets(&self.ratchet_state, salt, ciphertext)
                    {
                        return Ok((plaintext, true));
                    }
                }
            }
            if self.ratchet_state.enforce_ratchets {
                return Err(RnsError::CryptoError);
            }
        }

        let plaintext = decrypt_with_identity(&self.identity, salt, ciphertext)?;
        Ok((plaintext, false))
    }

    pub fn announce<R: CryptoRngCore + Copy>(
        &mut self,
        rng: R,
        app_data: Option<&[u8]>,
    ) -> Result<Packet, RnsError> {
        let mut packet_data = PacketDataBuffer::new();

        // Python Reticulum encodes announce randomness as 5 random bytes
        // followed by a 5-byte big-endian unix timestamp. Matching this
        // layout keeps announce freshness/path ordering interoperable.
        let mut rand_hash = [0u8; RAND_HASH_LENGTH];
        let mut random_part = [0u8; RAND_HASH_LENGTH / 2];
        let mut rng_mut = rng;
        rng_mut.fill_bytes(&mut random_part);
        rand_hash[..RAND_HASH_LENGTH / 2].copy_from_slice(&random_part);
        let emitted_secs = now_secs().floor() as u64;
        let emitted_be = emitted_secs.to_be_bytes();
        rand_hash[RAND_HASH_LENGTH / 2..].copy_from_slice(&emitted_be[3..8]);

        let pub_key = self.identity.as_identity().public_key_bytes();
        let verifying_key = self.identity.as_identity().verifying_key_bytes();

        let ratchet = if self.ratchet_state.enabled {
            let now = now_secs();
            self.ratchet_state.rotate_if_needed(&self.identity, now)?;
            self.ratchet_state.current_ratchet_public()
        } else {
            None
        };

        packet_data
            .chain_safe_write(self.desc.address_hash.as_slice())
            .chain_safe_write(pub_key)
            .chain_safe_write(verifying_key)
            .chain_safe_write(self.desc.name.as_name_hash_slice())
            .chain_safe_write(&rand_hash);

        if let Some(ratchet) = ratchet {
            packet_data.chain_safe_write(&ratchet);
        }

        if let Some(data) = app_data {
            packet_data.chain_safe_write(data);
        }

        let signature = self.identity.sign(packet_data.as_slice());

        packet_data.reset();

        packet_data
            .chain_safe_write(pub_key)
            .chain_safe_write(verifying_key)
            .chain_safe_write(self.desc.name.as_name_hash_slice())
            .chain_safe_write(&rand_hash);

        if let Some(ratchet) = ratchet {
            packet_data.chain_safe_write(&ratchet);
        }

        packet_data.chain_safe_write(&signature.to_bytes());

        if let Some(data) = app_data {
            packet_data.write(data)?;
        }

        Ok(Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: if ratchet.is_some() { ContextFlag::Set } else { ContextFlag::Unset },
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Announce,
                hops: 0,
            },
            ifac: None,
            destination: self.desc.address_hash,
            transport: None,
            context: PacketContext::None,
            data: packet_data,
        })
    }

    pub fn path_response<R: CryptoRngCore + Copy>(
        &mut self,
        rng: R,
        app_data: Option<&[u8]>,
    ) -> Result<Packet, RnsError> {
        let mut announce = self.announce(rng, app_data)?;
        announce.context = PacketContext::PathResponse;

        Ok(announce)
    }

    pub fn handle_packet(&mut self, packet: &Packet) -> DestinationHandleStatus {
        if self.desc.address_hash != packet.destination {
            return DestinationHandleStatus::None;
        }

        if packet.header.packet_type == PacketType::LinkRequest {
            // TODO: check prove strategy
            return DestinationHandleStatus::LinkProof;
        }

        DestinationHandleStatus::None
    }

    pub fn sign_key(&self) -> &SigningKey {
        self.identity.sign_key()
    }
}

impl Destination<Identity, Output, Single> {
    pub fn new(identity: Identity, name: DestinationName) -> Self {
        let address_hash = create_address_hash(&identity, &name);
        Self {
            direction: PhantomData,
            r#type: PhantomData,
            identity,
            desc: DestinationDesc { identity, name, address_hash },
            ratchet_state: RatchetState::default(),
        }
    }
}

impl<D: Direction> Destination<EmptyIdentity, D, Plain> {
    pub fn new(identity: EmptyIdentity, name: DestinationName) -> Self {
        let address_hash = create_address_hash(&identity, &name);
        Self {
            direction: PhantomData,
            r#type: PhantomData,
            identity,
            desc: DestinationDesc { identity: Default::default(), name, address_hash },
            ratchet_state: RatchetState::default(),
        }
    }
}

fn create_address_hash<I: HashIdentity>(identity: &I, name: &DestinationName) -> AddressHash {
    AddressHash::new_from_hash(&Hash::new(
        Hash::generator()
            .chain_update(name.as_name_hash_slice())
            .chain_update(identity.as_address_hash_slice())
            .finalize()
            .into(),
    ))
}

fn try_decrypt_with_ratchets(
    state: &RatchetState,
    salt: &[u8],
    ciphertext: &[u8],
) -> Option<Vec<u8>> {
    for ratchet in &state.ratchets {
        let secret = StaticSecret::from(*ratchet);
        if let Ok(plaintext) = decrypt_with_private_key(&secret, salt, ciphertext) {
            return Some(plaintext);
        }
    }
    None
}

pub type SingleInputDestination = Destination<PrivateIdentity, Input, Single>;
pub type SingleOutputDestination = Destination<Identity, Output, Single>;
pub type PlainInputDestination = Destination<EmptyIdentity, Input, Plain>;
pub type PlainOutputDestination = Destination<EmptyIdentity, Output, Plain>;

pub fn new_in(identity: PrivateIdentity, app_name: &str, aspect: &str) -> SingleInputDestination {
    SingleInputDestination::new(identity, DestinationName::new(app_name, aspect))
}

pub fn new_out(identity: Identity, app_name: &str, aspect: &str) -> SingleOutputDestination {
    SingleOutputDestination::new(identity, DestinationName::new(app_name, aspect))
}

#[cfg(test)]
mod tests {
    use crate::ratchets::now_secs;
    use core::num::Wrapping;
    use rand_core::OsRng;
    use rand_core::{CryptoRng, RngCore};
    use tempfile::TempDir;

    use crate::buffer::OutputBuffer;
    use crate::error::RnsError;
    use crate::hash::Hash;
    use crate::identity::PrivateIdentity;
    use crate::serde::Serialize;

    use super::DestinationAnnounce;
    use super::DestinationName;
    use super::SingleInputDestination;
    use super::RATCHET_LENGTH;

    #[derive(Clone, Copy)]
    struct FixedRng {
        next: Wrapping<u8>,
    }

    impl FixedRng {
        fn new(seed: u8) -> Self {
            Self { next: Wrapping(seed) }
        }
    }

    impl RngCore for FixedRng {
        fn next_u32(&mut self) -> u32 {
            let mut bytes = [0u8; 4];
            self.fill_bytes(&mut bytes);
            u32::from_le_bytes(bytes)
        }

        fn next_u64(&mut self) -> u64 {
            let mut bytes = [0u8; 8];
            self.fill_bytes(&mut bytes);
            u64::from_le_bytes(bytes)
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for slot in dest.iter_mut() {
                *slot = self.next.0;
                self.next += Wrapping(1);
            }
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    impl CryptoRng for FixedRng {}

    fn decode_announce_random_blob(announce: &crate::packet::Packet) -> [u8; 10] {
        let payload = announce.data.as_slice();
        let start = 32 + 32 + 10;
        let end = start + 10;
        let mut blob = [0u8; 10];
        blob.copy_from_slice(&payload[start..end]);
        blob
    }

    #[test]
    fn create_announce() {
        let identity = PrivateIdentity::new_from_rand(OsRng);

        let mut single_in_destination =
            SingleInputDestination::new(identity, DestinationName::new("test", "in"));

        let announce_packet =
            single_in_destination.announce(OsRng, None).expect("valid announce packet");

        println!("Announce packet {}", announce_packet);
    }

    #[test]
    fn create_path_request_hash() {
        let name = DestinationName::new("rnstransport", "path.request");

        println!("PathRequest Name Hash {}", name.hash);
        println!(
            "PathRequest Destination Hash {}",
            Hash::new_from_slice(name.as_name_hash_slice())
        );
    }

    #[test]
    fn compare_announce() {
        let priv_key: [u8; 32] = [
            0xf0, 0xec, 0xbb, 0xa4, 0x9e, 0x78, 0x3d, 0xee, 0x14, 0xff, 0xc6, 0xc9, 0xf1, 0xe1,
            0x25, 0x1e, 0xfa, 0x7d, 0x76, 0x29, 0xe0, 0xfa, 0x32, 0x41, 0x3c, 0x5c, 0x59, 0xec,
            0x2e, 0x0f, 0x6d, 0x6c,
        ];

        let sign_priv_key: [u8; 32] = [
            0xf0, 0xec, 0xbb, 0xa4, 0x9e, 0x78, 0x3d, 0xee, 0x14, 0xff, 0xc6, 0xc9, 0xf1, 0xe1,
            0x25, 0x1e, 0xfa, 0x7d, 0x76, 0x29, 0xe0, 0xfa, 0x32, 0x41, 0x3c, 0x5c, 0x59, 0xec,
            0x2e, 0x0f, 0x6d, 0x6c,
        ];

        let priv_identity = PrivateIdentity::new(priv_key.into(), sign_priv_key.into());

        println!("identity hash {}", priv_identity.as_identity().address_hash);

        let mut destination = SingleInputDestination::new(
            priv_identity,
            DestinationName::new("example_utilities", "announcesample.fruits"),
        );

        println!("destination name hash {}", destination.desc.name.hash);
        println!("destination hash {}", destination.desc.address_hash);

        let announce = destination.announce(OsRng, None).expect("valid announce packet");

        let mut output_data = [0u8; 4096];
        let mut buffer = OutputBuffer::new(&mut output_data);

        let _ = announce.serialize(&mut buffer).expect("correct data");

        println!("ANNOUNCE {}", buffer);
    }

    #[test]
    fn check_announce() {
        let priv_identity = PrivateIdentity::new_from_rand(OsRng);

        let mut destination = SingleInputDestination::new(
            priv_identity,
            DestinationName::new("example_utilities", "announcesample.fruits"),
        );

        let announce = destination.announce(OsRng, None).expect("valid announce packet");

        DestinationAnnounce::validate(&announce).expect("valid announce");
    }

    #[test]
    fn announce_signature_covers_app_data() {
        let priv_identity = PrivateIdentity::new_from_rand(OsRng);
        let mut destination = SingleInputDestination::new(
            priv_identity,
            DestinationName::new("example_utilities", "announcesample.fruits"),
        );

        let app_data = b"Rust announce app-data";
        let announce = destination.announce(OsRng, Some(app_data)).expect("valid announce packet");

        let mut tampered = announce;
        let payload = tampered.data.as_mut_slice();
        let app_data_offset = 32 + 32 + 10 + 10 + 64;
        assert!(payload.len() > app_data_offset, "announce must include app_data");
        payload[app_data_offset] ^= 0x01;

        match DestinationAnnounce::validate(&tampered) {
            Ok(_) => panic!("tampered app_data should fail signature verification"),
            Err(err) => assert!(matches!(err, RnsError::IncorrectSignature)),
        }
    }

    #[test]
    fn announce_includes_ratchet_when_enabled() {
        let temp = TempDir::new().expect("temp dir");
        let priv_identity = PrivateIdentity::new_from_rand(OsRng);
        let mut destination = SingleInputDestination::new(
            priv_identity,
            DestinationName::new("example_utilities", "announcesample.fruits"),
        );
        let ratchet_path = temp
            .path()
            .join("ratchets")
            .join(format!("{}.ratchets", destination.desc.address_hash.to_hex_string()));
        destination.enable_ratchets(&ratchet_path).expect("enable ratchets");

        let announce = destination.announce(OsRng, None).expect("valid announce packet");
        let info = DestinationAnnounce::validate(&announce).expect("valid announce");
        assert!(info.ratchet.is_some());
    }

    #[test]
    fn announce_without_ratchet_flag_ignores_ratchet_bytes() {
        let priv_identity = PrivateIdentity::new_from_rand(OsRng);
        let mut destination = SingleInputDestination::new(
            priv_identity,
            DestinationName::new("example_utilities", "announcesample.fruits"),
        );

        let app_data = vec![0u8; RATCHET_LENGTH];
        let announce = destination.announce(OsRng, Some(&app_data)).expect("valid announce packet");
        let info = DestinationAnnounce::validate(&announce).expect("valid announce");
        assert!(info.ratchet.is_none());
        assert_eq!(info.app_data, app_data.as_slice());
    }

    #[test]
    fn announce_random_blob_matches_python_layout() {
        let priv_identity = PrivateIdentity::new_from_rand(OsRng);
        let mut destination = SingleInputDestination::new(
            priv_identity,
            DestinationName::new("example_utilities", "announcesample.fruits"),
        );
        let before = now_secs().floor() as u64;
        let announce = destination.announce(FixedRng::new(0x11), None).expect("valid announce");
        let after = now_secs().floor() as u64;

        let blob = decode_announce_random_blob(&announce);
        assert_eq!(&blob[..5], &[0x11, 0x12, 0x13, 0x14, 0x15]);

        let mut ts_bytes = [0u8; 8];
        ts_bytes[3..8].copy_from_slice(&blob[5..10]);
        let emitted = u64::from_be_bytes(ts_bytes);
        assert!(emitted >= before.saturating_sub(1));
        assert!(emitted <= after.saturating_add(1));
    }
}

pub mod link;
pub mod link_map;
use crate::{
    error::RnsError,
    hash::{AddressHash, Hash},
    identity::{EmptyIdentity, HashIdentity, Identity, PrivateIdentity, PUBLIC_KEY_LENGTH},
    packet::{
        self, ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
        PacketDataBuffer, PacketType, PropagationType,
    },
    ratchets::{decrypt_with_identity, now_secs},
};
use core::{fmt, marker::PhantomData};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey, SIGNATURE_LENGTH};
use rand_core::CryptoRngCore;
use sha2::Digest;
use std::path::Path;
use x25519_dalek::PublicKey;

#[path = "destination/primitives.rs"]
mod primitives;
#[path = "destination/ratchet.rs"]
mod ratchet;
#[cfg(test)]
#[path = "destination/tests.rs"]
mod tests;

pub use primitives::{
    group_decrypt, group_encrypt, Direction, Group, Input, Output, Plain, Single, Type,
};
pub use ratchet::RATCHET_LENGTH;
use ratchet::{try_decrypt_with_ratchets, RatchetState};

pub const NAME_HASH_LENGTH: usize = 10;
pub const RAND_HASH_LENGTH: usize = 10;
pub const MIN_ANNOUNCE_DATA_LENGTH: usize =
    PUBLIC_KEY_LENGTH * 2 + NAME_HASH_LENGTH + RAND_HASH_LENGTH + SIGNATURE_LENGTH;

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
            // Current policy: always emit a link proof for addressed link requests.
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

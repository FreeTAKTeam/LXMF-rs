use std::collections::HashMap;
use std::io::Read;
use tokio::time::{Duration, Instant};

use bzip2::read::BzDecoder;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use sha2::Digest;

use crate::crypt::fernet::{FERNET_MAX_PADDING_SIZE, FERNET_OVERHEAD_SIZE};
use crate::destination::link::Link;
use crate::error::RnsError;
use crate::hash::{AddressHash, Hash, ADDRESS_HASH_SIZE, HASH_SIZE};
use crate::packet::DestinationType;
use crate::packet::{Header, Packet, PacketContext, PacketDataBuffer, PacketType, PACKET_MDU};

pub const WINDOW: usize = 4;
pub const MAPHASH_LEN: usize = 4;
pub const RANDOM_HASH_SIZE: usize = 4;
pub const ADVERTISEMENT_OVERHEAD: usize = 134;
const HEADER_MINSIZE: usize = 2 + 1 + ADDRESS_HASH_SIZE;
const HEADER_MAXSIZE: usize = 2 + 1 + (ADDRESS_HASH_SIZE * 2);
const IFAC_MIN_SIZE: usize = 1;
const RETICULUM_MTU: usize = PACKET_MDU + HEADER_MAXSIZE + IFAC_MIN_SIZE;
pub const LINK_PACKET_MDU: usize =
    ((RETICULUM_MTU - IFAC_MIN_SIZE - HEADER_MINSIZE - FERNET_OVERHEAD_SIZE)
        / FERNET_MAX_PADDING_SIZE)
        * FERNET_MAX_PADDING_SIZE
        - 1;
pub const HASHMAP_MAX_LEN: usize =
    (LINK_PACKET_MDU.saturating_sub(ADVERTISEMENT_OVERHEAD)) / MAPHASH_LEN;

const FLAG_ENCRYPTED: u8 = 0x01;
const FLAG_COMPRESSED: u8 = 0x02;
const FLAG_SPLIT: u8 = 0x04;
const FLAG_REQUEST: u8 = 0x08;
const FLAG_RESPONSE: u8 = 0x10;
const FLAG_METADATA: u8 = 0x20;

const METADATA_MAX_SIZE: usize = 16 * 1024 * 1024 - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceStatus {
    None,
    Advertised,
    Transferring,
    AwaitingProof,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceAdvertisement {
    pub transfer_size: u64,
    pub data_size: u64,
    pub parts: u32,
    pub hash: Hash,
    pub random_hash: [u8; RANDOM_HASH_SIZE],
    pub original_hash: Hash,
    pub segment_index: u32,
    pub total_segments: u32,
    pub request_id: Option<ByteBuf>,
    pub flags: u8,
    pub hashmap: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ResourceEvent {
    pub hash: Hash,
    pub link_id: AddressHash,
    pub kind: ResourceEventKind,
}

#[derive(Debug, Clone)]
pub enum ResourceEventKind {
    Progress(ResourceProgress),
    Complete(ResourceComplete),
    OutboundComplete,
}

#[derive(Debug, Clone)]
pub struct ResourceProgress {
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub received_parts: usize,
    pub total_parts: usize,
}

#[derive(Debug, Clone)]
pub struct ResourceComplete {
    pub data: Vec<u8>,
    pub metadata: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResourceAdvertisementFrame {
    #[serde(rename = "t")]
    transfer_size: u64,
    #[serde(rename = "d")]
    data_size: u64,
    #[serde(rename = "n")]
    parts: u32,
    #[serde(rename = "h", with = "serde_bytes")]
    hash: Vec<u8>,
    #[serde(rename = "r", with = "serde_bytes")]
    random_hash: Vec<u8>,
    #[serde(rename = "o", with = "serde_bytes")]
    original_hash: Vec<u8>,
    #[serde(rename = "i")]
    segment_index: u32,
    #[serde(rename = "l")]
    total_segments: u32,
    #[serde(rename = "q")]
    request_id: Option<ByteBuf>,
    #[serde(rename = "f")]
    flags: u8,
    #[serde(rename = "m", with = "serde_bytes")]
    hashmap: Vec<u8>,
}

impl ResourceAdvertisement {
    pub fn pack(&self) -> Result<Vec<u8>, RnsError> {
        let frame = ResourceAdvertisementFrame {
            transfer_size: self.transfer_size,
            data_size: self.data_size,
            parts: self.parts,
            hash: self.hash.as_slice().to_vec(),
            random_hash: self.random_hash.to_vec(),
            original_hash: self.original_hash.as_slice().to_vec(),
            segment_index: self.segment_index,
            total_segments: self.total_segments,
            request_id: self.request_id.clone(),
            flags: self.flags,
            hashmap: self.hashmap.clone(),
        };
        rmp_serde::to_vec_named(&frame).map_err(|_| RnsError::PacketError)
    }

    pub fn unpack(data: &[u8]) -> Result<Self, RnsError> {
        let frame: ResourceAdvertisementFrame =
            rmp_serde::from_slice(data).map_err(|_| RnsError::PacketError)?;
        let hash = Hash::new(copy_hash(&frame.hash)?);
        let original_hash = Hash::new(copy_hash(&frame.original_hash)?);
        let random_hash = copy_fixed::<RANDOM_HASH_SIZE>(&frame.random_hash)?;
        Ok(Self {
            transfer_size: frame.transfer_size,
            data_size: frame.data_size,
            parts: frame.parts,
            hash,
            random_hash,
            original_hash,
            segment_index: frame.segment_index,
            total_segments: frame.total_segments,
            request_id: frame.request_id,
            flags: frame.flags,
            hashmap: frame.hashmap,
        })
    }

    pub fn encrypted(&self) -> bool {
        (self.flags & FLAG_ENCRYPTED) == FLAG_ENCRYPTED
    }

    pub fn compressed(&self) -> bool {
        (self.flags & FLAG_COMPRESSED) == FLAG_COMPRESSED
    }

    pub fn is_request(&self) -> bool {
        (self.flags & FLAG_REQUEST) == FLAG_REQUEST && self.request_id.is_some()
    }

    pub fn is_response(&self) -> bool {
        (self.flags & FLAG_RESPONSE) == FLAG_RESPONSE && self.request_id.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRequest {
    pub hashmap_exhausted: bool,
    pub last_map_hash: Option<[u8; MAPHASH_LEN]>,
    pub resource_hash: Hash,
    pub requested_hashes: Vec<[u8; MAPHASH_LEN]>,
}

impl ResourceRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            1 + MAPHASH_LEN + HASH_SIZE + self.requested_hashes.len() * MAPHASH_LEN,
        );
        if self.hashmap_exhausted {
            out.push(0xFF);
            if let Some(last) = self.last_map_hash {
                out.extend_from_slice(&last);
            } else {
                out.extend_from_slice(&[0u8; MAPHASH_LEN]);
            }
        } else {
            out.push(0x00);
        }
        out.extend_from_slice(self.resource_hash.as_slice());
        for hash in &self.requested_hashes {
            out.extend_from_slice(hash);
        }
        out
    }

    pub fn decode(data: &[u8]) -> Result<Self, RnsError> {
        if data.len() < 1 + HASH_SIZE {
            return Err(RnsError::PacketError);
        }
        let hashmap_exhausted = data[0] == 0xFF;
        let mut offset = 1;
        let last_map_hash = if hashmap_exhausted {
            if data.len() < 1 + MAPHASH_LEN + HASH_SIZE {
                return Err(RnsError::PacketError);
            }
            let mut last = [0u8; MAPHASH_LEN];
            last.copy_from_slice(&data[offset..offset + MAPHASH_LEN]);
            offset += MAPHASH_LEN;
            Some(last)
        } else {
            None
        };
        let resource_hash = Hash::new(copy_hash(&data[offset..offset + HASH_SIZE])?);
        offset += HASH_SIZE;
        let mut requested_hashes = Vec::new();
        while offset + MAPHASH_LEN <= data.len() {
            let mut entry = [0u8; MAPHASH_LEN];
            entry.copy_from_slice(&data[offset..offset + MAPHASH_LEN]);
            requested_hashes.push(entry);
            offset += MAPHASH_LEN;
        }
        Ok(Self { hashmap_exhausted, last_map_hash, resource_hash, requested_hashes })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceHashUpdate {
    pub resource_hash: Hash,
    pub segment: u32,
    pub hashmap: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResourceHashUpdateFrame(u32, #[serde(with = "serde_bytes")] Vec<u8>);

impl ResourceHashUpdate {
    pub fn encode(&self) -> Result<Vec<u8>, RnsError> {
        let mut out = Vec::with_capacity(HASH_SIZE + self.hashmap.len() + 8);
        out.extend_from_slice(self.resource_hash.as_slice());
        let payload =
            rmp_serde::to_vec(&ResourceHashUpdateFrame(self.segment, self.hashmap.clone()))
                .map_err(|_| RnsError::PacketError)?;
        out.extend_from_slice(&payload);
        Ok(out)
    }

    pub fn decode(data: &[u8]) -> Result<Self, RnsError> {
        if data.len() < HASH_SIZE + 1 {
            return Err(RnsError::PacketError);
        }
        let resource_hash = Hash::new(copy_hash(&data[..HASH_SIZE])?);
        let frame: ResourceHashUpdateFrame =
            rmp_serde::from_slice(&data[HASH_SIZE..]).map_err(|_| RnsError::PacketError)?;
        Ok(Self { resource_hash, segment: frame.0, hashmap: frame.1 })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceProof {
    pub resource_hash: Hash,
    pub proof: Hash,
}

impl ResourceProof {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HASH_SIZE * 2);
        out.extend_from_slice(self.resource_hash.as_slice());
        out.extend_from_slice(self.proof.as_slice());
        out
    }

    pub fn decode(data: &[u8]) -> Result<Self, RnsError> {
        if data.len() < HASH_SIZE * 2 {
            return Err(RnsError::PacketError);
        }
        let resource_hash = Hash::new(copy_hash(&data[..HASH_SIZE])?);
        let proof = Hash::new(copy_hash(&data[HASH_SIZE..HASH_SIZE * 2])?);
        Ok(Self { resource_hash, proof })
    }
}

include!("resource/sender.rs");
include!("resource/receiver.rs");
include!("resource/manager.rs");
include!("resource/utils.rs");
include!("resource/tests.rs");

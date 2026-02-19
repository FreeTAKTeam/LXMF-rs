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

#[derive(Debug, Clone)]
struct ResourceSender {
    resource_hash: Hash,
    random_hash: [u8; RANDOM_HASH_SIZE],
    original_hash: Hash,
    parts: Vec<Vec<u8>>,
    map_hashes: Vec<[u8; MAPHASH_LEN]>,
    expected_proof: Hash,
    data_size: u64,
    has_metadata: bool,
    status: ResourceStatus,
}

impl ResourceSender {
    fn new(link: &Link, data: Vec<u8>, metadata: Option<Vec<u8>>) -> Result<Self, RnsError> {
        let has_metadata = metadata.is_some();
        let metadata_prefix = if let Some(payload) = metadata.as_ref() {
            if payload.len() > METADATA_MAX_SIZE {
                return Err(RnsError::InvalidArgument);
            }
            let size = payload.len() as u32;
            let size_bytes = size.to_be_bytes();
            let mut prefix = Vec::with_capacity(3 + payload.len());
            prefix.extend_from_slice(&size_bytes[1..]);
            prefix.extend_from_slice(payload);
            prefix
        } else {
            Vec::new()
        };
        let mut combined = metadata_prefix.clone();
        combined.extend_from_slice(&data);
        let random_hash = random_bytes::<RANDOM_HASH_SIZE>();
        let data_size = combined.len() as u64;
        let mut hasher = sha2::Sha256::new();
        hasher.update(&combined);
        hasher.update(random_hash);
        let resource_hash = Hash::new(copy_hash(&hasher.finalize())?);

        let mut proof_hasher = sha2::Sha256::new();
        proof_hasher.update(&combined);
        proof_hasher.update(resource_hash.as_slice());
        let expected_proof = Hash::new(copy_hash(&proof_hasher.finalize())?);

        let mut prefix = random_bytes::<RANDOM_HASH_SIZE>().to_vec();
        prefix.extend_from_slice(&combined);

        let mut cipher_buf = vec![0u8; prefix.len() + 128];
        let cipher = link.encrypt(&prefix, &mut cipher_buf).map_err(|_| RnsError::CryptoError)?;
        let cipher_text = cipher.to_vec();

        let mut parts = Vec::new();
        for chunk in cipher_text.chunks(PACKET_MDU) {
            parts.push(chunk.to_vec());
        }

        let mut map_hashes = Vec::with_capacity(parts.len());
        for part in &parts {
            map_hashes.push(map_hash(part, &random_hash));
        }

        Ok(Self {
            resource_hash,
            random_hash,
            original_hash: resource_hash,
            parts,
            map_hashes,
            expected_proof,
            data_size,
            has_metadata,
            status: ResourceStatus::Advertised,
        })
    }

    fn advertisement(&self, segment: usize) -> ResourceAdvertisement {
        let hashmap = slice_hashmap_segment(&self.map_hashes, segment);
        let mut flags = FLAG_ENCRYPTED;
        if self.has_metadata {
            flags |= FLAG_METADATA;
        }
        ResourceAdvertisement {
            transfer_size: self.parts.iter().map(|part| part.len() as u64).sum(),
            data_size: self.data_size,
            parts: self.parts.len() as u32,
            hash: self.resource_hash,
            random_hash: self.random_hash,
            original_hash: self.original_hash,
            segment_index: segment as u32 + 1,
            total_segments: 1,
            request_id: None,
            flags,
            hashmap,
        }
    }

    fn handle_request(&mut self, request: &ResourceRequest, link: &Link) -> Vec<Packet> {
        if request.resource_hash != self.resource_hash {
            return Vec::new();
        }

        let mut packets = Vec::new();
        for hash in &request.requested_hashes {
            if let Some(index) = self.map_hashes.iter().position(|entry| entry == hash) {
                if let Some(part) = self.parts.get(index) {
                    if let Ok(packet) =
                        build_link_packet(link, PacketType::Data, PacketContext::Resource, part)
                    {
                        packets.push(packet);
                    } else {
                        log::warn!("resource: failed to build resource packet");
                    }
                }
            }
        }

        if request.hashmap_exhausted {
            if let Some(last_hash) = request.last_map_hash {
                if let Some(last_index) =
                    self.map_hashes.iter().position(|entry| *entry == last_hash)
                {
                    let next_segment = (last_index / HASHMAP_MAX_LEN) + 1;
                    if next_segment * HASHMAP_MAX_LEN < self.map_hashes.len() {
                        let update = ResourceHashUpdate {
                            resource_hash: self.resource_hash,
                            segment: next_segment as u32,
                            hashmap: slice_hashmap_segment(&self.map_hashes, next_segment),
                        };
                        if let Ok(payload) = update.encode() {
                            if let Ok(packet) = build_link_packet(
                                link,
                                PacketType::Data,
                                PacketContext::ResourceHashUpdate,
                                &payload,
                            ) {
                                packets.push(packet);
                            } else {
                                log::warn!("resource: failed to build hash update packet");
                            }
                        }
                    }
                }
            }
        }

        if self.status == ResourceStatus::Advertised || self.status == ResourceStatus::Transferring
        {
            self.status = ResourceStatus::Transferring;
        }

        packets
    }

    fn handle_proof(&mut self, proof: &ResourceProof) -> bool {
        if proof.resource_hash != self.resource_hash {
            return false;
        }
        if proof.proof == self.expected_proof {
            self.status = ResourceStatus::Complete;
            return true;
        }
        false
    }
}

#[derive(Debug, Clone)]
struct ResourceReceiver {
    resource_hash: Hash,
    link_id: AddressHash,
    random_hash: [u8; RANDOM_HASH_SIZE],
    parts: Vec<Option<Vec<u8>>>,
    hashmap: Vec<Option<[u8; MAPHASH_LEN]>>,
    received: usize,
    received_bytes: u64,
    total_bytes: u64,
    encrypted: bool,
    compressed: bool,
    split: bool,
    has_metadata: bool,
    last_progress: Instant,
    last_request: Instant,
    retry_count: u8,
    status: ResourceStatus,
}

#[derive(Debug, Clone)]
struct ResourcePayload {
    data: Vec<u8>,
    metadata: Option<Vec<u8>>,
}

#[allow(clippy::large_enum_variant)]
enum PartOutcome {
    NoMatch,
    Incomplete,
    Complete(Packet, ResourcePayload),
}

impl ResourceReceiver {
    fn new(adv: &ResourceAdvertisement, link_id: AddressHash) -> Self {
        let now = Instant::now();
        let total_parts = adv.parts as usize;
        let mut receiver = Self {
            resource_hash: adv.hash,
            link_id,
            random_hash: adv.random_hash,
            parts: vec![None; total_parts],
            hashmap: vec![None; total_parts],
            received: 0,
            received_bytes: 0,
            total_bytes: adv.transfer_size,
            encrypted: adv.encrypted(),
            compressed: adv.compressed(),
            split: (adv.flags & FLAG_SPLIT) == FLAG_SPLIT,
            has_metadata: (adv.flags & FLAG_METADATA) == FLAG_METADATA,
            last_progress: now,
            last_request: now,
            retry_count: 0,
            status: ResourceStatus::Advertised,
        };
        receiver.apply_hashmap_segment(adv.segment_index.saturating_sub(1) as usize, &adv.hashmap);
        receiver
    }

    fn apply_hashmap_segment(&mut self, segment: usize, bytes: &[u8]) {
        let hashes = bytes.len() / MAPHASH_LEN;
        for i in 0..hashes {
            let start = i * MAPHASH_LEN;
            let mut entry = [0u8; MAPHASH_LEN];
            entry.copy_from_slice(&bytes[start..start + MAPHASH_LEN]);
            let idx = segment * HASHMAP_MAX_LEN + i;
            if idx < self.hashmap.len() {
                self.hashmap[idx] = Some(entry);
            }
        }
    }

    fn build_request(&self) -> ResourceRequest {
        let mut requested = Vec::new();
        let mut last_known: Option<[u8; MAPHASH_LEN]> = None;
        let mut hashmap_exhausted = false;

        for (idx, entry) in self.hashmap.iter().enumerate() {
            if let Some(hash) = entry {
                last_known = Some(*hash);
                if self.parts[idx].is_none() {
                    requested.push(*hash);
                    if requested.len() >= WINDOW {
                        break;
                    }
                }
            } else {
                hashmap_exhausted = true;
                break;
            }
        }

        ResourceRequest {
            hashmap_exhausted,
            last_map_hash: if hashmap_exhausted { last_known } else { None },
            resource_hash: self.resource_hash,
            requested_hashes: requested,
        }
    }

    fn handle_hash_update(&mut self, update: &ResourceHashUpdate) {
        if update.resource_hash != self.resource_hash {
            return;
        }
        self.apply_hashmap_segment(update.segment as usize, &update.hashmap);
    }

    fn handle_part(&mut self, part: &[u8], link: &Link) -> PartOutcome {
        if self.split {
            self.status = ResourceStatus::Failed;
            return PartOutcome::Incomplete;
        }

        let hash = map_hash(part, &self.random_hash);
        let Some(index) = self.hashmap.iter().position(|entry| entry.as_ref() == Some(&hash))
        else {
            return PartOutcome::NoMatch;
        };

        if self.parts[index].is_none() {
            self.parts[index] = Some(part.to_vec());
            self.received += 1;
            self.received_bytes = self.received_bytes.saturating_add(part.len() as u64);
            self.last_progress = Instant::now();
        }

        if self.received == self.parts.len() && !self.parts.is_empty() {
            let mut stream = Vec::new();
            for part in &self.parts {
                if let Some(bytes) = part {
                    stream.extend_from_slice(bytes);
                } else {
                    return PartOutcome::Incomplete;
                }
            }

            let plain = if self.encrypted {
                let mut out = vec![0u8; stream.len() + 64];
                let decrypted = match link.decrypt(&stream, &mut out) {
                    Ok(value) => value,
                    Err(_) => {
                        self.status = ResourceStatus::Failed;
                        return PartOutcome::Incomplete;
                    }
                };
                decrypted.to_vec()
            } else {
                stream
            };

            let mut payload = if plain.len() > RANDOM_HASH_SIZE {
                plain[RANDOM_HASH_SIZE..].to_vec()
            } else {
                Vec::new()
            };

            if self.compressed {
                let mut decoder = BzDecoder::new(payload.as_slice());
                let mut decompressed = Vec::new();
                if decoder.read_to_end(&mut decompressed).is_err() {
                    self.status = ResourceStatus::Failed;
                    return PartOutcome::Incomplete;
                }
                payload = decompressed;
            }

            let (metadata, data_payload) = if self.has_metadata && payload.len() >= 3 {
                let size = ((payload[0] as usize) << 16)
                    | ((payload[1] as usize) << 8)
                    | payload[2] as usize;
                if size > METADATA_MAX_SIZE {
                    self.status = ResourceStatus::Failed;
                    return PartOutcome::Incomplete;
                }
                if payload.len() >= 3 + size {
                    let meta = payload[3..3 + size].to_vec();
                    let data = payload[3 + size..].to_vec();
                    (Some(meta), data)
                } else {
                    (None, payload.clone())
                }
            } else {
                (None, payload.clone())
            };

            let mut hasher = sha2::Sha256::new();
            hasher.update(&payload);
            hasher.update(self.random_hash);
            let computed = match copy_hash(&hasher.finalize()) {
                Ok(hash) => Hash::new(hash),
                Err(_) => {
                    self.status = ResourceStatus::Failed;
                    return PartOutcome::Incomplete;
                }
            };

            if computed == self.resource_hash {
                let mut proof_hasher = sha2::Sha256::new();
                proof_hasher.update(&payload);
                proof_hasher.update(self.resource_hash.as_slice());
                let proof = match copy_hash(&proof_hasher.finalize()) {
                    Ok(hash) => Hash::new(hash),
                    Err(_) => {
                        self.status = ResourceStatus::Failed;
                        return PartOutcome::Incomplete;
                    }
                };
                let proof_payload = ResourceProof { resource_hash: self.resource_hash, proof };
                self.status = ResourceStatus::Complete;
                let packet = match build_link_packet(
                    link,
                    PacketType::Proof,
                    PacketContext::ResourceProof,
                    &proof_payload.encode(),
                ) {
                    Ok(packet) => packet,
                    Err(_) => {
                        log::warn!("resource: failed to build proof packet");
                        self.status = ResourceStatus::Failed;
                        return PartOutcome::Incomplete;
                    }
                };
                return PartOutcome::Complete(
                    packet,
                    ResourcePayload { data: data_payload, metadata },
                );
            } else {
                self.status = ResourceStatus::Failed;
            }
        }

        PartOutcome::Incomplete
    }

    fn mark_request(&mut self) {
        self.last_request = Instant::now();
        self.retry_count = self.retry_count.saturating_add(1);
    }

    fn retry_due(&self, now: Instant, retry_interval: Duration, max_retries: u8) -> bool {
        if self.status == ResourceStatus::Complete || self.status == ResourceStatus::Failed {
            return false;
        }
        if self.retry_count >= max_retries {
            return false;
        }
        now.duration_since(self.last_progress) >= retry_interval
            && now.duration_since(self.last_request) >= retry_interval
    }

    fn progress(&self) -> ResourceProgress {
        ResourceProgress {
            received_bytes: self.received_bytes,
            total_bytes: self.total_bytes,
            received_parts: self.received,
            total_parts: self.parts.len(),
        }
    }
}

#[derive(Debug)]
pub struct ResourceManager {
    outgoing: HashMap<Hash, ResourceSender>,
    incoming: HashMap<Hash, ResourceReceiver>,
    events: Vec<ResourceEvent>,
    retry_interval: Duration,
    retry_limit: u8,
}

impl ResourceManager {
    pub fn new() -> Self {
        Self::new_with_config(Duration::from_secs(2), 5)
    }

    pub fn new_with_config(retry_interval: Duration, retry_limit: u8) -> Self {
        Self {
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            events: Vec::new(),
            retry_interval,
            retry_limit,
        }
    }

    pub fn start_send(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<(Hash, Packet), RnsError> {
        let sender = ResourceSender::new(link, data, metadata)?;
        let resource_hash = sender.resource_hash;
        let advertisement = sender.advertisement(0);
        let payload = advertisement.pack()?;
        let packet = build_link_packet(
            link,
            PacketType::Data,
            PacketContext::ResourceAdvrtisement,
            &payload,
        )?;
        self.outgoing.insert(resource_hash, sender);
        Ok((resource_hash, packet))
    }

    pub fn drain_events(&mut self) -> Vec<ResourceEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn retry_requests(&mut self, now: Instant) -> Vec<(AddressHash, ResourceRequest)> {
        let mut requests = Vec::new();
        let mut failed = Vec::new();
        for (hash, receiver) in self.incoming.iter_mut() {
            if receiver.retry_due(now, self.retry_interval, self.retry_limit) {
                let request = receiver.build_request();
                receiver.mark_request();
                requests.push((receiver.link_id, request));
            }
            if receiver.retry_count >= self.retry_limit {
                failed.push(*hash);
            }
        }
        for hash in failed {
            self.incoming.remove(&hash);
        }
        requests
    }

    pub fn handle_packet(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        match packet.context {
            PacketContext::ResourceAdvrtisement => self.handle_advertisement(packet, link),
            PacketContext::ResourceRequest => self.handle_request(packet, link),
            PacketContext::ResourceHashUpdate => self.handle_hash_update(packet, link),
            PacketContext::Resource => self.handle_resource_part(packet, link),
            PacketContext::ResourceProof => self.handle_proof(packet),
            PacketContext::ResourceInitiatorCancel | PacketContext::ResourceReceiverCancel => {
                self.cancel(packet)
            }
            _ => Vec::new(),
        }
    }

    fn handle_advertisement(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let Ok(advertisement) = ResourceAdvertisement::unpack(packet.data.as_slice()) else {
            return Vec::new();
        };
        if (advertisement.flags & FLAG_SPLIT) == FLAG_SPLIT {
            log::warn!(
                "resource: rejecting unsupported advertisement flags (split={})",
                (advertisement.flags & FLAG_SPLIT) == FLAG_SPLIT
            );
            return Vec::new();
        }
        let resource_hash = advertisement.hash;
        let mut receiver = ResourceReceiver::new(&advertisement, *link.id());
        let request = receiver.build_request();
        receiver.mark_request();
        self.incoming.insert(resource_hash, receiver);
        match build_link_packet(
            link,
            PacketType::Data,
            PacketContext::ResourceRequest,
            &request.encode(),
        ) {
            Ok(packet) => vec![packet],
            Err(_) => {
                log::warn!("resource: failed to build request packet");
                Vec::new()
            }
        }
    }

    fn handle_request(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let Ok(request) = ResourceRequest::decode(packet.data.as_slice()) else {
            return Vec::new();
        };
        if let Some(sender) = self.outgoing.get_mut(&request.resource_hash) {
            sender.handle_request(&request, link)
        } else {
            Vec::new()
        }
    }

    fn handle_hash_update(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let Ok(update) = ResourceHashUpdate::decode(packet.data.as_slice()) else {
            return Vec::new();
        };
        if let Some(receiver) = self.incoming.get_mut(&update.resource_hash) {
            receiver.handle_hash_update(&update);
            let request = receiver.build_request();
            return match build_link_packet(
                link,
                PacketType::Data,
                PacketContext::ResourceRequest,
                &request.encode(),
            ) {
                Ok(packet) => vec![packet],
                Err(_) => {
                    log::warn!("resource: failed to build request packet");
                    Vec::new()
                }
            };
        }
        Vec::new()
    }

    fn handle_resource_part(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let mut completed: Option<Hash> = None;
        let mut proof_packet: Option<Packet> = None;
        let mut request_packet: Option<Packet> = None;
        let mut payload: Option<ResourcePayload> = None;
        for (hash, receiver) in self.incoming.iter_mut() {
            let before_received = receiver.received;
            match receiver.handle_part(packet.data.as_slice(), link) {
                PartOutcome::NoMatch => continue,
                PartOutcome::Complete(packet, data_payload) => {
                    completed = Some(*hash);
                    proof_packet = Some(packet);
                    payload = Some(data_payload);
                    break;
                }
                PartOutcome::Incomplete => {
                    let request = receiver.build_request();
                    receiver.mark_request();
                    request_packet = match build_link_packet(
                        link,
                        PacketType::Data,
                        PacketContext::ResourceRequest,
                        &request.encode(),
                    ) {
                        Ok(packet) => Some(packet),
                        Err(_) => {
                            log::warn!("resource: failed to build request packet");
                            None
                        }
                    };
                    if receiver.received > before_received {
                        self.events.push(ResourceEvent {
                            hash: *hash,
                            link_id: receiver.link_id,
                            kind: ResourceEventKind::Progress(receiver.progress()),
                        });
                    }
                    break;
                }
            }
        }
        if let Some(hash) = completed {
            self.incoming.remove(&hash);
            if let Some(payload) = payload {
                self.events.push(ResourceEvent {
                    hash,
                    link_id: *link.id(),
                    kind: ResourceEventKind::Complete(ResourceComplete {
                        data: payload.data,
                        metadata: payload.metadata,
                    }),
                });
            }
        }
        if let Some(packet) = proof_packet {
            return vec![packet];
        }
        if let Some(packet) = request_packet {
            return vec![packet];
        }
        Vec::new()
    }

    fn handle_proof(&mut self, packet: &Packet) -> Vec<Packet> {
        let Ok(proof) = ResourceProof::decode(packet.data.as_slice()) else {
            return Vec::new();
        };
        if let Some(sender) = self.outgoing.get_mut(&proof.resource_hash) {
            if sender.handle_proof(&proof) {
                self.outgoing.remove(&proof.resource_hash);
                self.events.push(ResourceEvent {
                    hash: proof.resource_hash,
                    link_id: packet.destination,
                    kind: ResourceEventKind::OutboundComplete,
                });
            }
        }
        Vec::new()
    }

    fn cancel(&mut self, packet: &Packet) -> Vec<Packet> {
        if let Ok(hash_bytes) = copy_hash(packet.data.as_slice()) {
            let hash = Hash::new(hash_bytes);
            self.incoming.remove(&hash);
            self.outgoing.remove(&hash);
        }
        Vec::new()
    }
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}

fn build_link_packet(
    link: &Link,
    packet_type: PacketType,
    context: PacketContext,
    payload: &[u8],
) -> Result<Packet, RnsError> {
    let mut packet_data = PacketDataBuffer::new();
    let should_encrypt = context != PacketContext::Resource
        && !(packet_type == PacketType::Proof && context == PacketContext::ResourceProof);
    if should_encrypt {
        let cipher_text_len = {
            let cipher_text = link.encrypt(payload, packet_data.accuire_buf_max())?;
            cipher_text.len()
        };
        packet_data.resize(cipher_text_len);
    } else {
        packet_data.write(payload)?;
    }
    Ok(Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type,
            ..Default::default()
        },
        ifac: None,
        destination: *link.id(),
        transport: None,
        context,
        data: packet_data,
    })
}

pub(crate) fn build_resource_request_packet(link: &Link, request: &ResourceRequest) -> Packet {
    build_link_packet(link, PacketType::Data, PacketContext::ResourceRequest, &request.encode())
        .expect("resource request packet")
}

fn slice_hashmap_segment(hashes: &[[u8; MAPHASH_LEN]], segment: usize) -> Vec<u8> {
    let start = segment * HASHMAP_MAX_LEN;
    let end = usize::min((segment + 1) * HASHMAP_MAX_LEN, hashes.len());
    let mut out = Vec::with_capacity((end - start) * MAPHASH_LEN);
    for hash in &hashes[start..end] {
        out.extend_from_slice(hash);
    }
    out
}

fn map_hash(part: &[u8], random_hash: &[u8; RANDOM_HASH_SIZE]) -> [u8; MAPHASH_LEN] {
    let mut hasher = sha2::Sha256::new();
    hasher.update(part);
    hasher.update(random_hash);
    let digest = hasher.finalize();
    let mut out = [0u8; MAPHASH_LEN];
    out.copy_from_slice(&digest[..MAPHASH_LEN]);
    out
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    OsRng.fill_bytes(&mut out);
    out
}

fn copy_hash(bytes: &[u8]) -> Result<[u8; HASH_SIZE], RnsError> {
    copy_fixed::<HASH_SIZE>(bytes)
}

fn copy_fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], RnsError> {
    if bytes.len() < N {
        return Err(RnsError::PacketError);
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[..N]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::{DestinationDesc, DestinationName};
    use crate::identity::PrivateIdentity;
    use rand_core::OsRng;

    #[test]
    fn resource_sender_rejects_oversized_metadata() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let link = Link::new(destination, tx);
        let data = vec![0u8; 4];
        let metadata = vec![0u8; METADATA_MAX_SIZE + 1];

        let result = ResourceSender::new(&link, data, Some(metadata));
        assert!(matches!(result, Err(RnsError::InvalidArgument)));
    }

    #[test]
    fn resource_manager_rejects_split_flag() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let adv = ResourceAdvertisement {
            transfer_size: 1,
            data_size: 1,
            parts: 1,
            hash: Hash::new_from_slice(&[1, 2, 3, 4]),
            random_hash: [0u8; RANDOM_HASH_SIZE],
            original_hash: Hash::new_from_slice(&[1, 2, 3, 4]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: FLAG_SPLIT,
            hashmap: vec![0u8; MAPHASH_LEN],
        };

        let packet = build_link_packet(
            &link,
            PacketType::Data,
            PacketContext::ResourceAdvrtisement,
            &adv.pack().expect("advertisement"),
        )
        .expect("resource advertisement packet");

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let responses = manager.handle_packet(&packet, &mut link);

        assert!(responses.is_empty());
        assert!(manager.incoming.is_empty());
    }
}

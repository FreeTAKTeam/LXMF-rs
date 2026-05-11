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
    data_size: u64,
    encrypted: bool,
    compressed: bool,
    split: bool,
    has_metadata: bool,
    request_id: Option<Vec<u8>>,
    is_request: bool,
    is_response: bool,
    last_progress: Instant,
    last_request: Instant,
    retry_count: u8,
    status: ResourceStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct ResourcePayload {
    pub(crate) data: Vec<u8>,
    pub(crate) metadata: Option<Vec<u8>>,
    pub(crate) request_id: Option<Vec<u8>>,
    pub(crate) is_request: bool,
    pub(crate) is_response: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ResourceCompletionJob {
    pub(crate) resource_hash: Hash,
    pub(crate) link_id: AddressHash,
    random_hash: [u8; RANDOM_HASH_SIZE],
    encrypted: bool,
    compressed: bool,
    has_metadata: bool,
    data_size: u64,
    request_id: Option<Vec<u8>>,
    is_request: bool,
    is_response: bool,
    stream: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct ResourceCompletionSnapshot {
    pub(crate) resource_hash: [u8; HASH_SIZE],
    pub(crate) link_id: [u8; ADDRESS_HASH_SIZE],
    pub(crate) random_hash: [u8; RANDOM_HASH_SIZE],
    pub(crate) encrypted: bool,
    pub(crate) compressed: bool,
    pub(crate) has_metadata: bool,
    pub(crate) data_size: u64,
    pub(crate) request_id: Option<ByteBuf>,
    pub(crate) is_request: bool,
    pub(crate) is_response: bool,
    pub(crate) stream: ByteBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceCompletionOutcome {
    pub(crate) resource_hash: [u8; HASH_SIZE],
    pub(crate) proof: [u8; HASH_SIZE],
    pub(crate) data: Vec<u8>,
    pub(crate) metadata: Option<Vec<u8>>,
    pub(crate) request_id: Option<Vec<u8>>,
    pub(crate) is_request: bool,
    pub(crate) is_response: bool,
}

impl ResourceCompletionJob {
    #[allow(dead_code)]
    pub(crate) fn to_snapshot(&self) -> ResourceCompletionSnapshot {
        let mut resource_hash = [0u8; HASH_SIZE];
        resource_hash.copy_from_slice(self.resource_hash.as_slice());
        let mut link_id = [0u8; ADDRESS_HASH_SIZE];
        link_id.copy_from_slice(self.link_id.as_slice());
        ResourceCompletionSnapshot {
            resource_hash,
            link_id,
            random_hash: self.random_hash,
            encrypted: self.encrypted,
            compressed: self.compressed,
            has_metadata: self.has_metadata,
            data_size: self.data_size,
            request_id: self.request_id.clone().map(ByteBuf::from),
            is_request: self.is_request,
            is_response: self.is_response,
            stream: ByteBuf::from(self.stream.clone()),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_snapshot(snapshot: ResourceCompletionSnapshot) -> Self {
        Self {
            resource_hash: Hash::new(snapshot.resource_hash),
            link_id: AddressHash::new(snapshot.link_id),
            random_hash: snapshot.random_hash,
            encrypted: snapshot.encrypted,
            compressed: snapshot.compressed,
            has_metadata: snapshot.has_metadata,
            data_size: snapshot.data_size,
            request_id: snapshot.request_id.map(|value| value.to_vec()),
            is_request: snapshot.is_request,
            is_response: snapshot.is_response,
            stream: snapshot.stream.to_vec(),
        }
    }
}

impl ResourceCompletionSnapshot {
    #[allow(dead_code)]
    pub(crate) fn complete_with<F>(self, decrypt: F) -> Result<ResourceCompletionOutcome, ()>
    where
        F: FnOnce(&[u8]) -> Result<Vec<u8>, ()>,
    {
        let (proof, payload) = complete_resource_job(ResourceCompletionJob::from_snapshot(self), decrypt)?;
        let mut resource_hash = [0u8; HASH_SIZE];
        resource_hash.copy_from_slice(proof.resource_hash.as_slice());
        let mut proof_hash = [0u8; HASH_SIZE];
        proof_hash.copy_from_slice(proof.proof.as_slice());
        Ok(ResourceCompletionOutcome {
            resource_hash,
            proof: proof_hash,
            data: payload.data,
            metadata: payload.metadata,
            request_id: payload.request_id,
            is_request: payload.is_request,
            is_response: payload.is_response,
        })
    }
}

#[cfg(test)]
impl ResourceCompletionJob {
    pub(crate) fn unencrypted_for_test(link_id: AddressHash, payload: &[u8]) -> Self {
        let random_hash = [0x5a; RANDOM_HASH_SIZE];
        let mut hasher = sha2::Sha256::new();
        hasher.update(payload);
        hasher.update(random_hash);
        let resource_hash =
            Hash::new(copy_hash(&hasher.finalize()).expect("test resource hash should fit"));
        let mut stream = random_hash.to_vec();
        stream.extend_from_slice(payload);

        Self {
            resource_hash,
            link_id,
            random_hash,
            encrypted: false,
            compressed: false,
            has_metadata: false,
            data_size: payload.len() as u64,
            request_id: None,
            is_request: false,
            is_response: false,
            stream,
        }
    }
}

#[cfg(test)]
mod resource_completion_snapshot_tests {
    use super::*;

    #[test]
    fn resource_completion_job_round_trips_through_snapshot() {
        let link_id = AddressHash::new_from_slice(b"snapshot link");
        let job = ResourceCompletionJob::unencrypted_for_test(link_id, b"snapshot payload");

        let snapshot = job.to_snapshot();
        let encoded = rmp_serde::to_vec_named(&snapshot).expect("encode snapshot");
        let decoded: ResourceCompletionSnapshot =
            rmp_serde::from_slice(&encoded).expect("decode snapshot");
        let restored = ResourceCompletionJob::from_snapshot(decoded);

        assert_eq!(restored.to_snapshot(), snapshot);
    }

    #[test]
    fn resource_completion_snapshot_completes_to_worker_ready_outcome() {
        let link_id = AddressHash::new_from_slice(b"snapshot link");
        let job = ResourceCompletionJob::unencrypted_for_test(link_id, b"snapshot payload");

        let outcome = job
            .to_snapshot()
            .complete_with(|ciphertext| Ok(ciphertext.to_vec()))
            .expect("complete snapshot");

        assert_eq!(outcome.resource_hash, job.to_snapshot().resource_hash);
        assert_ne!(outcome.proof, [0u8; HASH_SIZE]);
        assert_eq!(outcome.data, b"snapshot payload");
        assert!(outcome.metadata.is_none());
        assert!(!outcome.is_request);
        assert!(!outcome.is_response);
    }
}

#[allow(clippy::large_enum_variant)]
enum PartOutcome {
    NoMatch,
    Incomplete,
    Failed,
    Complete(Packet, ResourcePayload),
}

impl ResourceReceiver {
    fn new(adv: &ResourceAdvertisement, link_id: AddressHash) -> Result<Self, RnsError> {
        let now = Instant::now();
        let max_parts = max_advertised_parts(adv.transfer_size)?;
        if adv.parts == 0 || u64::from(adv.parts) > max_parts {
            return Err(RnsError::InvalidArgument);
        }
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
            data_size: adv.data_size,
            encrypted: adv.encrypted(),
            compressed: adv.compressed(),
            split: (adv.flags & FLAG_SPLIT) == FLAG_SPLIT,
            has_metadata: (adv.flags & FLAG_METADATA) == FLAG_METADATA,
            request_id: adv.request_id.as_ref().map(|request_id| request_id.to_vec()),
            is_request: adv.is_request(),
            is_response: adv.is_response(),
            last_progress: now,
            last_request: now,
            retry_count: 0,
            status: ResourceStatus::Advertised,
        };
        receiver.apply_hashmap_segment(adv.segment_index.saturating_sub(1) as usize, &adv.hashmap);
        Ok(receiver)
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
        match self.accept_part(part) {
            PartAcceptOutcome::NoMatch => PartOutcome::NoMatch,
            PartAcceptOutcome::Incomplete => PartOutcome::Incomplete,
            PartAcceptOutcome::Failed => PartOutcome::Failed,
            PartAcceptOutcome::Complete(job) => {
                match complete_resource_job(job, |ciphertext| {
                    let mut out = vec![0u8; ciphertext.len() + 64];
                    link.decrypt(ciphertext, &mut out)
                        .map(|plaintext| plaintext.to_vec())
                        .map_err(|_| ())
                }) {
                    Ok((proof, payload)) => match build_resource_proof_packet(link, proof) {
                        Ok(packet) => {
                            self.status = ResourceStatus::Complete;
                            PartOutcome::Complete(packet, payload)
                        }
                        Err(_) => {
                            log::warn!("resource: failed to build proof packet");
                            self.status = ResourceStatus::Failed;
                            PartOutcome::Failed
                        }
                    },
                    Err(()) => {
                        self.status = ResourceStatus::Failed;
                        PartOutcome::Failed
                    }
                }
            }
        }
    }

    fn accept_part(&mut self, part: &[u8]) -> PartAcceptOutcome {
        if self.split {
            self.status = ResourceStatus::Failed;
            return PartAcceptOutcome::Failed;
        }

        let hash = map_hash(part, &self.random_hash);
        let Some(index) = self.hashmap.iter().position(|entry| entry.as_ref() == Some(&hash))
        else {
            return PartAcceptOutcome::NoMatch;
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
                    return PartAcceptOutcome::Incomplete;
                }
            }

            return PartAcceptOutcome::Complete(ResourceCompletionJob {
                resource_hash: self.resource_hash,
                link_id: self.link_id,
                random_hash: self.random_hash,
                encrypted: self.encrypted,
                compressed: self.compressed,
                has_metadata: self.has_metadata,
                data_size: self.data_size,
                request_id: self.request_id.clone(),
                is_request: self.is_request,
                is_response: self.is_response,
                stream,
            });
        }

        PartAcceptOutcome::Incomplete
    }

    fn is_active(&self) -> bool {
        !self.status.is_terminal()
    }

    fn mark_request(&mut self) {
        self.last_request = Instant::now();
        self.retry_count = self.retry_count.saturating_add(1);
    }

    fn retry_due(&self, now: Instant, retry_interval: Duration, max_retries: u8) -> bool {
        if self.status.is_terminal() {
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

enum PartAcceptOutcome {
    NoMatch,
    Incomplete,
    Failed,
    Complete(ResourceCompletionJob),
}

pub(crate) fn complete_resource_job<F>(
    job: ResourceCompletionJob,
    decrypt: F,
) -> Result<(ResourceProof, ResourcePayload), ()>
where
    F: FnOnce(&[u8]) -> Result<Vec<u8>, ()>,
{
    let plain = if job.encrypted {
        decrypt(&job.stream)?
    } else {
        job.stream
    };

    let mut payload = if plain.len() > RANDOM_HASH_SIZE {
        plain[RANDOM_HASH_SIZE..].to_vec()
    } else {
        Vec::new()
    };

    if job.compressed {
        let max_decompressed_size = max_decompressed_resource_size(job.data_size);
        let decompressed = decompress_resource_payload(payload.as_slice(), max_decompressed_size)?;
        if decompressed.len() > max_decompressed_size {
            return Err(());
        }
        payload = decompressed;
    }

    let (metadata, data_payload) = if job.has_metadata && payload.len() >= 3 {
        let size = ((payload[0] as usize) << 16) | ((payload[1] as usize) << 8) | payload[2] as usize;
        if size > METADATA_MAX_SIZE {
            return Err(());
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
    hasher.update(job.random_hash);
    let computed = Hash::new(copy_hash(&hasher.finalize()).map_err(|_| ())?);
    if computed != job.resource_hash {
        return Err(());
    }

    let mut proof_hasher = sha2::Sha256::new();
    proof_hasher.update(&payload);
    proof_hasher.update(job.resource_hash.as_slice());
    let proof = Hash::new(copy_hash(&proof_hasher.finalize()).map_err(|_| ())?);

    Ok((
        ResourceProof { resource_hash: job.resource_hash, proof },
        ResourcePayload {
            data: data_payload,
            metadata,
            request_id: job.request_id,
            is_request: job.is_request,
            is_response: job.is_response,
        },
    ))
}

pub(crate) fn build_resource_proof_packet(
    link: &(impl ResourcePacketLink + ?Sized),
    proof: ResourceProof,
) -> Result<Packet, RnsError> {
    build_link_packet_for(link, PacketType::Proof, PacketContext::ResourceProof, &proof.encode())
}

fn max_decompressed_resource_size(advertised_data_size: u64) -> usize {
    usize::try_from(advertised_data_size)
        .unwrap_or(AUTO_COMPRESS_MAX_SIZE)
        .min(AUTO_COMPRESS_MAX_SIZE)
}

fn max_advertised_parts(transfer_size: u64) -> Result<u64, RnsError> {
    if transfer_size == 0 || transfer_size > MAX_INBOUND_RESOURCE_TRANSFER_SIZE {
        return Err(RnsError::InvalidArgument);
    }
    let packet_mdu = PACKET_MDU as u64;
    Ok(transfer_size.div_ceil(packet_mdu).max(1))
}

fn decompress_resource_payload(payload: &[u8], max_size: usize) -> Result<Vec<u8>, ()> {
    let mut decoder = BzDecoder::new(payload);
    let mut decompressed = Vec::new();
    let limit = max_size.checked_add(1).ok_or(())?;
    let read = decoder
        .by_ref()
        .take(limit as u64)
        .read_to_end(&mut decompressed)
        .map_err(|_| ())?;
    if read > max_size || decompressed.len() > max_size {
        return Err(());
    }

    let mut trailing = [0u8; 1];
    match decoder.read(&mut trailing) {
        Ok(0) => Ok(decompressed),
        Ok(_) => Err(()),
        Err(_) => Err(()),
    }
}

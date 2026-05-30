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
struct ResourcePayload {
    data: Vec<u8>,
    metadata: Option<Vec<u8>>,
    request_id: Option<Vec<u8>>,
    is_request: bool,
    is_response: bool,
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
        if self.split {
            self.status = ResourceStatus::Failed;
            return PartOutcome::Failed;
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
                        return PartOutcome::Failed;
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
                let max_decompressed_size = max_decompressed_resource_size(self.data_size);
                let decompressed = match decompress_resource_payload(
                    payload.as_slice(),
                    max_decompressed_size,
                ) {
                    Ok(decompressed) => decompressed,
                    Err(()) => {
                        self.status = ResourceStatus::Failed;
                        return PartOutcome::Failed;
                    }
                };
                if decompressed.len() > max_decompressed_size {
                    self.status = ResourceStatus::Failed;
                    return PartOutcome::Failed;
                }
                payload = decompressed;
            }

            let (metadata, data_payload) = if self.has_metadata && payload.len() >= 3 {
                let size = ((payload[0] as usize) << 16)
                    | ((payload[1] as usize) << 8)
                    | payload[2] as usize;
                if size > METADATA_MAX_SIZE {
                    self.status = ResourceStatus::Failed;
                    return PartOutcome::Failed;
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
                    return PartOutcome::Failed;
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
                        return PartOutcome::Failed;
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
                        log::warn!("failed to build proof packet");
                        self.status = ResourceStatus::Failed;
                        return PartOutcome::Failed;
                    }
                };
                return PartOutcome::Complete(
                    packet,
                    ResourcePayload {
                        data: data_payload,
                        metadata,
                        request_id: self.request_id.clone(),
                        is_request: self.is_request,
                        is_response: self.is_response,
                    },
                );
            } else {
                self.status = ResourceStatus::Failed;
                return PartOutcome::Failed;
            }
        }

        PartOutcome::Incomplete
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

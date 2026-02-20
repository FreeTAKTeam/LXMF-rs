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

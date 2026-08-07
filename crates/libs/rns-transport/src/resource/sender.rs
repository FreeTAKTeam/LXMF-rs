const MAX_AWAITING_PROOF_RETRIES: u8 = 3;

#[derive(Debug, Clone)]
struct ResourceSender {
    link_id: AddressHash,
    resource_hash: Hash,
    original_hash: Hash,
    segment_index: u32,
    total_segments: u32,
    parts: Vec<Vec<u8>>,
    sent_parts: Vec<bool>,
    map_hashes: Vec<[u8; MAPHASH_LEN]>,
    hashmap_segment_len: usize,
    expected_proof: Hash,
    advertisement_packet: Packet,
    last_activity: Instant,
    adv_sent: Instant,
    last_part_sent: Instant,
    max_retries: u8,
    retries_left: u8,
    status: ResourceStatus,
    /// Lowest part index the peer may request. Reticulum advances this after
    /// serving a hashmap update and silently ignores older hashes.
    receiver_min_consecutive_height: usize,
}

enum OutboundResourcePoll {
    None,
    Send(Box<Packet>),
    Failed,
}

impl ResourceSender {
    #[cfg(test)]
    fn new(link: &Link, data: Vec<u8>, metadata: Option<Vec<u8>>) -> Result<Self, RnsError> {
        Self::new_with_mtu(link, data, metadata, DEFAULT_RESOURCE_INTERFACE_MTU)
    }

    #[cfg(test)]
    fn new_with_mtu(
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        interface_mtu: usize,
    ) -> Result<Self, RnsError> {
        Self::new_with_options_mtu(link, data, metadata, None, false, interface_mtu)
    }

    #[cfg(test)]
    pub(super) fn new_with_options(
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
    ) -> Result<Self, RnsError> {
        Self::new_with_options_mtu(
            link,
            data,
            metadata,
            request_id,
            is_response,
            DEFAULT_RESOURCE_INTERFACE_MTU,
        )
    }

    pub(super) fn new_with_options_mtu(
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
    ) -> Result<Self, RnsError> {
        Self::new_segment_with_options_mtu(
            link,
            data,
            metadata,
            request_id,
            is_response,
            interface_mtu,
            None,
            1,
            1,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn new_segment_with_options_mtu(
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
        original_hash: Option<Hash>,
        segment_index: u32,
        total_segments: u32,
        total_data_size: Option<u64>,
    ) -> Result<Self, RnsError> {
        let resource_mdu = resource_packet_mdu_for_mtu(interface_mtu)?;
        let hashmap_segment_len = resource_hashmap_segment_len_for_mtu(interface_mtu)?;
        let has_metadata = metadata.is_some();
        let has_request_id = request_id.is_some();
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
        let original_hash = original_hash.unwrap_or(resource_hash);

        let mut proof_hasher = sha2::Sha256::new();
        proof_hasher.update(&combined);
        proof_hasher.update(resource_hash.as_slice());
        let expected_proof = Hash::new(copy_hash(&proof_hasher.finalize())?);

        // Auto-compress before encrypting/chunking, matching real RNS's own
        // `Resource.__init__` exactly (`auto_compress=True` by default):
        // attempt bz2 only below `AUTO_COMPRESS_MAX_SIZE` (pointless CPU
        // otherwise, on something already too large to ever transfer as one
        // Resource), and only actually USE the compressed form if it's
        // genuinely smaller — a small/already-dense payload can grow under
        // bz2's own framing overhead. `resource_hash`/`expected_proof`/
        // `data_size` above are deliberately computed from `combined`
        // (the uncompressed logical content) either way, never from the
        // compressed bytes — this is a content identity, not a wire-format
        // one, exactly mirroring the reference (`self.hash = RNS.Identity.
        // full_hash(data+self.random_hash)`, computed from the pre-
        // compression `data`, confirmed by reading `RNS/Resource.py`
        // directly). The receive side already understands `FLAG_COMPRESSED`
        // and decompresses back to exactly this same `combined` before
        // parsing metadata/data — this was the one half of that round trip
        // never previously exercised on send.
        let compressed_candidate = if combined.len() as u64 <= AUTO_COMPRESS_MAX_SIZE as u64 {
            let mut encoder = BzEncoder::new(Vec::new(), Compression::best());
            match encoder.write_all(&combined).and_then(|_| encoder.finish()) {
                Ok(compressed) => {
                    // Not shrinking is ordinary — already-compressed payloads
                    // do it constantly — so that case stays quiet.
                    Some(compressed).filter(|compressed| compressed.len() < combined.len())
                }
                Err(error) => {
                    // A genuine encoder failure is not the same thing, and
                    // collapsing both into `None` hides it completely. Sending
                    // uncompressed is still the right fallback — the reference
                    // treats compression as opportunistic and a receiver reads
                    // the flag, not the intent — but this should never happen
                    // and is worth saying so.
                    log::warn!(
                        "resource outbound: bz2 compression failed, sending uncompressed ({error})"
                    );
                    None
                }
            }
        } else {
            None
        };
        let compressed = compressed_candidate.is_some();
        let transfer_payload = compressed_candidate.unwrap_or(combined);

        let mut prefix = random_bytes::<RANDOM_HASH_SIZE>().to_vec();
        prefix.extend_from_slice(&transfer_payload);

        let mut cipher_buf = vec![0u8; prefix.len() + 128];
        let cipher = link.encrypt(&prefix, &mut cipher_buf).map_err(|_| RnsError::CryptoError)?;
        let cipher_text = cipher.to_vec();

        let mut parts = Vec::new();
        for chunk in cipher_text.chunks(resource_mdu) {
            parts.push(chunk.to_vec());
        }

        let mut map_hashes = Vec::with_capacity(parts.len());
        for part in &parts {
            map_hashes.push(map_hash(part, &random_hash));
        }

        let advertisement = ResourceAdvertisement {
            transfer_size: parts.iter().map(|part| part.len() as u64).sum(),
            data_size: total_data_size.unwrap_or(data_size),
            parts: parts.len() as u32,
            hash: resource_hash,
            random_hash,
            original_hash,
            segment_index,
            total_segments,
            request_id: request_id.map(ByteBuf::from),
            flags: {
                let mut flags = FLAG_ENCRYPTED;
                if has_metadata {
                    flags |= FLAG_METADATA;
                }
                if has_request_id {
                    flags |= if is_response { FLAG_RESPONSE } else { FLAG_REQUEST };
                }
                if total_segments > 1 {
                    flags |= FLAG_SPLIT;
                }
                if compressed {
                    flags |= FLAG_COMPRESSED;
                }
                flags
            },
            hashmap: slice_hashmap_segment(&map_hashes, 0, hashmap_segment_len),
        };
        let advertisement_packet = build_link_packet(
            link,
            PacketType::Data,
            PacketContext::ResourceAdvrtisement,
            &advertisement.pack()?,
        )?;
        let now = Instant::now();

        Ok(Self {
            link_id: *link.id(),
            resource_hash,
            original_hash,
            segment_index,
            total_segments,
            parts,
            sent_parts: vec![false; map_hashes.len()],
            map_hashes,
            hashmap_segment_len,
            expected_proof,
            advertisement_packet,
            last_activity: now,
            adv_sent: now,
            last_part_sent: now,
            max_retries: 0,
            retries_left: 0,
            status: ResourceStatus::None,
            receiver_min_consecutive_height: 0,
        })
    }

    fn advertisement_packet(&self) -> Packet {
        self.advertisement_packet.clone()
    }

    fn mark_advertised(&mut self, retry_limit: u8) {
        let now = Instant::now();
        self.last_activity = now;
        self.adv_sent = now;
        self.last_part_sent = now;
        self.max_retries = retry_limit;
        self.retries_left = retry_limit.min(DEFAULT_RESOURCE_MAX_ADV_RETRIES);
        self.status = ResourceStatus::Advertised;
    }

    fn poll(&mut self, now: Instant, retry_interval: Duration) -> OutboundResourcePoll {
        match self.status {
            ResourceStatus::Advertised => {
                if now.duration_since(self.adv_sent) < retry_interval {
                    return OutboundResourcePoll::None;
                }
                if self.retries_left == 0 {
                    return OutboundResourcePoll::Failed;
                }
                self.retries_left -= 1;
                self.last_activity = now;
                self.adv_sent = now;
                OutboundResourcePoll::Send(Box::new(self.advertisement_packet()))
            }
            ResourceStatus::Transferring => {
                if now.duration_since(self.last_activity) < retry_interval {
                    return OutboundResourcePoll::None;
                }
                if self.retries_left == 0 {
                    return OutboundResourcePoll::Failed;
                }
                self.retries_left -= 1;
                self.last_activity = now;
                OutboundResourcePoll::None
            }
            ResourceStatus::AwaitingProof => {
                if now.duration_since(self.last_part_sent) < retry_interval {
                    return OutboundResourcePoll::None;
                }
                if self.retries_left == 0 {
                    return OutboundResourcePoll::Failed;
                }
                self.retries_left -= 1;
                self.last_part_sent = now;
                OutboundResourcePoll::None
            }
            ResourceStatus::Failed => OutboundResourcePoll::Failed,
            _ => OutboundResourcePoll::None,
        }
    }

    fn handle_request_into(
        &mut self,
        request: &ResourceRequest,
        link: &Link,
        packets: &mut Vec<Packet>,
    ) {
        if request.resource_hash != self.resource_hash {
            return;
        }

        let mut sent_any = false;
        let mut scratch_packet = Packet::default();
        let search_start = self.receiver_min_consecutive_height;
        let search_end = search_start
            .saturating_add(COLLISION_GUARD_SIZE)
            .min(self.map_hashes.len());
        for hash in &request.requested_hashes {
            if let Some(index) = self.map_hashes[search_start..search_end]
                .iter()
                .position(|entry| entry == hash)
                .map(|index| index + search_start)
            {
                if let Some(part) = self.parts.get(index) {
                    if build_link_packet_into(
                        link,
                        PacketType::Data,
                        PacketContext::Resource,
                        part,
                        &mut scratch_packet,
                    )
                    .is_ok()
                    {
                        self.sent_parts[index] = true;
                        sent_any = true;
                        packets.push(scratch_packet.clone());
                    } else {
                        self.status = ResourceStatus::Failed;
                        return;
                    }
                }
            } else {
                log::debug!(
                    "[resource-diag] request_part_miss hash={} requested_map_hash={:02x}{:02x}{:02x}{:02x}",
                    self.resource_hash, hash[0], hash[1], hash[2], hash[3]
                );
            }
        }
        log::debug!(
            "[resource-diag] request_parts_built hash={} requested={} built={} sent_any={}",
            self.resource_hash,
            request.requested_hashes.len(),
            packets.len(),
            sent_any
        );

        if request.hashmap_exhausted {
            if let Some(last_hash) = request.last_map_hash {
                let part_index = self.map_hashes[search_start..search_end]
                    .iter()
                    .position(|entry| *entry == last_hash)
                    .map(|index| search_start + index + 1)
                    .unwrap_or(search_end);
                self.receiver_min_consecutive_height =
                    part_index.saturating_sub(1 + WINDOW_MAX_FAST);
                if part_index % self.hashmap_segment_len != 0 {
                    log::error!(
                        "resource sequencing error hash={} part_index={} segment_len={}",
                        self.resource_hash,
                        part_index,
                        self.hashmap_segment_len
                    );
                    self.status = ResourceStatus::Failed;
                    return;
                }
                let next_segment = part_index / self.hashmap_segment_len;
                if next_segment * self.hashmap_segment_len < self.map_hashes.len() {
                        let update = ResourceHashUpdate {
                            resource_hash: self.resource_hash,
                            segment: next_segment as u32,
                            hashmap: slice_hashmap_segment(
                                &self.map_hashes,
                                next_segment,
                                self.hashmap_segment_len,
                            ),
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
                                self.status = ResourceStatus::Failed;
                            }
                        }
                }
            }
        }

        if self.status.accepts_transfer_activity() {
            let now = Instant::now();
            self.last_activity = now;
            self.retries_left = self.max_retries;
            if sent_any {
                self.last_part_sent = now;
            }
            if self.sent_parts.iter().all(|sent| *sent) {
                self.status = ResourceStatus::AwaitingProof;
                // Once all parts are sent, only wait a small, bounded number of
                // retry intervals for the terminal proof before timing out.
                self.retries_left = self.max_retries.clamp(1, MAX_AWAITING_PROOF_RETRIES);
            } else {
                self.status = ResourceStatus::Transferring;
            }
        }
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

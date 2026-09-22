#[derive(Debug, Clone)]
struct ResourceReceiver {
    resource_hash: Hash,
    original_hash: Hash,
    segment_index: u32,
    total_segments: u32,
    link_id: AddressHash,
    random_hash: [u8; RANDOM_HASH_SIZE],
    parts: Vec<Option<Vec<u8>>>,
    hashmap: Vec<Option<[u8; MAPHASH_LEN]>>,
    hashmap_segment_len: usize,
    received: usize,
    received_bytes: u64,
    total_bytes: u64,
    data_size: u64,
    encrypted: bool,
    compressed: bool,
    has_metadata: bool,
    request_id: Option<Vec<u8>>,
    is_request: bool,
    is_response: bool,
    last_progress: Instant,
    last_request: Instant,
    retry_count: u8,
    status: ResourceStatus,
    /// Indices of fragments not yet requested, in hashmap order.
    request_queue: VecDeque<usize>,
    /// Ordered by send time (front = oldest). Used to detect timed-out fragments in O(1).
    in_flight_queue: VecDeque<(Instant, usize)>,
    /// Maps fragment index → time it was last requested, for RTT measurement.
    in_flight_set: HashMap<usize, Instant>,
    /// RTT sample from the most recently matched received part; read once by the manager.
    last_rtt_sample: Option<Duration>,
    /// Number of fragments completed contiguously from index 0 — so this is
    /// also the index of the next fragment the transfer actually needs.
    /// Mirrors `Resource.consecutive_completed_height` in the reference
    /// implementation, which bases its request window on the same point
    /// (`RNS/Resource.py`, `request_next`).
    consecutive_completed_height: usize,
    /// Number of hashmap slots filled so far. The map fills contiguously
    /// from zero (segment zero arrives on the advertisement, later segments
    /// in order), so `hashmap[hashmap_height - 1]` is the last hash we know
    /// — which is exactly what the reference sends as `last_map_hash`
    /// (`RNS/Resource.py`, `hashmap_update`/`request_next`).
    hashmap_height: usize,
    /// Fragments requested per round, right now. Starts at [`WINDOW`] and
    /// moves between `window_min` and `window_max` as the link proves
    /// itself — see `note_round_complete`/`note_fragments_lost`.
    window: usize,
    window_min: usize,
    window_max: usize,
    /// Consecutive rounds measured above [`RATE_FAST`] / below
    /// [`RATE_VERY_SLOW`]. Latched: once the fast ceiling is unlocked the
    /// very-slow path is never taken, matching the reference's
    /// `fast_rate_rounds == 0` guard.
    fast_rate_rounds: u32,
    very_slow_rate_rounds: u32,
    /// When the current round's request went out, and how many bytes had
    /// arrived by then — the two inputs to the round's measured rate.
    round_started_at: Instant,
    round_start_bytes: u64,
    /// Set when a hashmap update has been asked for and not yet received.
    ///
    /// While set, no further request is built. Without this gate a receiver
    /// keeps re-requesting the map at link RTT, and each of those requests
    /// walks the reference *sender*'s serving window forward by a full
    /// hashmap segment (`receiver_min_consecutive_height`,
    /// `RNS/Resource.py`) while the fragment frontier advances by at most
    /// `WINDOW` — so within a few rounds every fragment the receiver asks
    /// for falls outside the window the sender will serve, and is dropped
    /// with no error on either side.
    waiting_for_hashmap_update: bool,
}

#[derive(Debug, Clone)]
struct ResourcePayload {
    data: Vec<u8>,
    metadata: Option<Vec<u8>>,
    request_id: Option<Vec<u8>>,
    is_request: bool,
    is_response: bool,
    segmentation: ResourceSegmentProgress,
}

#[allow(clippy::large_enum_variant)]
enum PartOutcome {
    NoMatch,
    Incomplete,
    Failed(&'static str),
    Complete(Packet, ResourcePayload),
}

impl ResourceReceiver {
    fn new(adv: &ResourceAdvertisement, link_id: AddressHash) -> Result<Self, RnsError> {
        Self::new_with_mtu(adv, link_id, DEFAULT_RESOURCE_INTERFACE_MTU)
    }

    fn new_with_mtu(
        adv: &ResourceAdvertisement,
        link_id: AddressHash,
        interface_mtu: usize,
    ) -> Result<Self, RnsError> {
        let now = Instant::now();
        let resource_mdu = resource_packet_mdu_for_mtu(interface_mtu)?;
        let local_hashmap_segment_len = resource_hashmap_segment_len_for_mtu(interface_mtu)?;
        let max_parts = max_advertised_parts(adv.transfer_size, resource_mdu)?;
        if adv.total_segments == 0
            || adv.segment_index == 0
            || adv.segment_index > adv.total_segments
            || ((adv.flags & FLAG_SPLIT) == FLAG_SPLIT) != (adv.total_segments > 1)
            || (adv.segment_index == 1 && adv.original_hash != adv.hash)
            || adv.data_size > MAX_INBOUND_RESOURCE_TRANSFER_SIZE
        {
            return Err(RnsError::InvalidArgument);
        }
        if adv.parts == 0 || u64::from(adv.parts) > max_parts {
            return Err(RnsError::InvalidArgument);
        }
        let total_parts = adv.parts as usize;
        let advertised_hashes = adv.hashmap.len() / MAPHASH_LEN;
        let hashmap_segment_len = if advertised_hashes > 0 && advertised_hashes < total_parts {
            advertised_hashes
        } else {
            local_hashmap_segment_len
        };
        let mut receiver = Self {
            resource_hash: adv.hash,
            original_hash: adv.original_hash,
            segment_index: adv.segment_index,
            total_segments: adv.total_segments,
            link_id,
            random_hash: adv.random_hash,
            parts: vec![None; total_parts],
            hashmap: vec![None; total_parts],
            hashmap_segment_len,
            received: 0,
            received_bytes: 0,
            total_bytes: adv.transfer_size,
            data_size: adv.data_size,
            encrypted: adv.encrypted(),
            compressed: adv.compressed(),
            has_metadata: (adv.flags & FLAG_METADATA) == FLAG_METADATA,
            request_id: adv.request_id.as_ref().map(|request_id| request_id.to_vec()),
            is_request: adv.is_request(),
            is_response: adv.is_response(),
            last_progress: now,
            last_request: now,
            retry_count: 0,
            status: ResourceStatus::Advertised,
            request_queue: VecDeque::new(),
            in_flight_queue: VecDeque::new(),
            in_flight_set: HashMap::new(),
            last_rtt_sample: None,
            consecutive_completed_height: 0,
            hashmap_height: 0,
            window: WINDOW,
            window_min: WINDOW_MIN,
            window_max: WINDOW_MAX_SLOW,
            fast_rate_rounds: 0,
            very_slow_rate_rounds: 0,
            round_started_at: now,
            round_start_bytes: 0,
            waiting_for_hashmap_update: false,
        };
        // Advertisement hashmaps always contain resource hashmap segment zero.
        // `segment_index` identifies a split resource segment, not a hashmap
        // update segment.
        receiver.apply_hashmap_segment(0, &adv.hashmap);
        Ok(receiver)
    }

    fn apply_hashmap_segment(&mut self, segment: usize, bytes: &[u8]) {
        let hashes = bytes.len() / MAPHASH_LEN;
        for i in 0..hashes {
            let start = i * MAPHASH_LEN;
            let mut entry = [0u8; MAPHASH_LEN];
            entry.copy_from_slice(&bytes[start..start + MAPHASH_LEN]);
            let idx = segment * self.hashmap_segment_len + i;
            if idx < self.hashmap.len() && self.hashmap[idx].is_none() {
                self.hashmap[idx] = Some(entry);
                self.hashmap_height += 1;
                self.request_queue.push_back(idx);
            }
        }
    }

    fn handle_hash_update(&mut self, update: &ResourceHashUpdate) {
        if update.resource_hash != self.resource_hash {
            return;
        }
        self.apply_hashmap_segment(update.segment as usize, &update.hashmap);
        self.waiting_for_hashmap_update = false;
    }

    fn handle_part(&mut self, part: &[u8], link: &Link) -> PartOutcome {
        let hash = map_hash(part, &self.random_hash);
        // Reticulum only accepts a fragment while it is inside the current
        // receive window. Searching the complete hashmap accepts delayed
        // fragments from an already completed round (or fragments from a
        // future round that arrived out of order), which advances
        // `consecutive_completed_height` without the sender ever having been
        // asked for that data. The reference searches
        // `hashmap[consecutive_completed_height..][..window]` in
        // `Resource.receive_part`; keep the same bounded admission here.
        let search_start = self.consecutive_completed_height.min(self.hashmap.len());
        let search_end = search_start.saturating_add(self.window).min(self.hashmap.len());
        let Some(relative_index) = self.hashmap[search_start..search_end]
            .iter()
            .position(|entry| entry.as_ref() == Some(&hash))
        else {
            return PartOutcome::NoMatch;
        };
        let index = search_start + relative_index;

        if self.parts[index].is_none() {
            self.parts[index] = Some(part.to_vec());
            self.received += 1;
            self.received_bytes = self.received_bytes.saturating_add(part.len() as u64);
            let now = Instant::now();
            self.last_progress = now;
            // Measure RTT: if this fragment was in-flight, record how long it took.
            if let Some(sent_at) = self.in_flight_set.remove(&index) {
                self.last_rtt_sample = Some(now.duration_since(sent_at));
            }
            // Amortised O(1) across the transfer: each fragment moves this
            // forward at most once.
            while self.consecutive_completed_height < self.parts.len()
                && self.parts[self.consecutive_completed_height].is_some()
            {
                self.consecutive_completed_height += 1;
            }
            self.note_fragment_received(now);
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
                        return self.fail("decrypt_failed");
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
                        return self.fail("decompress_failed");
                    }
                };
                if decompressed.len() > max_decompressed_size {
                    return self.fail("decompressed_size_exceeded");
                }
                payload = decompressed;
            }

            // Only the *first* segment of a split resource carries the metadata
            // block. A sender flags every segment as metadata-bearing so the
            // receiver can account for those bytes in `total_data_size`, but it
            // prefixes the 3-byte-length block to segment 1 alone. Stripping it
            // from later segments reads three bytes of payload as a length and
            // silently deletes that many bytes of real data.
            let carries_metadata_block = self.has_metadata && self.segment_index == 1;
            let (metadata, data_payload) = if carries_metadata_block && payload.len() >= 3 {
                let size = ((payload[0] as usize) << 16)
                    | ((payload[1] as usize) << 8)
                    | payload[2] as usize;
                if size > METADATA_MAX_SIZE {
                    return self.fail("metadata_size_exceeded");
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
                    return self.fail("resource_hash_copy_failed");
                }
            };

            if computed == self.resource_hash {
                let mut proof_hasher = sha2::Sha256::new();
                proof_hasher.update(&payload);
                proof_hasher.update(self.resource_hash.as_slice());
                let proof = match copy_hash(&proof_hasher.finalize()) {
                    Ok(hash) => Hash::new(hash),
                    Err(_) => {
                        return self.fail("proof_hash_copy_failed");
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
                        return self.fail("proof_packet_build_failed");
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
                        segmentation: ResourceSegmentProgress {
                            original_hash: self.original_hash,
                            segment_index: self.segment_index,
                            total_segments: self.total_segments,
                            total_data_size: self.data_size,
                        },
                    },
                );
            } else {
                return self.fail("resource_hash_mismatch");
            }
        }

        PartOutcome::Incomplete
    }

    fn fail(&mut self, reason: &'static str) -> PartOutcome {
        self.status = ResourceStatus::Failed;
        PartOutcome::Failed(reason)
    }

    fn is_active(&self) -> bool {
        !self.status.is_terminal()
    }

    fn mark_request(&mut self) {
        self.last_request = Instant::now();
        self.retry_count = self.retry_count.saturating_add(1);
    }

    /// Update the request timestamp without counting a retry.
    ///
    /// Use when sending a request as a direct reaction to an incoming part
    /// (transfer is actively progressing). Calling `mark_request` in that path
    /// causes the periodic `retry_requests` timer to see `retry_count >=
    /// retry_limit` and prematurely kill the receiver even though no timeout
    /// occurred.
    fn mark_active_request(&mut self) {
        self.last_request = Instant::now();
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

fn max_advertised_parts(transfer_size: u64, _resource_mdu: usize) -> Result<u64, RnsError> {
    if transfer_size == 0 || transfer_size > MAX_INBOUND_RESOURCE_TRANSFER_SIZE {
        return Err(RnsError::InvalidArgument);
    }
    // Python Reticulum derives its part count from the sender link SDU. A peer
    // with a smaller effective SDU can advertise more parts than Rust's local
    // resource MDU lower bound, so bound allocation without rejecting that case.
    Ok(transfer_size.min(MAX_INBOUND_RESOURCE_PARTS))
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

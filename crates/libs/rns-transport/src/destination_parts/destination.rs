impl Destination<PrivateIdentity, Input, Single> {
    pub fn new(identity: PrivateIdentity, name: DestinationName) -> Self {
        let address_hash = create_address_hash(&identity, &name);
        let pub_identity = *identity.as_identity();

        Self {
            direction: PhantomData,
            r#type: PhantomData,
            identity,
            desc: DestinationDesc { identity: pub_identity, name, address_hash },
            max_request_size: None,
            ratchet_state: RatchetState::default(),
            path_responses: BTreeMap::new(),
            path_response_queue: VecDeque::new(),
            proof_strategy: ProofStrategy::default(),
            proof_requested_callback: None,
        }
    }

    /// Sets whether this destination automatically proves a plain
    /// opportunistic `Data` packet it receives. Defaults to
    /// `ProofStrategy::None` (never prove), matching Python Reticulum's own
    /// default — must be set explicitly to `All` or `App` to enable
    /// delivery confirmation. See `ProofStrategy`'s own doc comment.
    pub fn set_proof_strategy(&mut self, strategy: ProofStrategy) {
        self.proof_strategy = strategy;
    }

    /// Registers the callback consulted when `proof_strategy ==
    /// ProofStrategy::App`. See `ProofRequestedHandler`'s doc comment for
    /// the reentrancy/blocking constraints this callback must honor.
    pub fn set_proof_requested_callback(&mut self, handler: Box<dyn ProofRequestedHandler>) {
        self.proof_requested_callback = Some(Arc::from(handler));
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

    /// Set the maximum packed request size accepted by this destination.
    pub fn set_max_request_size(&mut self, max_request_size: usize) -> Result<(), RnsError> {
        self.max_request_size = Some(max_request_size);
        Ok(())
    }

    /// Remove the inbound request-size limit.
    pub fn clear_max_request_size(&mut self) {
        self.max_request_size = None;
    }

    /// Return the configured packed request-size limit, if any.
    pub fn max_request_size(&self) -> Option<usize> {
        self.max_request_size
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

    fn prune_path_responses(&mut self, now: Instant) {
        while let Some((tag, expires_at)) = self.path_response_queue.front().cloned() {
            if expires_at > now && self.path_responses.len() <= PATH_RESPONSE_TAG_CAP {
                break;
            }
            self.path_response_queue.pop_front();
            self.path_responses.remove(&tag);
        }
    }

    pub fn path_response<R: CryptoRngCore + Copy>(
        &mut self,
        rng: R,
        app_data: Option<&[u8]>,
    ) -> Result<Packet, RnsError> {
        self.path_response_with_tag(rng, app_data, None)
    }

    pub fn path_response_with_tag<R: CryptoRngCore + Copy>(
        &mut self,
        rng: R,
        app_data: Option<&[u8]>,
        tag: Option<&[u8]>,
    ) -> Result<Packet, RnsError> {
        let now = Instant::now();
        self.prune_path_responses(now);

        if let Some(tag) = tag {
            if let Some((_, cached)) = self.path_responses.get(tag) {
                return Ok(cached.clone());
            }
        }

        let mut announce = self.announce(rng, app_data)?;
        announce.context = PacketContext::PathResponse;

        if let Some(tag) = tag {
            let expires_at = now + std::time::Duration::from_secs(PATH_RESPONSE_TAG_WINDOW);
            let tag = tag.to_vec();
            self.path_responses.insert(tag.clone(), (expires_at, announce.clone()));
            self.path_response_queue.push_back((tag, expires_at));
            self.prune_path_responses(now);
        }

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
            max_request_size: None,
            ratchet_state: RatchetState::default(),
            path_responses: BTreeMap::new(),
            path_response_queue: VecDeque::new(),
            proof_strategy: ProofStrategy::default(),
            proof_requested_callback: None,
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
            max_request_size: None,
            ratchet_state: RatchetState::default(),
            path_responses: BTreeMap::new(),
            path_response_queue: VecDeque::new(),
            proof_strategy: ProofStrategy::default(),
            proof_requested_callback: None,
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

impl ResourceManager {
    pub fn new() -> Self {
        Self::new_with_config(
            Duration::from_secs(DEFAULT_RESOURCE_RETRY_INTERVAL_SECS),
            DEFAULT_RESOURCE_MAX_RETRIES,
        )
    }

    pub fn new_with_config(retry_interval: Duration, retry_limit: u8) -> Self {
        Self {
            pending_outgoing: HashMap::new(),
            outgoing: HashMap::new(),
            outgoing_segment_chains: HashMap::new(),
            incoming: HashMap::new(),
            incoming_segments: HashMap::new(),
            events: Vec::new(),
            retry_interval,
            retry_limit,
            link_stats: HashMap::new(),
        }
    }

    pub fn start_send(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<(Hash, Packet), RnsError> {
        self.start_segmented_send(
            link,
            data,
            metadata,
            None,
            false,
            DEFAULT_RESOURCE_INTERFACE_MTU,
        )
    }

    pub fn start_send_with_mtu(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        interface_mtu: usize,
    ) -> Result<(Hash, Packet), RnsError> {
        self.start_segmented_send(link, data, metadata, None, false, interface_mtu)
    }

    pub fn start_send_with_options(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
    ) -> Result<(Hash, Packet), RnsError> {
        self.start_segmented_send(
            link,
            data,
            metadata,
            request_id,
            is_response,
            DEFAULT_RESOURCE_INTERFACE_MTU,
        )
    }

    pub fn start_send_with_options_mtu(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
    ) -> Result<(Hash, Packet), RnsError> {
        self.start_segmented_send(
            link,
            data,
            metadata,
            request_id,
            is_response,
            interface_mtu,
        )
    }

    fn start_segmented_send(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
    ) -> Result<(Hash, Packet), RnsError> {
        let prepared =
            Self::prepare_send(link, data, metadata, request_id, is_response, interface_mtu)?;
        Ok(self.track_prepared(prepared))
    }

    /// Everything a send needs done *before* the manager is touched.
    ///
    /// Deliberately an associated function: it takes no `&self` and no
    /// `&mut self`, which is what lets a caller run it without holding the
    /// transport handler lock. All of the expensive work lives here —
    /// compression, encryption, chunking, hashing — while [`track_prepared`]
    /// is a pair of map inserts.
    ///
    /// [`track_prepared`]: ResourceManager::track_prepared
    pub fn prepare_send(
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
    ) -> Result<PreparedSend, RnsError> {
        Self::prepare_send_with_compression(
            link,
            data,
            metadata,
            request_id,
            is_response,
            interface_mtu,
            true,
        )
    }

    pub fn prepare_send_with_compression(
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
        auto_compress: bool,
    ) -> Result<PreparedSend, RnsError> {
        let metadata_size = metadata
            .as_ref()
            .map(|value| value.len().saturating_add(3))
            .unwrap_or(0);
        let total_size = metadata_size.checked_add(data.len()).ok_or(RnsError::InvalidArgument)?;
        if total_size <= MAX_EFFICIENT_SIZE {
            let sender = ResourceSender::new_with_options_mtu_and_compression(
                link,
                data,
                metadata,
                request_id,
                is_response,
                interface_mtu,
                auto_compress,
            )?;
            return Ok(PreparedSend { first: sender, pending: None });
        }
        if metadata_size >= MAX_EFFICIENT_SIZE || total_size > AUTO_COMPRESS_MAX_SIZE {
            return Err(RnsError::InvalidArgument);
        }

        let total_segments = total_size.div_ceil(MAX_EFFICIENT_SIZE) as u32;
        let first_data_len = (MAX_EFFICIENT_SIZE - metadata_size).min(data.len());
        let first = ResourceSender::new_segment_with_options_mtu_and_compression(
            link,
            data[..first_data_len].to_vec(),
            metadata,
            request_id.clone(),
            is_response,
            interface_mtu,
            None,
            1,
            total_segments,
            Some(total_size as u64),
            auto_compress,
        )?;
        let original_hash = first.original_hash;
        // Only segment 1 is built here. The rest are built as each preceding
        // segment's proof arrives — see `PendingSegments`.
        Ok(PreparedSend {
            pending: Some(PendingSegments {
                link_id: first.link_id,
                data,
                offset: first_data_len,
                next_segment_index: 2,
                total_segments,
                total_size: total_size as u64,
                request_id,
                is_response,
                interface_mtu,
                original_hash,
                auto_compress,
            }),
            first,
        })
    }

    /// Takes ownership of a [`prepare_send`] result and returns the
    /// advertisement to dispatch.
    ///
    /// Cheap by construction — two map inserts and a clone of an
    /// already-built packet — so it is safe to call with the handler lock
    /// held.
    ///
    /// [`prepare_send`]: ResourceManager::prepare_send
    pub fn track_prepared(&mut self, prepared: PreparedSend) -> (Hash, Packet) {
        let PreparedSend { first, pending } = prepared;
        if let Some(pending) = pending {
            self.outgoing_segment_chains.insert(pending.original_hash, pending);
        }
        let resource_hash = first.resource_hash;
        let packet = first.advertisement_packet();
        self.pending_outgoing.insert(resource_hash, first);
        (resource_hash, packet)
    }
}

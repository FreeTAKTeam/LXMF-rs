#[derive(Debug, Clone, PartialEq, Eq)]
enum QueuedTransmission {
    Chunk { index: u8, position: u8 },
    Request(Vec<u8>),
}

#[derive(Debug)]
pub struct MeshtasticTunnel {
    config: MeshtasticInterfaceConfig,
    outgoing_packet_storage: HashMap<u8, (MeshtasticDestination, MeshtasticPacketHandler)>,
    packet_queue: VecDeque<QueuedTransmission>,
    assembly: HashMap<u32, HashMap<u8, MeshtasticPacketHandler>>,
    expected_index: HashMap<u32, VecDeque<(u8, u8)>>,
    requested_index: HashMap<u32, VecDeque<(u8, u8)>>,
    dest_to_node: HashMap<[u8; ADDRESS_HASH_SIZE], u32>,
    dest_order: VecDeque<[u8; ADDRESS_HASH_SIZE]>,
    packet_index: u8,
    status: MeshtasticTunnelStatus,
}

impl MeshtasticTunnel {
    #[must_use]
    pub fn new(config: MeshtasticInterfaceConfig) -> Self {
        Self {
            config,
            outgoing_packet_storage: HashMap::new(),
            packet_queue: VecDeque::new(),
            assembly: HashMap::new(),
            expected_index: HashMap::new(),
            requested_index: HashMap::new(),
            dest_to_node: HashMap::new(),
            dest_order: VecDeque::new(),
            packet_index: 0,
            status: MeshtasticTunnelStatus::default(),
        }
    }

    pub fn queue_outgoing_packet(&mut self, data: &[u8]) -> Result<(), String> {
        let packet_index = self
            .next_available_packet_index()
            .ok_or_else(|| "meshtastic outgoing packet index space is full".to_string())?;
        let destination = self.destination_for_packet(data);
        let handler = MeshtasticPacketHandler::new_outgoing(
            data,
            packet_index,
            self.config.max_payload_bytes,
        )?;
        for position in handler.positions() {
            self.packet_queue.push_back(QueuedTransmission::Chunk {
                index: packet_index,
                position: position.unsigned_abs(),
            });
        }
        self.outgoing_packet_storage.insert(packet_index, (destination, handler));
        self.packet_index = calc_meshtastic_index(packet_index);
        self.refresh_status();
        Ok(())
    }

    pub fn process_received(
        &mut self,
        frame: MeshtasticReceivedFrame,
    ) -> Result<Option<Vec<u8>>, String> {
        self.status.chunks_rx = self.status.chunks_rx.saturating_add(1);
        if frame.payload.starts_with(REQUEST_PREFIX) {
            self.queue_retransmit_request(&frame.payload[REQUEST_PREFIX.len()..])?;
            self.refresh_status();
            return Ok(None);
        }

        let (new_index, position) = MeshtasticPacketHandler::metadata(&frame.payload)?;
        let abs_position = position.unsigned_abs();
        let expected_key = (new_index, abs_position);
        let mut missing_request = None;

        {
            let expected = self.expected_index.entry(frame.from).or_default();
            let requested = self.requested_index.entry(frame.from).or_default();
            let was_expected = expected.iter().any(|entry| *entry == expected_key);
            let requested_offset = requested.iter().position(|entry| *entry == expected_key);
            if was_expected {
                expected.retain(|entry| *entry != expected_key);
            }
            if let Some(offset) = requested_offset {
                requested.remove(offset);
            } else if !was_expected {
                missing_request = expected.pop_front();
            }
        }
        if let Some((missing_index, missing_position)) = missing_request {
            self.request_missing_chunk(frame.from, missing_index, missing_position);
        }

        let (complete, first_missing_position) = {
            let by_index = self.assembly.entry(frame.from).or_default();
            let handler =
                by_index.entry(new_index).or_insert_with(MeshtasticPacketHandler::new_inbound);
            let complete = handler.process_payload(&frame.payload)?;
            let first_missing_position = handler.first_missing_position();
            (complete, first_missing_position)
        };
        if position < 0 {
            self.expected_index
                .entry(frame.from)
                .or_default()
                .push_front((calc_meshtastic_index(new_index), 1));
        } else {
            self.expected_index
                .entry(frame.from)
                .or_default()
                .push_front((new_index, abs_position.saturating_add(1)));
        }
        if let Some(missing_position) = first_missing_position {
            self.request_missing_chunk(frame.from, new_index, missing_position);
        }

        if let Some(data) = complete {
            self.learn_destination(&data, frame.from);
            if let Some(by_index) = self.assembly.get_mut(&frame.from) {
                by_index.remove(&new_index);
            }
            self.status.packets_rx = self.status.packets_rx.saturating_add(1);
            self.refresh_status();
            return Ok(Some(data));
        }

        self.refresh_status();
        Ok(None)
    }

    pub fn next_transmit(&mut self) -> Option<MeshtasticTransmitFrame> {
        while let Some(next) = self.packet_queue.pop_front() {
            match next {
                QueuedTransmission::Request(payload) => {
                    self.status.chunks_tx = self.status.chunks_tx.saturating_add(1);
                    self.refresh_status();
                    return Some(self.transmit_frame(MeshtasticDestination::Broadcast, payload));
                }
                QueuedTransmission::Chunk { index, position } => {
                    let Some((destination, payload, final_chunk)) =
                        self.outgoing_packet_storage.get(&index).and_then(
                            |(destination, handler)| {
                                let payload = handler.payload_at(position)?.to_vec();
                                let final_chunk =
                                    handler.positions().last().map(|last| last.unsigned_abs())
                                        == Some(position);
                                Some((*destination, payload, final_chunk))
                            },
                        )
                    else {
                        continue;
                    };
                    self.status.chunks_tx = self.status.chunks_tx.saturating_add(1);
                    if final_chunk {
                        self.status.packets_tx = self.status.packets_tx.saturating_add(1);
                    }
                    self.refresh_status();
                    return Some(self.transmit_frame(destination, payload));
                }
            }
        }
        self.refresh_status();
        None
    }

    #[must_use]
    pub fn pending_transmit_len(&self) -> usize {
        self.packet_queue.len()
    }

    #[must_use]
    pub fn status(&self) -> MeshtasticTunnelStatus {
        self.status.clone()
    }

    fn queue_retransmit_request(&mut self, metadata: &[u8]) -> Result<(), String> {
        let (index, position) = MeshtasticPacketHandler::metadata(metadata)?;
        self.packet_queue
            .push_front(QueuedTransmission::Chunk { index, position: position.unsigned_abs() });
        Ok(())
    }

    fn request_missing_chunk(&mut self, from: u32, index: u8, position: u8) {
        let requested = self.requested_index.entry(from).or_default();
        if requested.iter().any(|entry| *entry == (index, position)) {
            return;
        }
        requested.push_back((index, position));
        while requested.len() > MAX_REQUESTED_CHUNKS_PER_NODE {
            requested.pop_front();
        }
        self.packet_queue
            .push_front(QueuedTransmission::Request(request_payload(index, position as i8)));
        self.status.requested_retransmits = self.status.requested_retransmits.saturating_add(1);
    }

    fn next_available_packet_index(&self) -> Option<u8> {
        (0..=usize::from(u8::MAX))
            .map(|offset| self.packet_index.wrapping_add(offset as u8))
            .find(|index| {
                !self.outgoing_packet_storage.contains_key(index)
                    || !self.packet_queue.iter().any(|queued| match queued {
                        QueuedTransmission::Chunk { index: queued_index, .. } => {
                            queued_index == index
                        }
                        QueuedTransmission::Request(_) => false,
                    })
            })
    }

    fn transmit_frame(
        &self,
        destination: MeshtasticDestination,
        payload: Vec<u8>,
    ) -> MeshtasticTransmitFrame {
        MeshtasticTransmitFrame {
            destination,
            payload,
            hop_limit: self.config.hop_limit,
            want_ack: false,
            want_response: false,
            channel_index: 0,
        }
    }

    fn destination_for_packet(&self, data: &[u8]) -> MeshtasticDestination {
        packet_destination_field(data)
            .and_then(|destination| self.dest_to_node.get(&destination).copied())
            .map(MeshtasticDestination::Node)
            .unwrap_or(MeshtasticDestination::Broadcast)
    }

    fn learn_destination(&mut self, data: &[u8], from: u32) {
        let Some(destination) = packet_destination(data) else {
            return;
        };
        if !self.dest_to_node.contains_key(&destination) {
            self.dest_order.push_back(destination);
        }
        self.dest_to_node.insert(destination, from);
        while self.dest_order.len() > self.config.destination_cache_size {
            if let Some(stale) = self.dest_order.pop_front() {
                self.dest_to_node.remove(&stale);
            }
        }
    }

    fn refresh_status(&mut self) {
        self.status.queued_transmissions = self.packet_queue.len();
        self.status.destination_routes = self.dest_to_node.len();
    }
}

fn request_payload(index: u8, position: i8) -> Vec<u8> {
    let mut payload = Vec::with_capacity(REQUEST_PREFIX.len() + 2);
    payload.extend_from_slice(REQUEST_PREFIX);
    payload.push(index);
    payload.push(position as u8);
    payload
}

fn packet_destination(data: &[u8]) -> Option<[u8; ADDRESS_HASH_SIZE]> {
    if data.is_empty() {
        return None;
    }
    if data[0] & 0b1100_1100 != 0b0000_1100 {
        return None;
    }
    packet_destination_field(data)
}

fn packet_destination_field(data: &[u8]) -> Option<[u8; ADDRESS_HASH_SIZE]> {
    if data.len() < 2 + ADDRESS_HASH_SIZE {
        return None;
    }
    let mut destination = [0_u8; ADDRESS_HASH_SIZE];
    destination.copy_from_slice(&data[2..2 + ADDRESS_HASH_SIZE]);
    Some(destination)
}

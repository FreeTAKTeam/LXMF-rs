#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshtasticPacketHandler {
    index: Option<u8>,
    chunks: BTreeMap<u8, Vec<u8>>,
    positions: BTreeMap<u8, i8>,
}

impl MeshtasticPacketHandler {
    #[must_use]
    pub fn new_inbound() -> Self {
        Self { index: None, chunks: BTreeMap::new(), positions: BTreeMap::new() }
    }

    pub fn new_outgoing(data: &[u8], index: u8, max_payload: usize) -> Result<Self, String> {
        if max_payload == 0 {
            return Err("meshtastic max_payload_bytes must be > 0".to_string());
        }

        let mut handler = Self::new_inbound();
        handler.index = Some(index);
        if data.is_empty() {
            handler.insert_payload(index, -1, &[])?;
            return Ok(handler);
        }

        let packet_count = data.len() / max_payload + 1;
        if packet_count > i8::MAX as usize {
            return Err("meshtastic packet requires more than 127 chunks".to_string());
        }
        let packet_size = data.len() / packet_count + 1;
        for (offset, chunk) in data.chunks(packet_size).enumerate() {
            let abs_position = u8::try_from(offset + 1)
                .map_err(|_| "meshtastic chunk position must fit in u8".to_string())?;
            let signed = i8::try_from(abs_position)
                .map_err(|_| "meshtastic chunk position must fit in i8".to_string())?;
            let signed = if offset + 1 == packet_count { -signed } else { signed };
            handler.insert_payload(index, signed, chunk)?;
        }
        Ok(handler)
    }

    pub fn metadata(packet: &[u8]) -> Result<(u8, i8), String> {
        if packet.len() < 2 {
            return Err("meshtastic packet chunk is missing metadata".to_string());
        }
        Ok((packet[0], packet[1] as i8))
    }

    #[must_use]
    pub fn positions(&self) -> Vec<i8> {
        self.positions.values().copied().collect()
    }

    #[must_use]
    pub fn payload_at(&self, abs_position: u8) -> Option<&[u8]> {
        self.chunks.get(&abs_position).map(Vec::as_slice)
    }

    #[must_use]
    pub fn first_missing_position(&self) -> Option<u8> {
        let final_position = self.positions.values().find(|position| **position < 0)?.unsigned_abs();
        (1..final_position).find(|position| !self.chunks.contains_key(position))
    }

    pub fn process_payload(&mut self, packet: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let (index, position) = Self::metadata(packet)?;
        self.index = Some(index);
        self.chunks.insert(position.unsigned_abs(), packet.to_vec());
        self.positions.insert(position.unsigned_abs(), position);
        if position < 0 || self.positions.values().any(|stored| *stored < 0) {
            return Ok(self.assemble_data());
        }
        Ok(None)
    }

    fn insert_payload(&mut self, index: u8, position: i8, data: &[u8]) -> Result<(), String> {
        if position == 0 {
            return Err("meshtastic chunk position must not be zero".to_string());
        }
        let mut payload = Vec::with_capacity(2 + data.len());
        payload.push(index);
        payload.push(position as u8);
        payload.extend_from_slice(data);
        self.chunks.insert(position.unsigned_abs(), payload);
        self.positions.insert(position.unsigned_abs(), position);
        Ok(())
    }

    fn assemble_data(&self) -> Option<Vec<u8>> {
        let mut expected = 1_u8;
        for key in self.chunks.keys().copied() {
            if key != expected {
                return None;
            }
            expected = expected.saturating_add(1);
        }

        let mut data = Vec::new();
        for payload in self.chunks.values() {
            data.extend_from_slice(payload.get(2..)?);
        }
        Some(data)
    }
}

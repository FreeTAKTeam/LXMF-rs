// Kept separate from `new.rs` so that module stays below the repository's
// 500-line limit — the same reason `identify_packet_tests.rs` is its own
// file. `include!`d into the same module, so this is a file boundary and
// not a privacy one.

impl Link {
    /// Build a request packet (context = `Request`) carrying `data`.
    ///
    /// The mirror of [`Self::response_packet`], and missing for the same
    /// reason. Real Reticulum makes the same size-based choice on the way
    /// out (`RNS/Link.py`, `request`):
    ///
    /// ```text
    /// packed_request = umsgpack.packb([time, path_hash, data])
    /// if len(packed_request) <= self.mdu:
    ///     RNS.Packet(self, packed_request, RNS.Packet.DATA,
    ///                context = RNS.Packet.REQUEST).send()
    /// else:
    ///     request_resource = RNS.Resource(packed_request, self, ...)
    /// ```
    ///
    /// **The two branches identify the request differently, and a caller
    /// has to know it.** Sent as a resource, the request id is the
    /// requester's own `truncated_hash(packed_request)`. Sent as a packet,
    /// there is no such field on the wire — the responder derives the id
    /// from the packet's hash (`handle_packet`, below, takes the first
    /// [`ADDRESS_HASH_SIZE`] bytes of it), so a requester correlating the
    /// response must use `packet.hash()` from the packet this returns
    /// rather than an id of its own choosing.
    pub fn request_packet(&self, data: &[u8]) -> Result<Packet, RnsError> {
        self.packet_with_context(data, PacketContext::Request)
    }

    /// Build a request packet and associate the optional Python-compatible
    /// `max_response_size` with the request id derived from its packet hash.
    pub fn request_packet_with_max_response_size(
        &mut self,
        data: &[u8],
        max_response_size: Option<usize>,
    ) -> Result<Packet, RnsError> {
        let packet = self.packet_with_context(data, PacketContext::Request)?;
        if let Some(max_response_size) = max_response_size {
            let request_id = packet.hash().to_bytes()[..ADDRESS_HASH_SIZE].to_vec();
            self.pending_response_limits.insert(request_id, max_response_size);
        }
        Ok(packet)
    }

    /// Register a response-size limit for a resource-backed request.
    pub fn set_response_size_limit(&mut self, request_id: &[u8], max_response_size: usize) {
        self.pending_response_limits.insert(request_id.to_vec(), max_response_size);
    }

    /// Remove a pending limit after the matching response is accepted or
    /// rejected. A missing limit means the request was unbounded.
    pub fn take_response_limit_if_allowed(&mut self, request_id: &[u8], response_size: usize) -> bool {
        self.pending_response_limits
            .remove(request_id)
            .is_none_or(|limit| response_size <= limit)
    }

    pub fn clear_response_size_limit(&mut self, request_id: &[u8]) {
        self.pending_response_limits.remove(request_id);
    }

    /// Build a response packet (context = `Response`) carrying `data`.
    ///
    /// Real Reticulum answers a request with a single packet whenever the
    /// packed response fits the link MDU, and only falls back to a resource
    /// transfer when it does not (`RNS/Link.py`, `handle_request`):
    ///
    /// ```text
    /// packed_response = umsgpack.packb([request_id, response])
    /// if len(packed_response) <= self.mdu:
    ///     RNS.Packet(self, packed_response, RNS.Packet.DATA,
    ///                context = RNS.Packet.RESPONSE).send()
    /// else:
    ///     response_resource = RNS.Resource(packed_response, self, ...)
    /// ```
    ///
    /// `data` is that already-packed `[request_id, response]` envelope —
    /// the same bytes either branch carries, so a responder only chooses
    /// the mechanism, never the payload.
    ///
    /// The receive half has always been here: `handle_packet` decrypts
    /// `PacketContext::Response` and posts it as a `LinkEvent::Data`. Only
    /// the constructor was missing, so a responder built on this crate had
    /// to send every reply as a resource — several round trips and an
    /// advertisement for a payload that fits in one packet, which on a slow
    /// link costs far more than the bytes do.
    pub fn response_packet(&self, data: &[u8]) -> Result<Packet, RnsError> {
        self.packet_with_context(data, PacketContext::Response)
    }
}

fn response_envelope_metadata(data: &[u8]) -> Option<(Option<[u8; ADDRESS_HASH_SIZE]>, usize)> {
    let mut cursor = std::io::Cursor::new(data);
    let value = rmpv::decode::read_value(&mut cursor).ok()?;
    if cursor.position() != data.len() as u64 {
        return None;
    }
    let values = value.as_array()?;
    let request_id = values.first()?.as_slice()?;
    let request_id = request_id.try_into().ok();
    let response = values.get(1)?;
    let mut packed_response = Vec::new();
    rmpv::encode::write_value(&mut packed_response, response).ok()?;
    Some((request_id, packed_response.len()))
}

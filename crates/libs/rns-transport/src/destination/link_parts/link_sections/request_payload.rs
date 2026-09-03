// Kept separate from `new.rs` and `response_packet.rs` so each stays below
// the repository's 500-line limit. `include!`d into the same module, so this
// is a file boundary and not a privacy one.

/// A packed request and what a client needs to correlate its response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRequest {
    /// `[time, truncated_hash(path), data]`, msgpack-packed: the bytes a
    /// [`Link::request_packet`] or a request resource carries.
    pub packed: Vec<u8>,
    /// `truncated_hash(path)`, the second element of `packed`.
    pub path_hash: [u8; ADDRESS_HASH_SIZE],
    /// The request id when `packed` travels as a resource —
    /// `truncated_hash(packed)`, which `RNS.Link.request` records for that
    /// branch. A request sent as a single packet is identified by the
    /// packet's own hash instead; see [`Link::request_packet`].
    pub resource_request_id: [u8; ADDRESS_HASH_SIZE],
}

impl Link {
    /// The wire form of a request for `path` carrying `data`, as
    /// `RNS.Link.request` packs it:
    ///
    /// ```text
    /// packed_request = umsgpack.packb([time.time(), request_path_hash, data])
    /// ```
    ///
    /// `data` is packed as its own msgpack type — a map, an array, nil for
    /// no body — never as a byte string: `Link.handle_request` hands
    /// `unpacked_request[2]` to the service handler as-is, so the type on
    /// the wire is the contract. Fails only if `data` cannot be packed.
    pub fn request_payload(path: &str, data: rmpv::Value) -> Result<LinkRequest, RnsError> {
        let path_hash = crate::hash::address_hash(path.as_bytes());
        let envelope = rmpv::Value::Array(vec![
            rmpv::Value::F64(crate::ratchets::now_secs()),
            rmpv::Value::Binary(path_hash.to_vec()),
            data,
        ]);
        let mut packed = Vec::new();
        rmpv::encode::write_value(&mut packed, &envelope).map_err(|_| RnsError::InvalidArgument)?;
        let resource_request_id = crate::hash::address_hash(&packed);
        Ok(LinkRequest { packed, path_hash, resource_request_id })
    }

    /// The payload `RNS.Link.identify` sends over this link for `identity`:
    /// `public_key ++ verifying_key ++ sign(link_id ++ public_key ++
    /// verifying_key)`, the bytes the receiving end verifies before posting
    /// `LinkEvent::PeerIdentified`. Wrap it with [`Self::identify_packet`].
    pub fn identify_payload(&self, identity: &PrivateIdentity) -> Vec<u8> {
        identify_payload(identity, &self.id)
    }
}

/// [`Link::identify_payload`] for a link known only by its id.
pub fn identify_payload(identity: &PrivateIdentity, link_id: &AddressHash) -> Vec<u8> {
    let public = identity.as_identity();
    let mut signed = Vec::with_capacity(ADDRESS_HASH_SIZE + PUBLIC_KEY_LENGTH * 2);
    signed.extend_from_slice(link_id.as_slice());
    signed.extend_from_slice(public.public_key_bytes());
    signed.extend_from_slice(public.verifying_key_bytes());

    let mut payload = Vec::with_capacity(LINK_IDENTIFY_PAYLOAD_LENGTH);
    payload.extend_from_slice(public.public_key_bytes());
    payload.extend_from_slice(public.verifying_key_bytes());
    payload.extend_from_slice(&identity.sign(&signed).to_bytes());
    payload
}

/// The `[request_id, response]` envelope a response carries —
/// `RNS.Link.handle_request` packs one for a single packet and a resource
/// alike. Returns the request id it answers and the response value.
pub fn unpack_response_envelope(
    data: &[u8],
) -> Result<([u8; ADDRESS_HASH_SIZE], rmpv::Value), RnsError> {
    let mut cursor = std::io::Cursor::new(data);
    let value = rmpv::decode::read_value(&mut cursor).map_err(|_| RnsError::PacketError)?;
    if cursor.position() != data.len() as u64 {
        return Err(RnsError::PacketError);
    }
    let rmpv::Value::Array(values) = value else {
        return Err(RnsError::PacketError);
    };
    let [request_id, response] =
        <[rmpv::Value; 2]>::try_from(values).map_err(|_| RnsError::PacketError)?;
    let request_id = request_id
        .as_slice()
        .and_then(|bytes| <[u8; ADDRESS_HASH_SIZE]>::try_from(bytes).ok())
        .ok_or(RnsError::PacketError)?;
    Ok((request_id, response))
}

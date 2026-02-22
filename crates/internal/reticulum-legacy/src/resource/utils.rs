fn build_link_packet(
    link: &Link,
    packet_type: PacketType,
    context: PacketContext,
    payload: &[u8],
) -> Result<Packet, RnsError> {
    let mut packet_data = PacketDataBuffer::new();
    let should_encrypt = context != PacketContext::Resource
        && !(packet_type == PacketType::Proof && context == PacketContext::ResourceProof);
    if should_encrypt {
        let cipher_text_len = {
            let cipher_text = link.encrypt(payload, packet_data.accuire_buf_max())?;
            cipher_text.len()
        };
        packet_data.resize(cipher_text_len);
    } else {
        packet_data.write(payload)?;
    }
    Ok(Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type,
            ..Default::default()
        },
        ifac: None,
        destination: *link.id(),
        transport: None,
        context,
        data: packet_data,
    })
}

pub(crate) fn build_resource_request_packet(link: &Link, request: &ResourceRequest) -> Packet {
    build_link_packet(link, PacketType::Data, PacketContext::ResourceRequest, &request.encode())
        .expect("resource request packet")
}

fn slice_hashmap_segment(hashes: &[[u8; MAPHASH_LEN]], segment: usize) -> Vec<u8> {
    let start = segment * HASHMAP_MAX_LEN;
    let end = usize::min((segment + 1) * HASHMAP_MAX_LEN, hashes.len());
    let mut out = Vec::with_capacity((end - start) * MAPHASH_LEN);
    for hash in &hashes[start..end] {
        out.extend_from_slice(hash);
    }
    out
}

fn map_hash(part: &[u8], random_hash: &[u8; RANDOM_HASH_SIZE]) -> [u8; MAPHASH_LEN] {
    let mut hasher = sha2::Sha256::new();
    hasher.update(part);
    hasher.update(random_hash);
    let digest = hasher.finalize();
    let mut out = [0u8; MAPHASH_LEN];
    out.copy_from_slice(&digest[..MAPHASH_LEN]);
    out
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    OsRng.fill_bytes(&mut out);
    out
}

fn copy_hash(bytes: &[u8]) -> Result<[u8; HASH_SIZE], RnsError> {
    copy_fixed::<HASH_SIZE>(bytes)
}

fn copy_fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], RnsError> {
    if bytes.len() < N {
        return Err(RnsError::PacketError);
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[..N]);
    Ok(out)
}


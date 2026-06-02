pub(crate) fn validate_link_request_proof_packet(
    destination: &DestinationDesc,
    id: &LinkId,
    packet: &Packet,
) -> Result<Identity, RnsError> {
    const MIN_PROOF_LEN: usize = SIGNATURE_LENGTH + PUBLIC_KEY_LENGTH;
    const MTU_PROOF_LEN: usize = SIGNATURE_LENGTH + PUBLIC_KEY_LENGTH + LINK_MTU_SIZE;
    const SIGN_DATA_LEN: usize = ADDRESS_HASH_SIZE + PUBLIC_KEY_LENGTH * 2 + LINK_MTU_SIZE;

    if packet.data.len() < MIN_PROOF_LEN {
        return Err(RnsError::PacketError);
    }

    let mut proof_data = [0u8; SIGN_DATA_LEN];

    let verifying_key = destination.identity.verifying_key.as_bytes();
    let sign_data_len = {
        let mut output = OutputBuffer::new(&mut proof_data[..]);

        output.write(id.as_slice())?;
        output.write(
            &packet.data.as_slice()[SIGNATURE_LENGTH..SIGNATURE_LENGTH + PUBLIC_KEY_LENGTH],
        )?;
        output.write(verifying_key)?;

        if packet.data.len() >= MTU_PROOF_LEN {
            let mtu_bytes = &packet.data.as_slice()[SIGNATURE_LENGTH + PUBLIC_KEY_LENGTH..];
            output.write(mtu_bytes)?;
        }

        output.offset()
    };

    let identity = Identity::new_from_slices(
        &proof_data[ADDRESS_HASH_SIZE..ADDRESS_HASH_SIZE + PUBLIC_KEY_LENGTH],
        verifying_key,
    );

    let signature = Signature::from_slice(&packet.data.as_slice()[..SIGNATURE_LENGTH])
        .map_err(|_| RnsError::CryptoError)?;

    identity
        .verify(&proof_data[..sign_data_len], &signature)
        .map_err(|_| RnsError::IncorrectSignature)?;

    Ok(identity)
}

pub(crate) fn validate_link_packet_proof(
    identity: &Identity,
    id: &LinkId,
    packet: &Packet,
) -> Result<Hash, RnsError> {
    if packet.header.packet_type != PacketType::Proof
        || packet.header.destination_type != DestinationType::Link
        || packet.context != PacketContext::LinkProof
    {
        return Err(RnsError::PacketError);
    }
    if packet.destination != *id {
        return Err(RnsError::IncorrectHash);
    }
    if packet.data.len() < HASH_SIZE + SIGNATURE_LENGTH {
        return Err(RnsError::PacketError);
    }

    let mut hash = [0u8; HASH_SIZE];
    hash.copy_from_slice(&packet.data.as_slice()[..HASH_SIZE]);
    let signature = Signature::from_slice(&packet.data.as_slice()[HASH_SIZE..HASH_SIZE + SIGNATURE_LENGTH])
        .map_err(|_| RnsError::CryptoError)?;

    identity.verify(&hash, &signature)?;

    Ok(Hash::new(hash))
}

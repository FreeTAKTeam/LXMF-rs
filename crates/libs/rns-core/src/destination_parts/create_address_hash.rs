fn create_address_hash<I: HashIdentity>(identity: &I, name: &DestinationName) -> AddressHash {
    AddressHash::new_from_hash(&Hash::new(
        Hash::generator()
            .chain_update(name.as_name_hash_slice())
            .chain_update(identity.as_address_hash_slice())
            .finalize()
            .into(),
    ))
}

#[allow(clippy::too_many_arguments)]
fn verify_announce_with_buffer(
    identity: &Identity,
    destination: &[u8],
    public_key: &[u8],
    verifying_key: &[u8],
    name_hash: &[u8],
    rand_hash: &[u8],
    ratchet: Option<&[u8]>,
    signature: &[u8],
    app_data: &[u8],
    signed_data: &mut [u8],
) -> Result<(), RnsError> {
    let required_len = destination.len()
        + public_key.len()
        + verifying_key.len()
        + name_hash.len()
        + rand_hash.len()
        + ratchet.map(|value| value.len()).unwrap_or(0)
        + app_data.len();
    if required_len > signed_data.len() {
        return Err(RnsError::OutOfMemory);
    }

    let mut offset = 0usize;
    signed_data[offset..offset + destination.len()].copy_from_slice(destination);
    offset += destination.len();
    signed_data[offset..offset + public_key.len()].copy_from_slice(public_key);
    offset += public_key.len();
    signed_data[offset..offset + verifying_key.len()].copy_from_slice(verifying_key);
    offset += verifying_key.len();
    signed_data[offset..offset + name_hash.len()].copy_from_slice(name_hash);
    offset += name_hash.len();
    signed_data[offset..offset + rand_hash.len()].copy_from_slice(rand_hash);
    offset += rand_hash.len();
    if let Some(ratchet) = ratchet {
        signed_data[offset..offset + ratchet.len()].copy_from_slice(ratchet);
        offset += ratchet.len();
    }
    if !app_data.is_empty() {
        signed_data[offset..offset + app_data.len()].copy_from_slice(app_data);
        offset += app_data.len();
    }

    let signature = Signature::from_slice(signature).map_err(|_| RnsError::CryptoError)?;
    identity.verify(&signed_data[..offset], &signature).map_err(|_| RnsError::IncorrectSignature)
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

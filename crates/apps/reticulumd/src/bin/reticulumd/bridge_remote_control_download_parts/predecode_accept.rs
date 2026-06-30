fn prepare_downloaded_propagation_wire(
    daemon: &RpcDaemon,
    destination: &SingleInputDestination,
    transient_payload: &[u8],
) -> Result<([u8; 16], Vec<u8>), std::io::Error> {
    let mut destination_hash = [0u8; 16];
    destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
    if transient_payload.len() <= 16 + 32 {
        emit_propagation_predecode_drop_event(
            daemon,
            destination_hash,
            transient_payload,
            "payload_too_short",
            "propagated LXMF payload too short",
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "propagated LXMF payload too short",
        ));
    }
    if &transient_payload[..16] != destination.desc.address_hash.as_slice() {
        emit_propagation_predecode_drop_event(
            daemon,
            destination_hash,
            transient_payload,
            "destination_mismatch",
            "propagated LXMF payload is not addressed to local delivery destination",
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "propagated LXMF payload is not addressed to local delivery destination",
        ));
    }
    match decrypt_local_propagated_wire(destination, transient_payload) {
        Ok(wire) => Ok((destination_hash, wire)),
        Err(error) => {
            emit_propagation_predecode_drop_event(
                daemon,
                destination_hash,
                transient_payload,
                "decrypt_failed",
                error.to_string(),
            );
            Err(error)
        }
    }
}

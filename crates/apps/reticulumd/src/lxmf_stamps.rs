//! LXMF stamp and ticket helpers. The implementations live in `lxmf-wire`
//! (`lxmf::stamp`) so library consumers share them with the daemon; this
//! module keeps the daemon's paths and adds the RPC-facing hex decoding.

pub use lxmf::stamp::{
    generate_peering_key, generate_propagation_stamp, generate_propagation_stamp_until_cancelled,
    generate_propagation_stamp_with_value_until_cancelled, generate_stamp,
    generate_stamp_until_cancelled, invalid_stamp_value, stamp_valid, stamp_value, stamp_workblock,
    ticket_stamp, validate_peering_key, validate_propagation_stamp, validate_stamp, COST_TICKET,
    PROPAGATION_STAMP_SIZE, TICKET_LENGTH,
};

/// `LXMF.FIELD_TICKET` as the signed msgpack key the daemon's field maps use.
pub const FIELD_TICKET: i64 = lxmf::constants::FIELD_TICKET as i64;

pub fn decode_ticket_hex(ticket_hex: &str) -> Result<Vec<u8>, String> {
    let bytes = hex::decode(ticket_hex.trim())
        .map_err(|error| format!("invalid outbound ticket hex: {error}"))?;
    if bytes.len() != TICKET_LENGTH {
        return Err(format!(
            "invalid outbound ticket length {}; expected {} bytes",
            bytes.len(),
            TICKET_LENGTH
        ));
    }
    Ok(bytes)
}

use super::*;
use lxmf::WireMessage;
use rand_core::OsRng;
use reticulum_daemon::lxmf_bridge::{build_wire_message_with_options, rmpv_to_json};
use rns_core::identity::Identity as CoreIdentity;
use rns_rpc::{PaperDecodeOutcome, PaperEncodeEnvelope};

pub(super) fn encode_paper(
    bridge: &TransportBridge,
    record: &rns_rpc::MessageRecord,
) -> Result<Option<PaperEncodeEnvelope>, std::io::Error> {
    let destination = parse_destination_hash_required(&record.destination)?;
    let destination_identity = bridge
        .peer_crypto
        .lock()
        .expect("peer map")
        .get(&record.destination)
        .map(|info| info.identity)
        .or_else(|| {
            resolve_destination_identity_blocking(
                bridge.transport.clone(),
                AddressHash::new(destination),
                Duration::from_secs(12),
            )
            .unwrap_or_else(|err| {
                log::warn!("[daemon] identity resolver for paper delivery: {err}");
                None
            })
        });
    let Some(destination_identity) = destination_identity else {
        return Ok(None);
    };
    let service_identity =
        bridge.service_identity_for_destination(record.source.as_str()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("source delivery destination {} is not registered", record.source),
            )
        })?;
    let mut source_hash = [0u8; 16];
    source_hash.copy_from_slice(
        parse_destination_hash_required(service_identity.delivery_destination.as_str())?.as_slice(),
    );
    let payload = build_wire_message_with_options(
        source_hash,
        destination,
        &record.title,
        &record.content,
        record.fields.clone(),
        &service_identity.identity,
        None,
        None,
        None,
    )
    .map_err(std::io::Error::other)?;
    let wire = WireMessage::unpack(payload.as_slice()).map_err(std::io::Error::other)?;
    let transient_id = hex::encode(wire.try_message_id().map_err(std::io::Error::other)?);
    let destination_identity = CoreIdentity::new_from_slices(
        destination_identity.public_key_bytes(),
        destination_identity.verifying_key_bytes(),
    );
    let uri = wire
        .pack_paper_uri_with_rng(&destination_identity, OsRng)
        .map_err(std::io::Error::other)?;
    Ok(Some(PaperEncodeEnvelope {
        uri,
        transient_id,
        destination_hint: record.destination.clone(),
        extensions: serde_json::Map::new(),
    }))
}

pub(super) fn decode_paper_uri(
    bridge: &TransportBridge,
    uri: &str,
) -> Result<Option<PaperDecodeOutcome>, std::io::Error> {
    let wire = WireMessage::unpack_paper_uri(uri, &bridge.signer).map_err(std::io::Error::other)?;
    let raw_lxmf_bytes = wire.pack().map_err(std::io::Error::other)?;
    let transient_id = hex::encode(wire.try_message_id().map_err(std::io::Error::other)?);
    let destination_hint = hex::encode(wire.destination);
    let record = rns_rpc::MessageRecord {
        id: transient_id.clone(),
        source: hex::encode(wire.source),
        destination: destination_hint.clone(),
        title: wire
            .payload
            .title
            .as_ref()
            .map(|title| String::from_utf8_lossy(title).to_string())
            .unwrap_or_default(),
        content: wire
            .payload
            .content
            .as_ref()
            .map(|content| String::from_utf8_lossy(content).to_string())
            .unwrap_or_default(),
        timestamp: wire.payload.timestamp as i64,
        direction: "in".to_string(),
        fields: wire
            .payload
            .fields
            .as_ref()
            .map(rmpv_to_json)
            .transpose()
            .map_err(std::io::Error::other)?,
        receipt_status: None,
    };
    Ok(Some(PaperDecodeOutcome {
        transient_id,
        destination_hint,
        record: Some(record),
        raw_lxmf_bytes: Some(raw_lxmf_bytes),
    }))
}

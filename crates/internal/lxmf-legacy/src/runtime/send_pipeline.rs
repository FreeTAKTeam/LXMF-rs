use super::{
    build_propagation_envelope, build_wire_message, can_send_opportunistic, clean_non_empty,
    format_relay_request_status, is_message_marked_delivered, normalize_relay_destination_hash,
    opportunistic_payload, parse_delivery_method, persist_peer_identity_cache,
    propagation_relay_candidates, prune_receipt_mappings_for_message,
    sanitize_outbound_wire_fields, send_outcome_is_sent, send_outcome_status, short_hash_prefix,
    track_outbound_resource_mapping, track_receipt_mapping, trigger_rate_limited_announce,
    wait_for_external_relay_selection, DeliveryMethod, EmbeddedTransportBridge,
    OutboundDeliveryOptionsCompat, PeerCrypto, ReceiptEvent, MAX_ALTERNATIVE_PROPAGATION_RELAYS,
    POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS,
};
use reticulum::delivery::{send_via_link as shared_send_via_link, LinkSendResult};
use reticulum::destination::{DestinationDesc, DestinationName};
use reticulum::destination_hash::{
    parse_destination_hash as parse_destination_hex,
    parse_destination_hash_required as parse_destination_hex_required,
};
use reticulum::hash::AddressHash;
use reticulum::identity::PrivateIdentity;
use reticulum::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use reticulum::storage::messages::MessageRecord;
use std::time::Duration;

#[derive(Clone, Copy)]
struct DeliveryMethodPlan {
    requested: DeliveryMethod,
    effective: DeliveryMethod,
    allow_link: bool,
    allow_opportunistic: bool,
    allow_propagated: bool,
}

impl DeliveryMethodPlan {
    fn from_request(
        requested: DeliveryMethod,
        opportunistic_supported: bool,
        try_propagation_on_fail: bool,
    ) -> Self {
        let effective =
            if matches!(requested, DeliveryMethod::Opportunistic) && !opportunistic_supported {
                DeliveryMethod::Direct
            } else {
                requested
            };

        Self {
            requested,
            effective,
            allow_link: matches!(effective, DeliveryMethod::Auto | DeliveryMethod::Direct),
            allow_opportunistic: matches!(
                effective,
                DeliveryMethod::Auto | DeliveryMethod::Opportunistic
            ),
            allow_propagated: matches!(
                effective,
                DeliveryMethod::Auto | DeliveryMethod::Propagated
            ) || try_propagation_on_fail,
        }
    }

    fn downgraded_to_direct(self) -> bool {
        !matches!(self.requested, DeliveryMethod::Auto) && self.requested != self.effective
    }
}

fn resolve_signer_and_source_hash(
    bridge: &EmbeddedTransportBridge,
    requested_source: &str,
    source_private_key: Option<String>,
) -> Result<(PrivateIdentity, [u8; 16]), std::io::Error> {
    if let Some(source_private_key) = clean_non_empty(source_private_key) {
        let source_key_bytes = hex::decode(source_private_key.trim()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "source_private_key must be hex-encoded",
            )
        })?;
        let signer = PrivateIdentity::from_private_key_bytes(&source_key_bytes).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "source_private_key is not a valid identity private key",
            )
        })?;
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(signer.address_hash().as_slice());
        return Ok((signer, source_hash));
    }

    if let Some(parsed_source) = parse_destination_hex(requested_source) {
        if parsed_source != bridge.delivery_source_hash {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "source hash differs from runtime identity; set source_private_key for per-message source identities",
            ));
        }
    }

    Ok((bridge.signer.clone(), bridge.delivery_source_hash))
}

fn ticket_status(include_ticket: bool, ticket_present: bool) -> Option<&'static str> {
    if !include_ticket {
        return None;
    }
    if ticket_present {
        Some("ticket: present")
    } else {
        Some("ticket: unavailable")
    }
}

include!("send_pipeline/deliver.rs");

use reticulum::hash::Hash;
use reticulum::receipt::{
    prune_receipt_mappings_for_message as shared_prune_receipt_mappings_for_message,
    track_receipt_mapping as shared_track_receipt_mapping,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub(super) fn format_relay_request_status(exclude_relays: &[String]) -> String {
    if exclude_relays.is_empty() {
        return "retrying: requesting alternative propagation relay".to_string();
    }
    format!(
        "retrying: requesting alternative propagation relay;exclude={}",
        exclude_relays.join(",")
    )
}

pub(super) fn parse_alternative_relay_request_status(status: &str) -> Option<Vec<String>> {
    const PREFIX: &str = "retrying: requesting alternative propagation relay";
    if !status.starts_with(PREFIX) {
        return None;
    }
    let excludes_raw = status.split(";exclude=").nth(1).unwrap_or_default();
    let exclude_relays = excludes_raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    Some(exclude_relays)
}

pub(super) fn track_receipt_mapping(
    map: &Arc<Mutex<HashMap<String, String>>>,
    packet_hash: &str,
    message_id: &str,
) {
    shared_track_receipt_mapping(map, packet_hash, message_id);
}

pub(super) fn track_outbound_resource_mapping(
    map: &Arc<Mutex<HashMap<String, String>>>,
    resource_hash: &Hash,
    message_id: &str,
) {
    if let Ok(mut guard) = map.lock() {
        guard.insert(hex::encode(resource_hash.as_slice()), message_id.to_string());
    }
}

pub(super) fn is_message_marked_delivered(
    delivered_messages: &Arc<Mutex<HashSet<String>>>,
    message_id: &str,
) -> bool {
    delivered_messages.lock().map(|guard| guard.contains(message_id)).unwrap_or(false)
}

pub(super) fn prune_receipt_mappings_for_message(
    receipt_map: &Arc<Mutex<HashMap<String, String>>>,
    message_id: &str,
) {
    shared_prune_receipt_mappings_for_message(receipt_map, message_id);
}

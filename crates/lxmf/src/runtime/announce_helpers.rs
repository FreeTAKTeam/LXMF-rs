use super::PeerAnnounceMeta;
use crate::helpers::{display_name_from_app_data, is_msgpack_array_prefix, normalize_display_name};
use reticulum::destination::DestinationName;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn encode_delivery_display_name_app_data(display_name: &str) -> Option<Vec<u8>> {
    let peer_data = rmpv::Value::Array(vec![
        rmpv::Value::Binary(display_name.as_bytes().to_vec()),
        rmpv::Value::Nil,
    ]);
    rmp_serde::to_vec(&peer_data).ok()
}

pub(super) fn encode_propagation_node_app_data(display_name: Option<&str>) -> Option<Vec<u8>> {
    let mut metadata = Vec::new();
    if let Some(name) = display_name {
        metadata.push((
            rmpv::Value::Integer((crate::constants::PN_META_NAME as i64).into()),
            rmpv::Value::Binary(name.as_bytes().to_vec()),
        ));
    }

    let announce_data = rmpv::Value::Array(vec![
        rmpv::Value::Boolean(false),
        rmpv::Value::Integer((now_epoch_secs() as i64).into()),
        rmpv::Value::Boolean(true),
        rmpv::Value::Integer((crate::constants::PROPAGATION_LIMIT as i64).into()),
        rmpv::Value::Integer((crate::constants::SYNC_LIMIT as i64).into()),
        rmpv::Value::Array(vec![
            rmpv::Value::Integer((crate::constants::PROPAGATION_COST as i64).into()),
            rmpv::Value::Integer((crate::constants::PROPAGATION_COST_FLEX as i64).into()),
            rmpv::Value::Integer((crate::constants::PEERING_COST as i64).into()),
        ]),
        rmpv::Value::Map(metadata),
    ]);
    rmp_serde::to_vec(&announce_data).ok()
}

pub(super) fn parse_peer_name_from_app_data(app_data: &[u8]) -> Option<(String, String)> {
    if app_data.is_empty() {
        return None;
    }

    if is_msgpack_array_prefix(app_data[0]) {
        if let Some(name) = display_name_from_app_data(app_data)
            .and_then(|value| normalize_display_name(&value).ok())
        {
            return Some((name, "delivery_app_data".to_string()));
        }
    }

    if let Some(name) = crate::helpers::pn_name_from_app_data(app_data)
        .and_then(|value| normalize_display_name(&value).ok())
    {
        return Some((name, "pn_meta".to_string()));
    }

    let text = std::str::from_utf8(app_data).ok()?;
    let name = normalize_display_name(text).ok()?;
    Some((name, "app_data_utf8".to_string()))
}

pub(super) fn lxmf_aspect_from_name_hash(name_hash: &[u8]) -> Option<String> {
    let delivery = DestinationName::new("lxmf", "delivery");
    if name_hash == delivery.as_name_hash_slice() {
        return Some("lxmf.delivery".to_string());
    }

    let propagation = DestinationName::new("lxmf", "propagation");
    if name_hash == propagation.as_name_hash_slice() {
        return Some("lxmf.propagation".to_string());
    }

    let rmsp_maps = DestinationName::new("rmsp", "maps");
    if name_hash == rmsp_maps.as_name_hash_slice() {
        return Some("rmsp.maps".to_string());
    }

    None
}

pub(super) fn update_peer_announce_meta(
    peer_announce_meta: &Arc<Mutex<HashMap<String, PeerAnnounceMeta>>>,
    peer: &str,
    app_data: &[u8],
) {
    let app_data_hex = if app_data.is_empty() { None } else { Some(hex::encode(app_data)) };

    let mut guard = peer_announce_meta.lock().expect("peer metadata map");
    guard.insert(peer.to_string(), PeerAnnounceMeta { app_data_hex });
}

pub(super) fn annotate_peer_records_with_announce_metadata(
    result: &mut Value,
    metadata: &HashMap<String, PeerAnnounceMeta>,
) {
    if metadata.is_empty() {
        return;
    }

    if let Some(object) = result.as_object_mut() {
        if let Some(Value::Array(peers)) = object.get_mut("peers") {
            annotate_peer_array(peers, metadata);
        }
        return;
    }

    if let Value::Array(peers) = result {
        annotate_peer_array(peers, metadata);
    }
}

fn annotate_peer_array(peers: &mut [Value], metadata: &HashMap<String, PeerAnnounceMeta>) {
    for peer in peers {
        let Some(record) = peer.as_object_mut() else {
            continue;
        };
        let Some(peer_hash) = record.get("peer").and_then(Value::as_str) else {
            continue;
        };
        let Some(meta) = metadata.get(peer_hash) else {
            continue;
        };
        if let Some(app_data_hex) = meta.app_data_hex.as_ref() {
            record.insert("app_data_hex".to_string(), Value::String(app_data_hex.clone()));
        }
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

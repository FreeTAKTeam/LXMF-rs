use super::PeerCrypto;
use reticulum::destination_hash::parse_destination_hash as shared_parse_destination_hash;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(super) fn propagation_relay_candidates(
    selected_propagation_node: &Arc<Mutex<Option<String>>>,
    known_propagation_nodes: &Arc<Mutex<HashSet<String>>>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    let selected = selected_propagation_node
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(selected) = selected {
        seen.insert(selected.clone());
        candidates.push(selected);
    }

    let mut known = known_propagation_nodes
        .lock()
        .map(|guard| guard.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    known.sort();
    for candidate in known {
        if seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    }

    candidates
}

pub(super) fn short_hash_prefix(value: &str) -> String {
    value.chars().take(12).collect::<String>()
}

pub(super) fn normalize_relay_destination_hash(
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    selected_hash: &str,
) -> Option<String> {
    let selected_destination = shared_parse_destination_hash(selected_hash)?;
    let guard = peer_crypto.lock().ok()?;
    if guard.contains_key(selected_hash) {
        return Some(selected_hash.to_string());
    }
    for (destination_hash, crypto) in guard.iter() {
        if crypto.identity.address_hash.as_slice() == selected_destination {
            return Some(destination_hash.clone());
        }
    }
    None
}

pub(super) async fn wait_for_external_relay_selection(
    selected_propagation_node: &Arc<Mutex<Option<String>>>,
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    attempted_relays: &[String],
    timeout: Duration,
) -> Option<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let selected = selected_propagation_node
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(selected) = selected {
            let normalized =
                normalize_relay_destination_hash(peer_crypto, &selected).unwrap_or(selected);
            if !attempted_relays.iter().any(|relay| relay == &normalized) {
                return Some(normalized);
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    None
}

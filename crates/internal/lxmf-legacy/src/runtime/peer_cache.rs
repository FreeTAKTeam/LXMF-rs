use super::PeerCrypto;
use crate::LxmfError;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use reticulum::destination::{DestinationName, SingleOutputDestination};
use reticulum::identity::Identity;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPeerIdentity {
    destination: String,
    identity_hex: String,
}

pub(super) fn load_peer_identity_cache(
    path: &Path,
) -> Result<HashMap<String, PeerCrypto>, LxmfError> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = fs::read_to_string(path).map_err(|err| LxmfError::Io(err.to_string()))?;
    let entries: Vec<PersistedPeerIdentity> =
        serde_json::from_str(&raw).map_err(|err| LxmfError::Decode(err.to_string()))?;
    let mut out = HashMap::new();
    for entry in entries {
        let Some(destination) = normalize_hash_hex_16(&entry.destination) else {
            continue;
        };
        let Ok(identity) = Identity::new_from_hex_string(entry.identity_hex.trim()) else {
            continue;
        };
        out.insert(destination, PeerCrypto { identity });
    }
    Ok(out)
}

pub(super) fn persist_peer_identity_cache(
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    path: &Path,
) {
    let snapshot = peer_crypto
        .lock()
        .map(|guard| {
            let mut entries = guard
                .iter()
                .map(|(destination, crypto)| PersistedPeerIdentity {
                    destination: destination.clone(),
                    identity_hex: crypto.identity.to_hex_string(),
                })
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.destination.cmp(&b.destination));
            entries
        })
        .unwrap_or_default();

    let encoded = match serde_json::to_string_pretty(&snapshot) {
        Ok(encoded) => encoded,
        Err(_) => return,
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, encoded).is_ok() {
        let _ = fs::rename(&tmp, path);
    } else {
        let _ = fs::remove_file(&tmp);
    }
}

pub(super) fn apply_runtime_identity_restore(
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    cache_path: &Path,
    method: &str,
    params: Option<&Value>,
) {
    let Some(params) = params.and_then(Value::as_object) else {
        return;
    };

    match method {
        "store_peer_identity" => {
            let identity_hash = params.get("identity_hash").and_then(Value::as_str);
            let public_key = params.get("public_key").and_then(Value::as_str);
            if let (Some(identity_hash), Some(public_key)) = (identity_hash, public_key) {
                register_peer_identity(peer_crypto, identity_hash, public_key);
            }
        }
        "restore_all_peer_identities" | "bulk_restore_peer_identities" => {
            if let Some(peers) = params.get("peers").and_then(Value::as_array) {
                for peer in peers {
                    let Some(peer) = peer.as_object() else {
                        continue;
                    };
                    let identity_hash = peer.get("identity_hash").and_then(Value::as_str);
                    let public_key = peer.get("public_key").and_then(Value::as_str);
                    if let (Some(identity_hash), Some(public_key)) = (identity_hash, public_key) {
                        register_peer_identity(peer_crypto, identity_hash, public_key);
                    }
                }
            }
        }
        "bulk_restore_announce_identities" => {
            if let Some(announces) = params.get("announces").and_then(Value::as_array) {
                for announce in announces {
                    let Some(announce) = announce.as_object() else {
                        continue;
                    };
                    let destination = announce.get("destination_hash").and_then(Value::as_str);
                    let public_key = announce.get("public_key").and_then(Value::as_str);
                    if let (Some(destination), Some(public_key)) = (destination, public_key) {
                        register_destination_identity(peer_crypto, destination, public_key);
                    }
                }
            }
        }
        _ => {}
    }

    persist_peer_identity_cache(peer_crypto, cache_path);
}

fn register_peer_identity(
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    identity_hash_hex: &str,
    public_key_material: &str,
) {
    let Some(identity_hash) = normalize_hash_hex_16(identity_hash_hex) else {
        return;
    };
    let Some(identity) = identity_from_public_key_material(public_key_material) else {
        return;
    };

    let destination =
        SingleOutputDestination::new(identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = hex::encode(destination.desc.address_hash.as_slice());

    if identity_hash != hex::encode(identity.address_hash.as_slice()) {
        // Public key material is source of truth for identity derivation.
    }

    if let Ok(mut guard) = peer_crypto.lock() {
        guard.insert(destination_hash, PeerCrypto { identity });
    }
}

fn register_destination_identity(
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    destination_hash: &str,
    public_key_material: &str,
) {
    let Some(destination_hash) = normalize_hash_hex_16(destination_hash) else {
        return;
    };
    let Some(identity) = identity_from_public_key_material(public_key_material) else {
        return;
    };
    if let Ok(mut guard) = peer_crypto.lock() {
        guard.insert(destination_hash, PeerCrypto { identity });
    }
}

fn identity_from_public_key_material(public_key_material: &str) -> Option<Identity> {
    let bytes = decode_key_material_bytes(public_key_material)?;
    if bytes.len() != 64 {
        return None;
    }
    Some(Identity::new_from_slices(&bytes[..32], &bytes[32..64]))
}

fn decode_key_material_bytes(value: &str) -> Option<Vec<u8>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() % 2 == 0 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return hex::decode(trimmed).ok();
    }
    BASE64_STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(trimmed))
        .ok()
}

fn normalize_hash_hex_16(value: &str) -> Option<String> {
    let bytes = hex::decode(value.trim()).ok()?;
    let mut normalized = [0u8; 16];
    match bytes.len() {
        16 => normalized.copy_from_slice(&bytes),
        32 => normalized.copy_from_slice(&bytes[..16]),
        _ => return None,
    }
    Some(hex::encode(normalized))
}

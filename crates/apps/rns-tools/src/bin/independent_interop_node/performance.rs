use super::model::SharedState;
use rns_transport::hash::AddressHash;
use rns_transport::transport::Transport;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub async fn prepare_resource(state: &SharedState, params: &Value) -> Result<Value, String> {
    let size = params
        .get("size")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| "size must be a usize".to_string())?;
    let seed = params.get("seed").and_then(Value::as_u64).unwrap_or(0x4c584d46);
    let key = format!("{seed:016x}-{size}");
    let payload = deterministic_payload(size, seed);
    let digest = hex::encode(Sha256::digest(&payload));
    state.prepared_resources.write().await.insert(key.clone(), payload);
    Ok(json!({"key": key, "bytes": size, "sha256": digest}))
}

pub async fn send_prepared_resource(
    transport: &Transport,
    state: &SharedState,
    params: &Value,
) -> Result<Value, String> {
    let link_id = params
        .get("link_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing string parameter link_id".to_string())?;
    let link_id = AddressHash::new_from_hex_string(link_id.trim_matches('/'))
        .map_err(|error| format!("invalid link_id: {error:?}"))?;
    let key = params
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing string parameter key".to_string())?;
    let payload = state
        .prepared_resources
        .write()
        .await
        .remove(key)
        .ok_or_else(|| format!("unknown prepared resource {key}"))?;
    let hash = transport
        .send_resource(&link_id, payload, None)
        .await
        .map_err(|error| format!("send prepared resource: {error:?}"))?;
    Ok(json!({"resource_hash": hex::encode(hash.as_slice())}))
}

fn deterministic_payload(size: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.max(1);
    (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

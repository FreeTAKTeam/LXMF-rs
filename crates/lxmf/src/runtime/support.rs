use crate::cli::profile::InterfaceEntry;
use crate::LxmfError;
use reticulum::identity::PrivateIdentity;
use reticulum::rpc::InterfaceRecord;
use serde_json::Value;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn parse_bind_host_port(bind: &str) -> Option<(String, u16)> {
    if let Ok(addr) = bind.parse::<SocketAddr>() {
        return Some((addr.ip().to_string(), addr.port()));
    }

    let (host, port) = bind.rsplit_once(':')?;
    Some((host.to_string(), port.parse::<u16>().ok()?))
}

pub(super) fn clean_non_empty(value: Option<String>) -> Option<String> {
    value.map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

pub(super) fn source_hash_from_private_key_hex(private_key_hex: &str) -> Result<String, LxmfError> {
    let key_bytes = hex::decode(private_key_hex.trim())
        .map_err(|_| LxmfError::Io("source_private_key must be hex-encoded".to_string()))?;
    let identity = PrivateIdentity::from_private_key_bytes(&key_bytes)
        .map_err(|_| LxmfError::Io("source_private_key is invalid".to_string()))?;
    Ok(hex::encode(identity.address_hash().as_slice()))
}

pub(super) fn generate_message_id() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    format!("lxmf-{now}")
}

pub(super) fn interface_to_rpc(entry: InterfaceEntry) -> InterfaceRecord {
    InterfaceRecord {
        kind: entry.kind,
        enabled: entry.enabled,
        host: entry.host,
        port: entry.port,
        name: Some(entry.name),
    }
}

pub(super) fn extract_identity_hash(status: &Value) -> Option<String> {
    for key in ["delivery_destination_hash", "identity_hash"] {
        if let Some(hash) = status
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|candidate| !candidate.is_empty())
        {
            return Some(hash.to_string());
        }
    }
    None
}

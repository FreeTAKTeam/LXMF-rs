#[cfg(feature = "rnode-ble")]
async fn rnode_peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
) -> Result<bool, String> {
    if native_rnode_identifier_matches(configured_id, &peripheral.id().to_string()) {
        return Ok(true);
    }
    let properties = peripheral
        .properties()
        .await
        .map_err(|err| format!("read peripheral properties: {err}"))?;
    if let Some(properties) = properties {
        if native_rnode_identifier_matches(configured_id, &properties.address.to_string()) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if native_rnode_identifier_matches(configured_id, &local_name) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(feature = "rnode-ble")]
fn parse_rnode_uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("RNode BLE UUID constants must be valid")
}

#[cfg(feature = "rnode-ble")]
fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

#[cfg(feature = "rnode-ble")]
async fn rnode_peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
    aliases: &[String],
    service_uuid: Uuid,
    allow_service_uuid_match: bool,
) -> Result<bool, String> {
    let peripheral_id = peripheral.id().to_string();

    if native_rnode_identifier_matches(configured_id, &peripheral.id().to_string()) {
        return Ok(true);
    }
    if aliases.iter().any(|alias| native_rnode_identifier_matches(alias, &peripheral_id)) {
        return Ok(true);
    }
    let properties = peripheral
        .properties()
        .await
        .map_err(|err| format!("read peripheral properties: {err}"))?;
    if let Some(properties) = properties {
        let address = properties.address.to_string();
        if native_rnode_identifier_matches(configured_id, &properties.address.to_string()) {
            return Ok(true);
        }
        if aliases.iter().any(|alias| native_rnode_identifier_matches(alias, &address)) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if native_rnode_identifier_matches(configured_id, &local_name) {
                return Ok(true);
            }
            if aliases.iter().any(|alias| native_rnode_identifier_matches(alias, &local_name)) {
                return Ok(true);
            }
        }
        if allow_service_uuid_match && properties.services.contains(&service_uuid) {
            log::warn!(
                "RNode BLE fallback matched advertised service without configured identifier"
            );
            return Ok(true);
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

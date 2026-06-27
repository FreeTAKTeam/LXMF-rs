#[cfg(feature = "rnode-ble")]
async fn rnode_peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
    aliases: &[String],
    exclude_exact_identifier: Option<&str>,
    service_uuid: Uuid,
    allow_service_uuid_match: bool,
) -> Result<bool, String> {
    let peripheral_id = peripheral.id().to_string();
    if !identifier_is_excluded(&peripheral_id, exclude_exact_identifier)
        && rnode_identifier_matches_any(&peripheral_id, configured_id, aliases)
    {
        return Ok(true);
    }
    let properties = peripheral
        .properties()
        .await
        .map_err(|err| format!("read peripheral properties: {err}"))?;
    if let Some(properties) = properties {
        let address = properties.address.to_string();
        if !identifier_is_excluded(&address, exclude_exact_identifier)
            && rnode_identifier_matches_any(&address, configured_id, aliases)
        {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if rnode_identifier_matches_any(&local_name, configured_id, aliases) {
                return Ok(true);
            }
            if aliases.iter().any(|alias| native_rnode_identifier_matches(alias, &local_name)) {
                return Ok(true);
            }
        }
        if allow_service_uuid_match && properties.services.contains(&service_uuid) {
            log::info!(
                "RNode BLE fallback matched advertised service uuid={} address={} configured_id={}",
                service_uuid,
                address,
                configured_id
            );
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(feature = "rnode-ble")]
fn rnode_identifier_matches_any(candidate: &str, configured_id: &str, aliases: &[String]) -> bool {
    native_rnode_identifier_matches(configured_id, candidate)
        || aliases.iter().any(|alias| native_rnode_identifier_matches(alias, candidate))
}

#[cfg(feature = "rnode-ble")]
fn identifier_is_excluded(candidate: &str, excluded: Option<&str>) -> bool {
    excluded.is_some_and(|excluded| native_rnode_identifier_matches(excluded, candidate))
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

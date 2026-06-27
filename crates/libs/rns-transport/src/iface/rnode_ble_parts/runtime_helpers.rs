#[cfg(feature = "rnode-ble")]
async fn rnode_peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
    aliases: &[String],
    exclude_exact_identifier: Option<&str>,
    service_uuid: Uuid,
    allow_service_uuid_match: bool,
    excluded_identifiers: &[String],
) -> Result<bool, String> {
    let peripheral_id = peripheral.id().to_string();
    if rnode_identifier_is_excluded(&peripheral_id, exclude_exact_identifier, excluded_identifiers)
    {
        return Ok(false);
    }
    if native_rnode_identifier_matches_any(&peripheral_id, configured_id, aliases) {
        return Ok(true);
    }
    let properties = peripheral
        .properties()
        .await
        .map_err(|err| format!("read peripheral properties: {err}"))?;
    if let Some(properties) = properties {
        let address = properties.address.to_string();
        if rnode_identifier_is_excluded(&address, exclude_exact_identifier, excluded_identifiers) {
            return Ok(false);
        }
        if native_rnode_identifier_matches_any(&address, configured_id, aliases) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name.as_deref() {
            if rnode_identifier_is_excluded(local_name, exclude_exact_identifier, excluded_identifiers) {
                return Ok(false);
            }
            if native_rnode_identifier_matches_any(local_name, configured_id, aliases) {
                return Ok(true);
            }
        }
        if allow_service_uuid_match && properties.services.contains(&service_uuid) {
            log::warn!(
                "RNode BLE fallback matched advertised service without configured identifier peripheral_id={} address={} local_name={:?} service_uuid={}",
                peripheral_id,
                address,
                properties.local_name,
                service_uuid
            );
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(feature = "rnode-ble")]
pub fn native_rnode_identifier_matches_any(
    discovered: &str,
    configured_id: &str,
    aliases: &[String],
) -> bool {
    native_rnode_identifier_matches(configured_id, discovered)
        || aliases.iter().any(|alias| native_rnode_identifier_matches(alias, discovered))
}

#[cfg(feature = "rnode-ble")]
pub fn native_rnode_identifier_is_excluded(
    discovered: &str,
    excluded_identifiers: &[String],
) -> bool {
    excluded_identifiers
        .iter()
        .any(|excluded| native_rnode_identifier_matches(excluded, discovered))
}

#[cfg(feature = "rnode-ble")]
fn rnode_identifier_is_excluded(
    discovered: &str,
    exclude_exact_identifier: Option<&str>,
    excluded_identifiers: &[String],
) -> bool {
    exclude_exact_identifier
        .is_some_and(|excluded| native_rnode_identifier_matches(excluded, discovered))
        || native_rnode_identifier_is_excluded(discovered, excluded_identifiers)
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

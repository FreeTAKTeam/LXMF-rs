/// Enforces the inbound advertisement caps before any receiver state is
/// created (issue #514): a hostile peer must not get a `ResourceReceiver`
/// — and the part-tracking allocations that come with it — for a resource
/// that exceeds the transfer-size or part-count caps.
///
/// Returns `true` when the advertisement must be rejected. Each rejection
/// logs the exact limit, the advertised value, the resource hash, and the
/// peer link. `ResourceReceiver::new_with_mtu` re-checks both bounds as
/// defense in depth; these guards reject early with actionable context.
fn advertisement_exceeds_inbound_limits(
    advertisement: &ResourceAdvertisement,
    link_id: &AddressHash,
) -> bool {
    if advertisement.transfer_size > MAX_INBOUND_RESOURCE_TRANSFER_SIZE {
        log::warn!(
            "rejecting resource advertisement over transfer-size limit link={} hash={} transfer_size={} limit={}",
            link_id,
            advertisement.hash,
            advertisement.transfer_size,
            MAX_INBOUND_RESOURCE_TRANSFER_SIZE
        );
        log::debug!(
            "[resource-diag] reject_advertisement transfer_size_limit hash={}",
            advertisement.hash
        );
        return true;
    }
    if u64::from(advertisement.parts) > MAX_INBOUND_RESOURCE_PARTS {
        log::warn!(
            "rejecting resource advertisement over part-count limit link={} hash={} parts={} limit={}",
            link_id,
            advertisement.hash,
            advertisement.parts,
            MAX_INBOUND_RESOURCE_PARTS
        );
        log::debug!(
            "[resource-diag] reject_advertisement parts_limit hash={}",
            advertisement.hash
        );
        return true;
    }
    false
}

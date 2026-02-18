#[test]
fn announce_cache_evicts_oldest() {
    let mut cache = reticulum::transport::announce_table::AnnounceCache::new(2);
    cache.insert(
        reticulum::hash::AddressHash::new([0u8; 16]),
        reticulum::transport::announce_table::AnnounceEntry::dummy(),
    );
    cache.insert(
        reticulum::hash::AddressHash::new([1u8; 16]),
        reticulum::transport::announce_table::AnnounceEntry::dummy(),
    );
    cache.insert(
        reticulum::hash::AddressHash::new([2u8; 16]),
        reticulum::transport::announce_table::AnnounceEntry::dummy(),
    );
    assert_eq!(cache.len(), 2);
}

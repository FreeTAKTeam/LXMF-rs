use reticulum::hash::AddressHash;
use reticulum::transport::discovery::DiscoveryCache;

#[test]
fn discovery_cache_eviction() {
    let mut cache = DiscoveryCache::new(2);
    let a = AddressHash::new_from_slice(&[1u8; 16]);
    let b = AddressHash::new_from_slice(&[2u8; 16]);
    let c = AddressHash::new_from_slice(&[3u8; 16]);

    assert!(cache.mark_seen(a));
    assert!(cache.mark_seen(b));
    assert_eq!(cache.len(), 2);

    assert!(cache.mark_seen(c));
    assert_eq!(cache.len(), 2);
    assert!(!cache.seen(&a));
    assert!(cache.seen(&b));
    assert!(cache.seen(&c));
}

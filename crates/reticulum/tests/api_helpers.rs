use reticulum::buffer::StaticBuffer;
use reticulum::crypt::fernet::Token;
use reticulum::destination::link::LinkPayload;
use reticulum::destination::link_map::LinkMap;
use reticulum::hash::AddressHash;
use reticulum::transport::announce_table::{AnnounceCache, AnnounceTable};
use reticulum::transport::discovery::DiscoveryCache;
use reticulum::transport::path_table::PathTable;
use reticulum::utils::resolver::Resolver;

#[test]
fn helper_methods_exist_and_work() {
    let buffer = StaticBuffer::<64>::default();
    assert!(buffer.is_empty());

    let token = Token::from(b"".as_ref());
    assert!(token.is_empty());

    let payload = LinkPayload::default();
    assert!(payload.is_empty());

    let map = LinkMap::default();
    assert!(map.is_empty());

    let hash = AddressHash::default();
    assert!(hash.is_empty());

    let table = PathTable::default();
    assert!(table.is_empty());

    let discovery = DiscoveryCache::default();
    assert!(discovery.is_empty());

    let resolver = Resolver::default();
    assert!(resolver.is_empty());

    let announce_table = AnnounceTable::default();
    assert!(announce_table.is_empty());

    let announce_cache = AnnounceCache::new(1);
    assert!(announce_cache.is_empty());
}

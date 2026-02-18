use reticulum::hash::AddressHash;
use reticulum::identity::Identity;
use reticulum::utils::resolver::Resolver;

#[test]
fn resolver_inserts_and_resolves() {
    let id = Identity::new_from_hex_string(&"00".repeat(64)).unwrap();
    let hash = AddressHash::new_from_slice(id.public_key_bytes());

    let mut resolver = Resolver::new();
    assert_eq!(resolver.len(), 0);

    resolver.insert(hash, id);
    assert_eq!(resolver.len(), 1);
    assert!(resolver.resolve(&hash).is_some());
}

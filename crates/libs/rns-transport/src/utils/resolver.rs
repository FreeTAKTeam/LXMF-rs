use std::collections::HashMap;

use crate::hash::AddressHash;
use crate::identity::Identity;

#[derive(Default)]
pub struct Resolver {
    cache: HashMap<AddressHash, Identity>,
}

impl Resolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, hash: AddressHash, identity: Identity) {
        self.cache.insert(hash, identity);
    }

    pub fn resolve(&self, hash: &AddressHash) -> Option<&Identity> {
        self.cache.get(hash)
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::PrivateIdentity;
    use rand_core::OsRng;

    #[test]
    fn unresolved_identity_is_absent_like_python_reference() {
        let resolver = Resolver::new();

        assert!(resolver.resolve(&AddressHash::new([1_u8; 16])).is_none());
        assert!(resolver.is_empty());
    }

    #[test]
    fn cached_identity_resolution_exceeds_python_noop_surface() {
        let private_identity = PrivateIdentity::new_from_rand(OsRng);
        let identity = *private_identity.as_identity();
        let destination = AddressHash::new([2_u8; 16]);
        let mut resolver = Resolver::new();

        resolver.insert(destination, identity);

        let resolved = resolver.resolve(&destination).expect("cached identity");
        assert_eq!(resolved.address_hash, identity.address_hash);
        assert_eq!(resolver.len(), 1);
    }
}

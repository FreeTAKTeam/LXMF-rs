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

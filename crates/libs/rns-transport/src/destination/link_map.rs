use std::collections::HashMap;

use crate::hash::AddressHash;

use super::link::LinkId;

pub struct LinkMap {
    map: HashMap<AddressHash, LinkId>,
}

impl LinkMap {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn resolve(&self, address: &AddressHash) -> Option<LinkId> {
        self.map.get(address).copied()
    }

    pub fn insert(&mut self, address: &AddressHash, id: &LinkId) {
        self.map.insert(*address, *id);
    }

    pub fn remove(&mut self, address: &AddressHash) {
        self.map.remove(address);
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl Default for LinkMap {
    fn default() -> Self {
        Self::new()
    }
}

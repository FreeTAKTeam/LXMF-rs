use rns_transport::hash::AddressHash;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct DirectBackchannelLinks {
    outbound_local_sources: Arc<Mutex<HashMap<AddressHash, [u8; 16]>>>,
}

impl DirectBackchannelLinks {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn record_outbound_local_source(&self, link_id: AddressHash, source: [u8; 16]) {
        match self.outbound_local_sources.lock() {
            Ok(mut guard) => {
                guard.insert(link_id, source);
            }
            Err(error) => {
                log::error!(
                    "[daemon] outbound local-source cache lock poisoned link={link_id}: {error}"
                );
            }
        }
    }

    pub(super) fn outbound_local_source(&self, link_id: &AddressHash) -> Option<[u8; 16]> {
        match self.outbound_local_sources.lock() {
            Ok(guard) => guard.get(link_id).copied(),
            Err(error) => {
                log::error!(
                    "[daemon] outbound local-source cache lock poisoned link={link_id}: {error}"
                );
                None
            }
        }
    }
}

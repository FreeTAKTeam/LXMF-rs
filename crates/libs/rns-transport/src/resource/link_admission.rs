//! Per-Link inbound Resource admission (issue #766).
//!
//! The global caps in `advertisement_limits.rs` protect the node. An
//! application protocol such as RPP also needs a per-Link budget so a peer
//! cannot start more, or larger, inbound assemblies than it negotiated.
//! The budget is checked from the advertisement alone, before any receiver
//! state or metadata exists.

use super::{ResourceAdvertisement, ResourceManager};
use crate::hash::AddressHash;

/// Inbound budget for unsolicited (non-response) Resources on one Link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkResourceAdmission {
    /// Largest advertised logical size, metadata included, for one Resource.
    pub max_data_size: u64,
    /// Most inbound assemblies that may be in progress at once on the Link.
    pub max_concurrent: usize,
}

impl ResourceManager {
    pub fn set_link_admission(
        &mut self,
        link_id: AddressHash,
        admission: Option<LinkResourceAdmission>,
    ) {
        match admission {
            Some(admission) => self.link_admission.insert(link_id, admission),
            None => self.link_admission.remove(&link_id),
        };
    }

    /// Returns `true` when a new (non-continuation) advertisement exceeds the
    /// Link's budget and must be refused.
    pub(super) fn advertisement_exceeds_link_admission(
        &self,
        advertisement: &ResourceAdvertisement,
        link_id: &AddressHash,
    ) -> bool {
        let Some(admission) = self.link_admission.get(link_id) else {
            return false;
        };
        // A later segment of an admitted split Resource was charged already.
        if advertisement.is_response()
            || self.incoming_segments.contains_key(&advertisement.original_hash)
        {
            return false;
        }
        if advertisement.data_size > admission.max_data_size {
            log::warn!(
                "rejecting resource advertisement over link admission size link={} hash={} data_size={} limit={}",
                link_id,
                advertisement.hash,
                advertisement.data_size,
                admission.max_data_size
            );
            return true;
        }
        // Count logical Resources: a split Resource has both an assembly and
        // the receiver of its current segment.
        let active = self
            .incoming
            .values()
            .filter(|receiver| receiver.link_id == *link_id && receiver.is_active())
            .map(|receiver| receiver.original_hash)
            .chain(
                self.incoming_segments
                    .iter()
                    .filter(|(_, assembly)| assembly.link_id == *link_id)
                    .map(|(hash, _)| *hash),
            )
            .collect::<std::collections::HashSet<_>>()
            .len();
        if active >= admission.max_concurrent {
            log::warn!(
                "rejecting resource advertisement over link admission concurrency link={} hash={} active={} limit={}",
                link_id,
                advertisement.hash,
                active,
                admission.max_concurrent
            );
            return true;
        }
        false
    }
}

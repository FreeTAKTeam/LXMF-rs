use std::collections::HashMap;
use tokio::time::{Duration, Instant};

use crate::destination::link::LinkId;
use crate::hash::AddressHash;
use crate::packet::Packet;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum LinkProofValidationPolicy {
    DestinationIdentity,
    SharedOwnerHandoff,
}

#[allow(dead_code)]
pub struct LinkEntry {
    pub timestamp: Instant,
    pub proof_timeout: Instant,
    pub next_hop: AddressHash,
    pub next_hop_iface: AddressHash,
    pub received_from: AddressHash,
    pub original_destination: AddressHash,
    pub taken_hops: u8,
    pub remaining_hops: u8,
    pub validated: bool,
    proof_validation_policy: LinkProofValidationPolicy,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ExpiredUnvalidatedLink {
    pub received_from: AddressHash,
    pub original_destination: AddressHash,
    pub taken_hops: u8,
}

fn send_backwards(packet: &Packet, entry: &LinkEntry) -> (Packet, AddressHash) {
    let propagated = Packet {
        header: packet.header,
        ifac: None,
        destination: packet.destination,
        transport: packet.transport,
        context: packet.context,
        data: packet.data.clone(),
    };

    (propagated, entry.received_from)
}

pub struct LinkTable {
    entries: HashMap<LinkId, LinkEntry>,
    proof_timeout: Duration,
    idle_timeout: Duration,
}

impl LinkTable {
    pub fn new(proof_timeout: Duration, idle_timeout: Duration) -> Self {
        Self { entries: HashMap::new(), proof_timeout, idle_timeout }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn active_len(&self) -> usize {
        self.entries.values().filter(|entry| entry.validated).count()
    }

    pub fn add(
        &mut self,
        link_request: &Packet,
        destination: AddressHash,
        received_from: AddressHash,
        next_hop: AddressHash,
        iface: AddressHash,
    ) {
        self.add_with_proof_validation_policy(
            link_request,
            destination,
            received_from,
            next_hop,
            iface,
            LinkProofValidationPolicy::DestinationIdentity,
        );
    }

    /// Records a LinkRequest handed to the unique configured shared owner.
    /// That owner may return a proof for a destination whose identity this
    /// relay cannot recall locally.
    pub fn add_shared_owner_handoff(
        &mut self,
        link_request: &Packet,
        destination: AddressHash,
        received_from: AddressHash,
        next_hop: AddressHash,
        iface: AddressHash,
    ) {
        self.add_with_proof_validation_policy(
            link_request,
            destination,
            received_from,
            next_hop,
            iface,
            LinkProofValidationPolicy::SharedOwnerHandoff,
        );
    }

    fn add_with_proof_validation_policy(
        &mut self,
        link_request: &Packet,
        destination: AddressHash,
        received_from: AddressHash,
        next_hop: AddressHash,
        iface: AddressHash,
        proof_validation_policy: LinkProofValidationPolicy,
    ) {
        let link_id = LinkId::from(link_request);

        if self.entries.contains_key(&link_id) {
            return;
        }

        let now = Instant::now();
        let taken_hops = link_request.header.hops;

        let entry = LinkEntry {
            timestamp: now,
            proof_timeout: now + self.proof_timeout,
            next_hop,
            next_hop_iface: iface,
            received_from,
            original_destination: destination,
            taken_hops,
            remaining_hops: 0,
            validated: false,
            proof_validation_policy,
        };

        self.entries.insert(link_id, entry);
    }

    /// Whether the table has any entry (validated or pending) for `link_id`.
    /// Unlike `original_destination`, this does not filter on `validated`, so it
    /// also recognizes link requests still awaiting their proof.
    pub fn knows(&self, link_id: &LinkId) -> bool {
        self.entries.contains_key(link_id)
    }

    pub fn original_destination(&self, link_id: &LinkId) -> Option<AddressHash> {
        self.entries.get(link_id).filter(|e| e.validated).map(|e| e.original_destination)
    }

    pub fn proof_validation_context(&self, link_id: &LinkId) -> Option<(AddressHash, AddressHash)> {
        self.entries.get(link_id).map(|entry| (entry.original_destination, entry.next_hop_iface))
    }

    /// Whether `link_id` was sent to the recorded shared owner interface.
    /// This does not authorize ordinary transit LinkRequest proofs.
    pub fn allows_shared_owner_proof_on_iface(&self, link_id: &LinkId, iface: AddressHash) -> bool {
        self.entries.get(link_id).is_some_and(|entry| {
            entry.proof_validation_policy == LinkProofValidationPolicy::SharedOwnerHandoff
                && entry.next_hop_iface == iface
        })
    }

    pub fn handle_keepalive(&mut self, packet: &Packet) -> Option<(Packet, AddressHash)> {
        if let Some(entry) = self.entries.get_mut(&packet.destination) {
            entry.timestamp = Instant::now();
            return Some(send_backwards(packet, entry));
        }
        None
    }

    pub fn handle_proof(&mut self, proof: &Packet) -> Option<(Packet, AddressHash)> {
        match self.entries.get_mut(&proof.destination) {
            Some(entry) => {
                entry.remaining_hops = proof.header.hops;
                entry.validated = true;
                entry.timestamp = Instant::now();

                Some(send_backwards(proof, entry))
            }
            None => None,
        }
    }

    pub fn handle_reverse_link_packet(
        &mut self,
        packet: &Packet,
        received_on: AddressHash,
    ) -> Option<(Packet, AddressHash)> {
        let entry = self.entries.get_mut(&packet.destination)?;
        if !entry.validated || received_on != entry.next_hop_iface {
            return None;
        }
        entry.timestamp = Instant::now();
        Some(send_backwards(packet, entry))
    }

    pub fn remove_stale(&mut self) -> Vec<ExpiredUnvalidatedLink> {
        let mut stale = vec![];
        let mut expired_unvalidated = vec![];
        let now = Instant::now();

        for (link_id, entry) in &self.entries {
            if entry.validated {
                if entry.timestamp + self.idle_timeout <= now {
                    stale.push(*link_id);
                }
            } else if entry.proof_timeout <= now {
                expired_unvalidated.push(ExpiredUnvalidatedLink {
                    received_from: entry.received_from,
                    original_destination: entry.original_destination,
                    taken_hops: entry.taken_hops,
                });
                stale.push(*link_id);
            }
        }

        for link_id in stale {
            self.entries.remove(&link_id);
        }

        expired_unvalidated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_table_len_reports_empty_table() {
        let table = LinkTable::new(Duration::from_secs(5), Duration::from_secs(30));

        assert_eq!(table.len(), 0);
    }
}

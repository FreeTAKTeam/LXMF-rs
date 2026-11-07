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
    pub remaining_hops: Option<u8>,
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

    pub(super) fn destinations(&self) -> impl Iterator<Item = AddressHash> + '_ {
        self.entries.values().map(|entry| entry.original_destination)
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
        remaining_hops: u8,
    ) {
        self.add_with_proof_validation_policy(
            link_request,
            destination,
            received_from,
            next_hop,
            iface,
            (Some(remaining_hops), LinkProofValidationPolicy::DestinationIdentity),
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
            (None, LinkProofValidationPolicy::SharedOwnerHandoff),
        );
    }

    fn add_with_proof_validation_policy(
        &mut self,
        link_request: &Packet,
        destination: AddressHash,
        received_from: AddressHash,
        next_hop: AddressHash,
        iface: AddressHash,
        proof_policy: (Option<u8>, LinkProofValidationPolicy),
    ) {
        let (remaining_hops, proof_validation_policy) = proof_policy;
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
            remaining_hops,
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

    #[cfg(test)]
    pub fn original_destination(&self, link_id: &LinkId) -> Option<AddressHash> {
        self.entries.get(link_id).filter(|e| e.validated).map(|e| e.original_destination)
    }

    pub fn proof_validation_context(&self, link_id: &LinkId) -> Option<(AddressHash, AddressHash)> {
        self.entries.get(link_id).map(|entry| (entry.original_destination, entry.next_hop_iface))
    }

    pub(super) fn relay_interfaces(&self, link_id: &LinkId) -> Option<(AddressHash, AddressHash)> {
        self.entries.get(link_id).map(|entry| (entry.received_from, entry.next_hop_iface))
    }

    pub(super) fn remaining_hops(&self, link_id: &LinkId) -> Option<u8> {
        self.entries.get(link_id).and_then(|entry| entry.remaining_hops)
    }

    /// Whether `link_id` was sent to the recorded shared owner interface.
    /// This does not authorize ordinary transit LinkRequest proofs.
    pub fn allows_shared_owner_proof_on_iface(&self, link_id: &LinkId, iface: AddressHash) -> bool {
        self.entries.get(link_id).is_some_and(|entry| {
            entry.proof_validation_policy == LinkProofValidationPolicy::SharedOwnerHandoff
                && entry.next_hop_iface == iface
        })
    }

    /// Caller has validated the destination signature or the explicit trusted
    /// shared-owner handoff, and checked the recorded next-hop interface.
    pub fn handle_proof(&mut self, proof: &Packet) -> Option<(Packet, AddressHash)> {
        match self.entries.get_mut(&proof.destination) {
            Some(entry) => {
                if entry.validated && entry.remaining_hops != Some(proof.header.hops) {
                    return None;
                }
                // An identityless shared handoff starts with unknown distance;
                // its first trusted-owner observation binds it, without claiming
                // signed path rebalancing or manufacturing a destination path.
                entry.remaining_hops = Some(proof.header.hops);
                entry.validated = true;
                entry.timestamp = Instant::now();

                Some(send_backwards(proof, entry))
            }
            None => None,
        }
    }

    pub fn handle_link_packet(
        &mut self,
        packet: &Packet,
        received_on: AddressHash,
    ) -> Option<(Packet, AddressHash)> {
        // Shared-owner handoff can establish a Link without a destination path entry.
        let entry = self.entries.get_mut(&packet.destination)?;
        if !entry.validated {
            return None;
        }
        let remaining_hops = entry.remaining_hops?;
        let hops = packet.header.hops;
        let target = if entry.next_hop_iface == entry.received_from {
            if received_on != entry.next_hop_iface
                || (hops != remaining_hops && hops != entry.taken_hops)
            {
                return None;
            }
            entry.next_hop_iface
        } else if received_on == entry.next_hop_iface && hops == remaining_hops {
            entry.received_from
        } else if received_on == entry.received_from && hops == entry.taken_hops {
            entry.next_hop_iface
        } else {
            return None;
        };
        entry.timestamp = Instant::now();
        let mut forwarded = packet.clone();
        forwarded.ifac = None;
        Some((forwarded, target))
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
    use crate::packet::{DestinationType, PacketContext, PacketType};

    #[test]
    fn link_table_len_reports_empty_table() {
        let table = LinkTable::new(Duration::from_secs(5), Duration::from_secs(30));

        assert_eq!(table.len(), 0);
    }

    #[test]
    fn validated_link_direction_uses_both_hop_counts_and_same_interface_guard() {
        for same_interface in [false, true] {
            let requester = AddressHash::new_from_slice(&[1; 16]);
            let responder =
                if same_interface { requester } else { AddressHash::new_from_slice(&[2; 16]) };
            let unrelated = AddressHash::new_from_slice(&[3; 16]);
            let mut request = Packet::default();
            request.header.packet_type = PacketType::LinkRequest;
            request.header.hops = 2;
            let id = LinkId::from(&request);
            let mut table = LinkTable::new(Duration::from_secs(5), Duration::from_secs(30));
            table.add(&request, request.destination, requester, request.destination, responder, 4);
            let mut packet = Packet { destination: id, ..Packet::default() };
            packet.header.destination_type = DestinationType::Link;
            packet.header.packet_type = PacketType::Data;
            packet.header.hops = 4;
            assert!(
                table.handle_link_packet(&packet, responder).is_none(),
                "pending rejects traffic"
            );
            let mut proof = packet.clone();
            proof.header.packet_type = PacketType::Proof;
            proof.context = PacketContext::LinkRequestProof;
            assert!(table.handle_proof(&proof).is_some());
            for context in [
                PacketContext::None,
                PacketContext::KeepAlive,
                PacketContext::ResourceRequest,
                PacketContext::ResourceProof,
            ] {
                packet.context = context;
                for received_on in [requester, responder, unrelated] {
                    for hops in [0, 1, 2, 3, 4, 5] {
                        packet.header.hops = hops;
                        let expected = if same_interface {
                            (received_on == requester && matches!(hops, 2 | 4)).then_some(requester)
                        } else if received_on == responder && hops == 4 {
                            Some(requester)
                        } else if received_on == requester && hops == 2 {
                            Some(responder)
                        } else {
                            None
                        };
                        let before = table.entries[&id].timestamp;
                        assert_eq!(
                            table
                                .handle_link_packet(&packet, received_on)
                                .map(|(_, target)| target),
                            expected
                        );
                        if expected.is_none() {
                            assert_eq!(
                                table.entries[&id].timestamp, before,
                                "rejection cannot refresh lifetime"
                            );
                        }
                    }
                }
            }
        }
    }
}

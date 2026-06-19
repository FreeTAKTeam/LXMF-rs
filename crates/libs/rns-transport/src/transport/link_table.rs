use std::collections::HashMap;
use tokio::time::{Duration, Instant};

use crate::destination::link::LinkId;
use crate::hash::AddressHash;
use crate::packet::Packet;

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

    pub fn add(
        &mut self,
        link_request: &Packet,
        destination: AddressHash,
        received_from: AddressHash,
        next_hop: AddressHash,
        iface: AddressHash,
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
        };

        self.entries.insert(link_id, entry);
    }

    pub fn original_destination(&self, link_id: &LinkId) -> Option<AddressHash> {
        self.entries.get(link_id).filter(|e| e.validated).map(|e| e.original_destination)
    }

    pub fn proof_validation_context(&self, link_id: &LinkId) -> Option<(AddressHash, AddressHash)> {
        self.entries.get(link_id).map(|entry| (entry.original_destination, entry.next_hop_iface))
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

    pub fn remove_stale(&mut self) {
        let mut stale = vec![];
        let now = Instant::now();

        for (link_id, entry) in &self.entries {
            if entry.validated {
                if entry.timestamp + self.idle_timeout <= now {
                    stale.push(*link_id);
                }
            } else if entry.proof_timeout <= now {
                stale.push(*link_id);
            }
        }

        for link_id in stale {
            self.entries.remove(&link_id);
        }
    }
}

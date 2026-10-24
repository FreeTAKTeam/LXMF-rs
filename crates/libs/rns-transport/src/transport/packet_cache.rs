use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use crate::{hash::AddressHash, hash::Hash, packet::Packet, packet::PacketType};

pub struct PacketTrack {
    pub time: Instant,
    pub min_hops: u8,
    pub source_iface: Option<AddressHash>,
    pub destination: AddressHash,
}

#[cfg(test)]
#[path = "tests/packet_cache_generations.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/duplicate_filter_reference.rs"]
mod reference_tests;

pub struct PacketCache {
    pub(super) link_receipts: super::link_packet_receipt::LinkReceiptRegistry,
    current: HashSet<Hash>,
    previous: HashSet<Hash>,
    map: HashMap<Hash, PacketTrack>,
    by_proof_destination: HashMap<AddressHash, Hash>,
    remove_cache: Vec<Hash>,
}

impl PacketCache {
    pub fn new() -> Self {
        Self {
            link_receipts: Default::default(),
            current: HashSet::new(),
            previous: HashSet::new(),
            map: HashMap::new(),
            by_proof_destination: HashMap::new(),
            remove_cache: Vec::new(),
        }
    }

    pub fn release(&mut self, duration: Duration) {
        self.link_receipts.release();
        for entry in &self.map {
            if entry.1.time.elapsed() > duration {
                self.remove_cache.push(*entry.0);
            }
        }

        for hash in &self.remove_cache {
            if self.map.remove(hash).is_some() {
                self.by_proof_destination.remove(&AddressHash::new_from_hash(hash));
            }
        }

        self.remove_cache.clear();
    }

    #[cfg(test)]
    pub fn update(&mut self, packet: &Packet) -> bool {
        let hash = packet.hash();
        let is_new = !self.contains_hash(&hash);
        self.remember_hash(hash);
        self.track_packet(packet);
        is_new
    }

    pub(super) fn contains_hash(&self, hash: &Hash) -> bool {
        self.current.contains(hash) || self.previous.contains(hash)
    }

    pub(super) fn remember_hash(&mut self, hash: Hash) {
        self.current.insert(hash);
    }

    pub(super) fn forget_hash(&mut self, hash: &Hash) {
        self.current.remove(hash);
        self.previous.remove(hash);
    }

    /// Frozen RNS maintenance rotates only when the current generation exceeds
    /// half the one-million hash budget. Proof/source context has its own TTL.
    /// Packet hashes held across both duplicate-filter generations.
    pub fn hash_count(&self) -> usize {
        self.current.len() + self.previous.len()
    }

    /// Packets tracked for receipts, proofs and retransmission bookkeeping.
    pub fn tracked_count(&self) -> usize {
        self.map.len() + self.by_proof_destination.len()
    }

    pub(super) fn maintain_duplicate_generations(&mut self) -> bool {
        const HASHLIST_MAXSIZE: usize = 1_000_000;
        if self.current.len() <= HASHLIST_MAXSIZE / 2 {
            return false;
        }
        self.previous = std::mem::take(&mut self.current);
        true
    }

    pub(super) fn track_packet(&mut self, packet: &Packet) {
        let hash = packet.hash();

        // Register outbound Data before handoff as well as recording received
        // Data through note_source. wire_receipt::validated_receipt_hash requires
        // this exact packet/recipient binding for explicit and implicit proofs;
        // duplicate membership alone carries no recipient or source authority.
        if packet.header.packet_type == PacketType::Data {
            self.by_proof_destination.insert(AddressHash::new_from_hash(&hash), hash);
        }

        let track = self.map.get_mut(&hash);
        if let Some(track) = track {
            track.time = Instant::now();
            track.min_hops = min(packet.header.hops, track.min_hops);
        } else {
            self.map.insert(
                hash,
                PacketTrack {
                    time: Instant::now(),
                    min_hops: packet.header.hops,
                    source_iface: None,
                    destination: packet.destination,
                },
            );
        }
    }

    pub fn restore_hashes(&mut self, hashes: impl IntoIterator<Item = Hash>) -> usize {
        let mut restored = 0;
        for hash in hashes {
            if self.current.insert(hash) {
                restored += 1;
            }
        }
        restored
    }

    pub fn note_source(&mut self, packet: &Packet, iface: AddressHash) {
        let hash = packet.hash();
        let now = Instant::now();
        self.by_proof_destination.insert(AddressHash::new_from_hash(&hash), hash);

        if let Some(track) = self.map.get_mut(&hash) {
            if packet.header.hops <= track.min_hops || track.source_iface.is_none() {
                track.source_iface = Some(iface);
                track.destination = packet.destination;
            }
            track.time = now;
            track.min_hops = min(packet.header.hops, track.min_hops);
        } else {
            self.map.insert(
                hash,
                PacketTrack {
                    time: now,
                    min_hops: packet.header.hops,
                    source_iface: Some(iface),
                    destination: packet.destination,
                },
            );
        }
    }

    pub fn source_iface_for_hash(&self, hash: &Hash) -> Option<AddressHash> {
        self.map.get(hash).and_then(|track| track.source_iface)
    }

    pub fn source_iface_for_proof_destination(
        &self,
        destination: &AddressHash,
    ) -> Option<(Hash, AddressHash)> {
        let hash = self.by_proof_destination.get(destination)?;
        let source_iface = self.source_iface_for_hash(hash)?;
        Some((*hash, source_iface))
    }

    pub fn proof_context_for_destination(
        &self,
        destination: &AddressHash,
    ) -> Option<(Hash, AddressHash, Option<AddressHash>)> {
        let hash = *self.by_proof_destination.get(destination)?;
        let track = self.map.get(&hash)?;
        Some((hash, track.destination, track.source_iface))
    }

    pub fn hashes(&self) -> Vec<Hash> {
        self.current.iter().copied().collect()
    }
}

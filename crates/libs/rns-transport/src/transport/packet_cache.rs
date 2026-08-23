use std::{
    cmp::min,
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{hash::AddressHash, hash::Hash, packet::Packet, packet::PacketType};

pub struct PacketTrack {
    pub time: Instant,
    pub min_hops: u8,
    pub source_iface: Option<AddressHash>,
    pub destination: AddressHash,
}

pub struct PacketCache {
    map: HashMap<Hash, PacketTrack>,
    by_proof_destination: HashMap<AddressHash, Hash>,
    remove_cache: Vec<Hash>,
}

impl PacketCache {
    pub fn new() -> Self {
        Self { map: HashMap::new(), by_proof_destination: HashMap::new(), remove_cache: Vec::new() }
    }

    pub fn release(&mut self, duration: Duration) {
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

    pub fn update(&mut self, packet: &Packet) -> bool {
        let hash = packet.hash();

        // Track outbound Data packets by their would-be proof destination too,
        // not just ones we've seen inbound via `note_source`. Without this, a
        // packet we originate ourselves (never passed to `note_source`, which
        // only runs for received Data packets) has no cache entry, and an
        // explicit delivery proof for it falls through to the fallback in
        // `validate_destination_receipt_proof` callers that trusted any known
        // identity's signature — letting a peer forge a receipt for a packet
        // addressed to someone else. See `wire.rs::validated_receipt_hash`.
        if packet.header.packet_type == PacketType::Data {
            self.by_proof_destination.insert(AddressHash::new_from_hash(&hash), hash);
        }

        let mut is_new_packet = false;

        let track = self.map.get_mut(&hash);
        if let Some(track) = track {
            track.time = Instant::now();
            track.min_hops = min(packet.header.hops, track.min_hops);
        } else {
            is_new_packet = true;

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

        is_new_packet
    }

    pub fn remove(&mut self, hash: &Hash) {
        if self.map.remove(hash).is_some() {
            self.by_proof_destination.remove(&AddressHash::new_from_hash(hash));
        }
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
        self.map.keys().copied().collect()
    }
}

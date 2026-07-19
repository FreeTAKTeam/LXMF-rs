use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use rand_core::{OsRng, RngCore};
use tokio::time::{Duration, Instant};

use crate::hash::AddressHash;
use crate::iface::{TxMessage, TxMessageType};
use crate::packet::{Header, HeaderType, Packet, PacketContext, PropagationType};

const PATHFINDER_RETRY_GRACE: Duration = Duration::from_secs(5);
const PATHFINDER_RETRY_WINDOW: Duration = Duration::from_millis(500);
const PATH_RESPONSE_GRACE: Duration = Duration::from_millis(400);
pub(super) const PATH_RESPONSE_ROAMING_GRACE: Duration = Duration::from_millis(1500);

#[derive(Clone)]
pub struct AnnounceEntry {
    pub packet: Packet,
    pub timeout: Instant,
    pub received_from: AddressHash,
    pub retries: u8,
    pub hops: u8,
    pub response_to_iface: Option<AddressHash>,
}

impl AnnounceEntry {
    pub fn retransmit(&mut self, transport_id: &AddressHash) -> Option<TxMessage> {
        if Instant::now() < self.timeout {
            return None;
        }

        self.retries = self.retries.saturating_add(1);
        self.timeout = Instant::now() + PATHFINDER_RETRY_GRACE + retry_window();

        let packet = Packet {
            header: Header {
                ifac_flag: self.packet.header.ifac_flag,
                header_type: HeaderType::Type2,
                context_flag: self.packet.header.context_flag,
                propagation_type: PropagationType::Broadcast,
                destination_type: self.packet.header.destination_type,
                packet_type: self.packet.header.packet_type,
                hops: self.hops,
            },
            ifac: None,
            destination: self.packet.destination,
            transport: Some(*transport_id),
            context: self.packet.context,
            data: self.packet.data.clone(),
        };

        let tx_type = match self.response_to_iface {
            Some(iface) => TxMessageType::Direct(iface),
            None => TxMessageType::Broadcast(Some(self.received_from)),
        };

        Some(TxMessage { tx_type, packet })
    }
}

fn retry_window() -> Duration {
    let window_ms = PATHFINDER_RETRY_WINDOW.as_millis() as u64;
    let mut rng = OsRng;
    Duration::from_millis(u64::from(rng.next_u32()) % (window_ms + 1))
}

pub struct AnnounceCache {
    newer: Option<BTreeMap<AddressHash, AnnounceEntry>>,
    older: Option<BTreeMap<AddressHash, AnnounceEntry>>,
    capacity: usize,
}

impl AnnounceCache {
    pub fn new(capacity: usize) -> Self {
        Self { newer: Some(BTreeMap::new()), older: None, capacity }
    }

    pub fn insert(&mut self, destination: AddressHash, entry: AnnounceEntry) {
        if self.capacity > 0 && self.len() >= self.capacity {
            self.evict_one();
        }

        if self.capacity > 0
            && self.newer.as_ref().is_some_and(|newer| newer.len() >= self.capacity)
        {
            self.older = self.newer.take();
            self.newer = Some(BTreeMap::new());
        }

        self.newer.get_or_insert_with(BTreeMap::new).insert(destination, entry);
    }

    fn get(&self, destination: &AddressHash) -> Option<AnnounceEntry> {
        if let Some(entry) = self.newer.as_ref().and_then(|newer| newer.get(destination)) {
            return Some(entry.clone());
        }

        if let Some(ref older) = self.older {
            return older.get(destination).cloned();
        }

        None
    }

    pub fn len(&self) -> usize {
        let newer_len = self.newer.as_ref().map(|m| m.len()).unwrap_or(0);
        let older_len = self.older.as_ref().map(|m| m.len()).unwrap_or(0);
        newer_len + older_len
    }

    fn evict_one(&mut self) {
        if let Some(ref mut older) = self.older {
            if let Some(first_key) = older.keys().next().cloned() {
                older.remove(&first_key);
                if older.is_empty() {
                    self.older = None;
                }
                return;
            }
        }

        if let Some(ref mut newer) = self.newer {
            if let Some(first_key) = newer.keys().next().cloned() {
                newer.remove(&first_key);
            }
        }
    }
}

pub struct AnnounceTable {
    map: BTreeMap<AddressHash, AnnounceEntry>,
    responses: BTreeMap<AddressHash, AnnounceEntry>,
    cache: AnnounceCache,
    retry_limit: u8,
}

impl AnnounceTable {
    pub fn new(cache_capacity: usize, retry_limit: u8) -> Self {
        Self {
            map: BTreeMap::new(),
            responses: BTreeMap::new(),
            cache: AnnounceCache::new(cache_capacity),
            retry_limit,
        }
    }

    pub fn add(&mut self, announce: &Packet, destination: AddressHash, received_from: AddressHash) {
        let now = Instant::now();
        let hops = announce.header.hops;

        let entry = AnnounceEntry {
            packet: announce.clone(),
            timeout: now + retry_window(),
            received_from,
            retries: 0,
            hops,
            response_to_iface: None,
        };

        self.map.insert(destination, entry);
    }

    pub(crate) fn add_cached(
        &mut self,
        announce: &Packet,
        destination: AddressHash,
        received_from: AddressHash,
    ) {
        let entry = AnnounceEntry {
            packet: announce.clone(),
            timeout: Instant::now(),
            received_from,
            retries: 0,
            hops: announce.header.hops,
            response_to_iface: None,
        };

        self.cache.insert(destination, entry);
    }

    fn do_add_response(
        &mut self,
        mut response: AnnounceEntry,
        destination: AddressHash,
        to_iface: AddressHash,
        hops: u8,
        extra_grace: Duration,
    ) {
        response.retries = self.retry_limit;
        response.hops = hops;
        response.timeout = Instant::now() + PATH_RESPONSE_GRACE + extra_grace;
        response.response_to_iface = Some(to_iface);
        response.packet.context = PacketContext::PathResponse;

        self.responses.insert(destination, response);
    }

    pub fn add_response(
        &mut self,
        destination: AddressHash,
        to_iface: AddressHash,
        hops: u8,
    ) -> bool {
        self.add_response_with_extra_grace(destination, to_iface, hops, Duration::ZERO)
    }

    pub fn add_response_with_extra_grace(
        &mut self,
        destination: AddressHash,
        to_iface: AddressHash,
        hops: u8,
        extra_grace: Duration,
    ) -> bool {
        if let Some(entry) = self.map.get(&destination) {
            self.do_add_response(entry.clone(), destination, to_iface, hops, extra_grace);
            return true;
        }

        if let Some(entry) = self.cache.get(&destination) {
            self.do_add_response(entry, destination, to_iface, hops, extra_grace);
            return true;
        }

        false
    }

    #[cfg(test)]
    pub fn packet_for_destination(&self, destination: &AddressHash) -> Option<Packet> {
        self.map
            .get(destination)
            .map(|entry| entry.packet.clone())
            .or_else(|| self.cache.get(destination).map(|entry| entry.packet))
            .or_else(|| self.responses.get(destination).map(|entry| entry.packet.clone()))
    }

    pub fn cached_packet_for_destination(&self, destination: &AddressHash) -> Option<Packet> {
        self.map
            .get(destination)
            .map(|entry| entry.packet.clone())
            .or_else(|| self.cache.get(destination).map(|entry| entry.packet))
    }

    pub fn observe_passed_rebroadcast(
        &mut self,
        destination: &AddressHash,
        observed_hops: u8,
    ) -> bool {
        let Some(entry) = self.map.get(destination) else {
            return false;
        };
        if entry.retries == 0 || Instant::now() >= entry.timeout {
            return false;
        }
        if Some(observed_hops) != entry.hops.checked_add(1) {
            return false;
        }

        if let Some(announce) = self.map.remove(destination) {
            self.cache.insert(*destination, announce);
            return true;
        }
        false
    }

    #[cfg(test)]
    pub(crate) fn pending_response_for_destination(
        &self,
        destination: &AddressHash,
    ) -> Option<&AnnounceEntry> {
        self.responses.get(destination)
    }

    pub fn drain_retransmissions(&mut self, transport_id: &AddressHash) -> Vec<TxMessage> {
        let mut messages = vec![];
        let mut completed = vec![];
        let mut completed_responses = vec![];
        let now = Instant::now();

        for (destination, ref mut entry) in &mut self.map {
            if self.responses.contains_key(destination) {
                continue;
            }

            if entry.retries > self.retry_limit {
                completed.push(*destination);
                continue;
            }

            if now < entry.timeout {
                continue;
            }

            if let Some(message) = entry.retransmit(transport_id) {
                messages.push(message);
                if entry.retries > self.retry_limit {
                    completed.push(*destination);
                }
            }
        }

        let n_announces = messages.len();

        for (destination, ref mut entry) in &mut self.responses {
            if entry.retries > self.retry_limit {
                completed_responses.push(*destination);
                continue;
            }
            if now < entry.timeout {
                continue;
            }
            if let Some(message) = entry.retransmit(transport_id) {
                messages.push(message);
                if entry.retries > self.retry_limit {
                    completed_responses.push(*destination);
                }
            }
        }

        let n_responses = messages.len() - n_announces;

        if !(messages.is_empty() && completed.is_empty()) {
            log::trace!(
                "Announce cache: {} retransmitted, {} path responses, {} dropped",
                n_announces,
                n_responses,
                completed.len(),
            );
        }

        for destination in completed {
            if let Some(announce) = self.map.remove(&destination) {
                self.cache.insert(destination, announce);
            }
        }

        for destination in completed_responses {
            self.responses.remove(&destination);
        }

        messages
    }
}

impl Default for AnnounceTable {
    fn default() -> Self {
        Self::new(100_000, 1)
    }
}

#[cfg(test)]
mod tests {
    include!("announce_table_tests.rs");
}

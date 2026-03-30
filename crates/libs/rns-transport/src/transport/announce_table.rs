use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use rand_core::{OsRng, RngCore};
use tokio::time::{Duration, Instant};

use crate::hash::AddressHash;
use crate::iface::{TxMessage, TxMessageType};
use crate::packet::{Header, HeaderType, Packet, PropagationType};

const PATHFINDER_RETRY_GRACE: Duration = Duration::from_secs(5);
const PATHFINDER_RETRY_WINDOW: Duration = Duration::from_millis(500);
const PATH_RESPONSE_GRACE: Duration = Duration::from_millis(400);

#[derive(Clone)]
pub struct AnnounceEntry {
    pub packet: Packet,
    pub timestamp: Instant,
    pub timeout: Instant,
    pub received_from: AddressHash,
    pub retries: u8,
    pub hops: u8,
    pub response_to_iface: Option<AddressHash>,
}

impl AnnounceEntry {
    pub fn dummy() -> Self {
        let now = Instant::now();
        Self {
            packet: Packet::default(),
            timestamp: now,
            timeout: now,
            received_from: AddressHash::new_empty(),
            retries: 0,
            hops: 0,
            response_to_iface: None,
        }
    }

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
            data: self.packet.data,
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

        if self.newer.as_ref().unwrap().len() >= self.capacity {
            self.older = Some(self.newer.take().unwrap());
            self.newer = Some(BTreeMap::new());
        }

        self.newer.as_mut().unwrap().insert(destination, entry);
    }

    fn get(&self, destination: &AddressHash) -> Option<AnnounceEntry> {
        if let Some(entry) = self.newer.as_ref().unwrap().get(destination) {
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

    pub fn is_empty(&self) -> bool {
        self.len() == 0
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

    fn clear(&mut self) {
        self.newer.as_mut().unwrap().clear();
        self.older = None;
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

    pub fn is_empty(&self) -> bool {
        self.map.is_empty() && self.responses.is_empty() && self.cache.is_empty()
    }

    pub fn add(&mut self, announce: &Packet, destination: AddressHash, received_from: AddressHash) {
        if self.map.contains_key(&destination) {
            return;
        }

        let now = Instant::now();
        let hops = announce.header.hops;

        let entry = AnnounceEntry {
            packet: *announce,
            timestamp: now,
            timeout: now + retry_window(),
            received_from,
            retries: 0,
            hops,
            response_to_iface: None,
        };

        self.map.insert(destination, entry);
    }

    fn do_add_response(
        &mut self,
        mut response: AnnounceEntry,
        destination: AddressHash,
        to_iface: AddressHash,
        hops: u8,
    ) {
        response.retries = self.retry_limit;
        response.hops = hops;
        response.timeout = Instant::now() + PATH_RESPONSE_GRACE;
        response.response_to_iface = Some(to_iface);

        self.responses.insert(destination, response);
    }

    pub fn add_response(
        &mut self,
        destination: AddressHash,
        to_iface: AddressHash,
        hops: u8,
    ) -> bool {
        if let Some(entry) = self.map.get(&destination) {
            self.do_add_response(entry.clone(), destination, to_iface, hops);
            return true;
        }

        if let Some(entry) = self.cache.get(&destination) {
            self.do_add_response(entry, destination, to_iface, hops);
            return true;
        }

        false
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.responses.clear();
        self.cache.clear();
    }

    pub fn new_packet(
        &mut self,
        dest_hash: &AddressHash,
        transport_id: &AddressHash,
    ) -> Option<TxMessage> {
        let message = {
            let entry = self.map.get_mut(dest_hash)?;
            if entry.retries > self.retry_limit {
                None
            } else {
                entry.retransmit(transport_id)
            }
        };

        let should_cache =
            self.map.get(dest_hash).map(|entry| entry.retries > self.retry_limit).unwrap_or(false);

        if should_cache {
            if let Some(announce) = self.map.remove(dest_hash) {
                self.cache.insert(*dest_hash, announce);
            }
        }

        message
    }

    pub fn to_retransmit(&mut self, transport_id: &AddressHash) -> Vec<TxMessage> {
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
    use super::*;
    use crate::packet::PacketContext;
    use rand_core::OsRng;
    use std::thread::sleep;
    use std::time::Duration as StdDuration;

    #[test]
    fn announce_entries_use_random_window_and_grace_retry() {
        let mut table = AnnounceTable::new(16, 1);
        let destination = AddressHash::new_from_rand(OsRng);
        let received_from = AddressHash::new_from_rand(OsRng);
        let transport_id = AddressHash::new_from_rand(OsRng);
        let packet = Packet { destination, ..Packet::default() };

        table.add(&packet, destination, received_from);
        let entry = table.map.get(&destination).expect("announce entry inserted");
        let initial_delay = entry
            .timeout
            .checked_duration_since(entry.timestamp)
            .expect("retry timeout is after insertion");
        assert!(
            initial_delay <= PATHFINDER_RETRY_WINDOW,
            "initial retry window should stay inside python's 0.5s jitter window"
        );
        assert_eq!(entry.retries, 0);

        table.map.get_mut(&destination).unwrap().timeout =
            Instant::now() - Duration::from_millis(1);

        let messages = table.to_retransmit(&transport_id);
        assert_eq!(messages.len(), 1, "first local rebroadcast should fire once");
        let entry = table.map.get(&destination).expect("entry stays live for grace retry");
        assert_eq!(entry.retries, 1);

        table.map.get_mut(&destination).unwrap().timeout =
            Instant::now() - Duration::from_millis(1);
        let messages = table.to_retransmit(&transport_id);
        assert_eq!(messages.len(), 1, "python keeps one extra grace retry");
        assert!(!table.map.contains_key(&destination));
        assert!(table.to_retransmit(&transport_id).is_empty());
    }

    #[test]
    fn path_response_entries_use_shorter_window_without_later_broadcast() {
        let mut table = AnnounceTable::new(16, 1);
        let destination = AddressHash::new_from_rand(OsRng);
        let received_from = AddressHash::new_from_rand(OsRng);
        let transport_id = AddressHash::new_from_rand(OsRng);
        let to_iface = AddressHash::new_from_rand(OsRng);
        let packet = Packet { destination, context: PacketContext::None, ..Packet::default() };

        table.add(&packet, destination, received_from);
        assert!(table.add_response(destination, to_iface, 3));
        assert!(
            table.map.contains_key(&destination),
            "live announce entry must stay available for later remote path requests"
        );
        assert!(table.to_retransmit(&transport_id).is_empty());
        assert_eq!(table.responses.len(), 1);
        let response = table.responses.get(&destination).expect("response entry inserted");
        let response_delay =
            response.timeout.checked_duration_since(Instant::now()).unwrap_or_default();
        assert!(
            response_delay <= PATH_RESPONSE_GRACE,
            "path responses should stay on the shorter direct-response grace window"
        );

        sleep(StdDuration::from_millis(450));

        let messages = table.to_retransmit(&transport_id);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].tx_type, TxMessageType::Direct(iface) if iface == to_iface));
        assert_eq!(messages[0].packet.context, PacketContext::None);
        assert!(table.responses.is_empty());
        assert!(table.map.contains_key(&destination));
        assert!(table.add_response(destination, to_iface, 4));

        sleep(StdDuration::from_millis(450));

        let messages = table.to_retransmit(&transport_id);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].tx_type, TxMessageType::Direct(iface) if iface == to_iface));
        assert_eq!(messages[0].packet.header.hops, 4);
        assert!(table.responses.is_empty());
        assert!(table.map.contains_key(&destination));
    }
}

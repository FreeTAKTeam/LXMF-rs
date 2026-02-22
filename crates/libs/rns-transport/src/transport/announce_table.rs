use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use tokio::time::{Duration, Instant};

use crate::hash::AddressHash;
use crate::iface::{TxMessage, TxMessageType};
use crate::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext, PacketType,
    PropagationType,
};

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
        if self.retries == 0 || Instant::now() >= self.timeout {
            return None;
        }

        self.retries = self.retries.saturating_sub(1);

        let context = if self.response_to_iface.is_some() {
            PacketContext::PathResponse
        } else {
            PacketContext::None
        };

        let packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type2,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Announce,
                hops: self.hops,
            },
            ifac: None,
            destination: self.packet.destination,
            transport: Some(*transport_id),
            context,
            data: self.packet.data,
        };

        let tx_type = match self.response_to_iface {
            Some(iface) => TxMessageType::Direct(iface),
            None => TxMessageType::Broadcast(Some(self.received_from)),
        };

        Some(TxMessage { tx_type, packet })
    }
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
        let hops = announce.header.hops + 1;

        let entry = AnnounceEntry {
            packet: *announce,
            timestamp: now,
            timeout: now + Duration::from_secs(60),
            received_from,
            retries: self.retry_limit,
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
        response.retries = 1;
        response.hops = hops;
        response.timeout = Instant::now() + Duration::from_secs(60);
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
            self.do_add_response(entry.clone(), destination, to_iface, hops);
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
        // temporary hack
        self.map.get_mut(dest_hash).and_then(|e| e.retransmit(transport_id))
    }

    pub fn to_retransmit(&mut self, transport_id: &AddressHash) -> Vec<TxMessage> {
        let mut messages = vec![];
        let mut completed = vec![];

        for (destination, ref mut entry) in &mut self.map {
            if self.responses.contains_key(destination) {
                continue;
            }

            if let Some(message) = entry.retransmit(transport_id) {
                messages.push(message);
            } else {
                completed.push(*destination);
            }
        }

        let n_announces = messages.len();

        for ref mut entry in self.responses.values_mut() {
            if let Some(message) = entry.retransmit(transport_id) {
                messages.push(message);
            }
        }

        let n_responses = messages.len() - n_announces;

        self.responses.clear(); // every response is only retransmitted once

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

        messages
    }
}

impl Default for AnnounceTable {
    fn default() -> Self {
        Self::new(100_000, 5)
    }
}

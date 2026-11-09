use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::hash::Hash;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PacketSignal {
    pub rssi: Option<f64>,
    pub snr: Option<f64>,
    pub q: Option<f64>,
}

/// Per-packet radio metadata by packet hash: the bearers that hear it record
/// it, the transport reads it. Python Reticulum's 512-entry local-client cache.
#[derive(Debug, Clone, Default)]
pub struct PacketSignalCache(Arc<Mutex<VecDeque<(Hash, PacketSignal)>>>);

impl PacketSignalCache {
    const CAPACITY: usize = 512;

    pub fn record(&self, packet_hash: Hash, signal: PacketSignal) {
        let mut entries = self.0.lock().expect("packet signal cache mutex poisoned");
        entries.push_back((packet_hash, signal));
        while entries.len() > Self::CAPACITY {
            entries.pop_front();
        }
    }

    #[must_use]
    pub fn get(&self, packet_hash: &Hash) -> Option<PacketSignal> {
        self.0
            .lock()
            .expect("packet signal cache mutex poisoned")
            .iter()
            .rev()
            .find(|(hash, _)| hash == packet_hash)
            .map(|(_, signal)| *signal)
    }

    pub fn clear(&self) {
        self.0.lock().expect("packet signal cache mutex poisoned").clear();
    }
}

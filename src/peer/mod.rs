use crate::error::LxmfError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    dest: [u8; 16],
    last_seen: Option<f64>,
    name: Option<String>,
    sync_backoff: u32,
    peering_key: Option<Vec<u8>>,
    handled: BTreeSet<Vec<u8>>,
    unhandled: BTreeSet<Vec<u8>>,
    queued_handled: Vec<Vec<u8>>,
    queued_unhandled: Vec<Vec<u8>>,
}

impl Peer {
    pub fn new(dest: [u8; 16]) -> Self {
        Self {
            dest,
            last_seen: None,
            name: None,
            sync_backoff: 0,
            peering_key: None,
            handled: BTreeSet::new(),
            unhandled: BTreeSet::new(),
            queued_handled: Vec::new(),
            queued_unhandled: Vec::new(),
        }
    }

    pub fn from_bytes(peer_bytes: &[u8]) -> Result<Self, LxmfError> {
        rmp_serde::from_slice(peer_bytes).map_err(|e| LxmfError::Decode(e.to_string()))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, LxmfError> {
        rmp_serde::to_vec(self).map_err(|e| LxmfError::Encode(e.to_string()))
    }

    pub fn mark_seen(&mut self, ts: f64) {
        self.last_seen = Some(ts);
    }

    pub fn last_seen(&self) -> Option<f64> {
        self.last_seen
    }

    pub fn dest(&self) -> [u8; 16] {
        self.dest
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = Some(name.into());
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn peering_key_ready(&self) -> bool {
        self.peering_key.is_some()
    }

    pub fn peering_key_value(&self) -> Option<Vec<u8>> {
        self.peering_key.clone()
    }

    pub fn generate_peering_key(&mut self) {
        let key = reticulum::hash::Hash::new_from_slice(&self.dest).to_bytes();
        self.peering_key = Some(key[..crate::constants::TICKET_LENGTH].to_vec());
    }

    pub fn queued_items(&self) -> usize {
        self.queued_unhandled.len() + self.queued_handled.len()
    }

    pub fn queue_unhandled_message(&mut self, transient_id: &[u8]) {
        self.queued_unhandled.push(transient_id.to_vec());
    }

    pub fn queue_handled_message(&mut self, transient_id: &[u8]) {
        self.queued_handled.push(transient_id.to_vec());
    }

    pub fn process_queues(&mut self) {
        for id in self.queued_handled.drain(..) {
            self.handled.insert(id.clone());
            self.unhandled.remove(&id);
        }
        for id in self.queued_unhandled.drain(..) {
            if !self.handled.contains(&id) {
                self.unhandled.insert(id);
            }
        }
    }

    pub fn handled_messages(&self) -> Vec<Vec<u8>> {
        self.handled.iter().cloned().collect()
    }

    pub fn unhandled_messages(&self) -> Vec<Vec<u8>> {
        self.unhandled.iter().cloned().collect()
    }

    pub fn handled_message_count(&self) -> usize {
        self.handled.len()
    }

    pub fn unhandled_message_count(&self) -> usize {
        self.unhandled.len()
    }

    pub fn acceptance_rate(&self) -> f64 {
        let total = self.handled.len() + self.unhandled.len();
        if total == 0 {
            0.0
        } else {
            self.handled.len() as f64 / total as f64
        }
    }

    pub fn add_handled_message(&mut self, transient_id: &[u8]) {
        self.handled.insert(transient_id.to_vec());
        self.unhandled.remove(transient_id);
    }

    pub fn add_unhandled_message(&mut self, transient_id: &[u8]) {
        if !self.handled.contains(transient_id) {
            self.unhandled.insert(transient_id.to_vec());
        }
    }

    pub fn remove_handled_message(&mut self, transient_id: &[u8]) {
        self.handled.remove(transient_id);
    }

    pub fn remove_unhandled_message(&mut self, transient_id: &[u8]) {
        self.unhandled.remove(transient_id);
    }

    pub fn set_sync_backoff(&mut self, seconds: u32) {
        self.sync_backoff = seconds;
    }

    pub fn sync_backoff(&self) -> u32 {
        self.sync_backoff
    }
}

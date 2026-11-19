//! Request replay records bound to (peer, Package, recipient, request ID).

use std::collections::HashMap;

use crate::control::{Header, RequestId};
use crate::error::{Result, RppError};
use crate::identity::Destination;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReplayKey {
    pub peer: Destination,
    pub package: Destination,
    pub recipient: Destination,
    pub request_id: RequestId,
}

impl ReplayKey {
    pub fn new(peer: Destination, header: &Header) -> Self {
        Self {
            peer,
            package: header.package_destination,
            recipient: header.recipient,
            request_id: header.request_id,
        }
    }
}

struct Entry {
    request: Vec<u8>,
    response: Vec<u8>,
    expires: u64,
}

/// In-memory replay state. It is not persisted, so requests issued before
/// `started_at` are refused through their expiry (RC1 "Preserve replay state
/// across restart or refuse outstanding requests").
pub struct ReplayCache {
    entries: HashMap<ReplayKey, Entry>,
    started_at: u64,
    capacity: usize,
}

impl ReplayCache {
    pub fn new(started_at: u64, capacity: usize) -> Self {
        Self { entries: HashMap::new(), started_at, capacity }
    }

    /// `Ok(Some(response))` for an identical retry, `Ok(None)` for new work.
    pub fn lookup(&self, key: &ReplayKey, request: &[u8], issued: u64) -> Result<Option<Vec<u8>>> {
        match self.entries.get(key) {
            Some(entry) if entry.request == request => Ok(Some(entry.response.clone())),
            Some(_) => Err(RppError::ReplayConflict),
            None if issued < self.started_at => Err(RppError::Expired),
            None => Ok(None),
        }
    }

    pub fn record(
        &mut self,
        key: ReplayKey,
        request: &[u8],
        response: &[u8],
        expires: u64,
        now: u64,
    ) -> Result<()> {
        self.entries.retain(|_, entry| entry.expires > now);
        if self.entries.len() >= self.capacity {
            return Err(RppError::Storage("replay cache full".into()));
        }
        self.entries
            .insert(key, Entry { request: request.to_vec(), response: response.to_vec(), expires });
        Ok(())
    }

    /// Whether this request ID was ever recorded for the key's peer/package.
    pub fn contains(&self, key: &ReplayKey) -> bool {
        self.entries.contains_key(key)
    }
}

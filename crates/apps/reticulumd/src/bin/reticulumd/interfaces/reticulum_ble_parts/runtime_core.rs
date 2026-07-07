use super::fragment::{BleReassembler, FragmentError};

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

const MAX_BLE_FRAGMENT_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReticulumBleRole {
    Central,
    Peripheral,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PeerRegistration {
    Added,
    Updated,
    DuplicateRejected { retained_role: ReticulumBleRole },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ReticulumBleCounters {
    pub(crate) duplicate_rejections: u64,
    pub(crate) stale_reassembly_drops: u64,
    pub(crate) malformed_fragments: u64,
    pub(crate) fragments_rx: u64,
    pub(crate) packets_rx: u64,
}

#[derive(Debug, Clone)]
struct ReticulumBlePeer {
    active_address: String,
    addresses: BTreeSet<String>,
    role: ReticulumBleRole,
    mtu: usize,
}

#[derive(Debug)]
pub(crate) struct ReticulumBleRuntimeCore {
    local_identity: [u8; 16],
    reassembler: BleReassembler,
    peers: BTreeMap<[u8; 16], ReticulumBlePeer>,
    pub(crate) counters: ReticulumBleCounters,
}

impl ReticulumBleRuntimeCore {
    pub(crate) fn new(local_identity: [u8; 16]) -> Self {
        Self {
            local_identity,
            reassembler: BleReassembler::new(Duration::from_secs(30)),
            peers: BTreeMap::new(),
            counters: ReticulumBleCounters::default(),
        }
    }

    pub(crate) fn register_peer(
        &mut self,
        identity: [u8; 16],
        address: impl Into<String>,
        role: ReticulumBleRole,
        mtu: usize,
    ) -> PeerRegistration {
        let address = address.into();
        let retained_role = if self.local_identity < identity {
            ReticulumBleRole::Central
        } else {
            ReticulumBleRole::Peripheral
        };
        match self.peers.get_mut(&identity) {
            Some(peer) if peer.role != role => {
                peer.addresses.insert(address.clone());
                if role == retained_role {
                    peer.role = role;
                    peer.active_address = address;
                    peer.mtu = mtu;
                }
                self.counters.duplicate_rejections =
                    self.counters.duplicate_rejections.saturating_add(1);
                PeerRegistration::DuplicateRejected { retained_role }
            }
            Some(peer) => {
                peer.addresses.insert(address.clone());
                peer.active_address = address;
                peer.mtu = mtu;
                PeerRegistration::Updated
            }
            None => {
                let mut addresses = BTreeSet::new();
                addresses.insert(address.clone());
                self.peers.insert(
                    identity,
                    ReticulumBlePeer { active_address: address, addresses, role, mtu },
                );
                PeerRegistration::Added
            }
        }
    }

    pub(crate) fn active_address(&self, identity: &[u8; 16]) -> Option<&str> {
        self.peers.get(identity).map(|peer| peer.active_address.as_str())
    }

    pub(crate) fn receive_fragment(
        &mut self,
        identity: [u8; 16],
        fragment: &[u8],
        now: Instant,
    ) -> Result<Option<Vec<u8>>, FragmentError> {
        if fragment.len() > MAX_BLE_FRAGMENT_BYTES {
            self.counters.malformed_fragments = self.counters.malformed_fragments.saturating_add(1);
            return Err(FragmentError::Oversize(fragment.len()));
        }
        self.counters.stale_reassembly_drops =
            self.counters.stale_reassembly_drops.saturating_add(self.reassembler.drop_stale(now));
        match self.reassembler.receive_fragment(identity, fragment, now) {
            Ok(Some(packet)) => {
                self.counters.fragments_rx = self.counters.fragments_rx.saturating_add(1);
                self.counters.packets_rx = self.counters.packets_rx.saturating_add(1);
                Ok(Some(packet))
            }
            Ok(None) => {
                self.counters.fragments_rx = self.counters.fragments_rx.saturating_add(1);
                Ok(None)
            }
            Err(err) => {
                self.counters.malformed_fragments =
                    self.counters.malformed_fragments.saturating_add(1);
                Err(err)
            }
        }
    }
}

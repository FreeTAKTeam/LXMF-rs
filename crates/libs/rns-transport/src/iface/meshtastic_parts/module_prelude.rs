use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::hash::{AddressHash, ADDRESS_HASH_SIZE};
use crate::iface::{IfaceRole, IfaceSource, InterfaceManager, RxMessage};
use crate::packet::Packet;

use super::{Interface, InterfaceContext, TxMessage};

const REQUEST_PREFIX: &[u8; 3] = b"REQ";
const DEFAULT_MESHTASTIC_HW_MTU: usize = 564;
const DEFAULT_MESHTASTIC_PACKET_PAYLOAD: usize = 200;
const DEFAULT_HOP_LIMIT: u8 = 7;
const DEFAULT_BITRATE_BPS: u64 = 500;
const DEFAULT_DESTINATION_CACHE_SIZE: usize = 20;
const MAX_REQUESTED_CHUNKS_PER_NODE: usize = 10;
const MESHTASTIC_CHANNEL_CAPACITY: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshtasticInterfaceConfig {
    pub hop_limit: u8,
    pub bitrate_bps: u64,
    pub max_payload_bytes: usize,
    pub send_delay: Duration,
    pub destination_cache_size: usize,
}

impl MeshtasticInterfaceConfig {
    #[must_use]
    pub fn from_modem_preset(preset: u8) -> Self {
        Self { send_delay: modem_preset_delay(preset), ..Self::default() }
    }
}

impl Default for MeshtasticInterfaceConfig {
    fn default() -> Self {
        Self {
            hop_limit: DEFAULT_HOP_LIMIT,
            bitrate_bps: DEFAULT_BITRATE_BPS,
            max_payload_bytes: DEFAULT_MESHTASTIC_PACKET_PAYLOAD,
            send_delay: Duration::from_secs(7),
            destination_cache_size: DEFAULT_DESTINATION_CACHE_SIZE,
        }
    }
}

#[must_use]
pub fn modem_preset_delay(preset: u8) -> Duration {
    match preset {
        8 => Duration::from_millis(400),
        6 => Duration::from_secs(1),
        5 => Duration::from_secs(3),
        7 => Duration::from_secs(12),
        4 => Duration::from_secs(4),
        3 => Duration::from_secs(6),
        1 => Duration::from_secs(15),
        0 => Duration::from_secs(8),
        _ => Duration::from_secs(7),
    }
}

#[must_use]
pub fn calc_meshtastic_index(curr_index: u8) -> u8 {
    curr_index.wrapping_add(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshtasticDestination {
    Broadcast,
    Node(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshtasticReceivedFrame {
    pub from: u32,
    pub payload: Vec<u8>,
}

impl MeshtasticReceivedFrame {
    #[must_use]
    pub fn new(from: u32, payload: &[u8]) -> Self {
        Self { from, payload: payload.to_vec() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshtasticTransmitFrame {
    pub destination: MeshtasticDestination,
    pub payload: Vec<u8>,
    pub hop_limit: u8,
    pub want_ack: bool,
    pub want_response: bool,
    pub channel_index: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeshtasticTunnelStatus {
    pub queued_transmissions: usize,
    pub destination_routes: usize,
    pub packets_rx: u64,
    pub packets_tx: u64,
    pub chunks_rx: u64,
    pub chunks_tx: u64,
    pub requested_retransmits: u64,
    pub decode_errors: u64,
    pub last_error: Option<String>,
}

impl MeshtasticTunnelStatus {
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "queued_transmissions": self.queued_transmissions,
            "destination_routes": self.destination_routes,
            "packets_rx": self.packets_rx,
            "packets_tx": self.packets_tx,
            "chunks_rx": self.chunks_rx,
            "chunks_tx": self.chunks_tx,
            "requested_retransmits": self.requested_retransmits,
            "decode_errors": self.decode_errors,
            "last_error": self.last_error,
        })
    }
}

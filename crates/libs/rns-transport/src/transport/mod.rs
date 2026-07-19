use alloc::sync::Arc;
use announce_limits::AnnounceLimits;
use announce_table::AnnounceTable;
use link_table::LinkTable;
use packet_cache::PacketCache;
use path_requests::create_path_request_destination;
use path_requests::PathRequests;
use path_requests::TagBytes;
use path_table::PathTable;
use rand_core::OsRng;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tunnels::create_tunnel_synthesize_destination;
use tunnels::TunnelTable;

use tokio::sync::broadcast;
use tokio::sync::Mutex;
use tokio::sync::MutexGuard;
use x25519_dalek::PublicKey;

use crate::destination::link::Link;
use crate::destination::link::LinkEvent;
use crate::destination::link::LinkEventData;
use crate::destination::link::LinkHandleResult;
use crate::destination::link::LinkId;
use crate::destination::link::LinkStatus;
use crate::destination::DestinationAnnounce;
use crate::destination::DestinationDesc;
use crate::destination::DestinationHandleStatus;
use crate::destination::DestinationName;
use crate::destination::ProofStrategy;
use crate::destination::SingleInputDestination;
use crate::destination::SingleOutputDestination;

use crate::error::RnsError;
use crate::hash::{AddressHash, Hash, HASH_SIZE};
use crate::identity::{Identity, PrivateIdentity};

use crate::iface::AnnounceBroadcastPolicy;
use crate::iface::IfaceRole;
use crate::iface::IfaceSource;
use crate::iface::InterfaceManager;
use crate::iface::InterfaceRxReceiver;
use crate::iface::RxMessage;
use crate::iface::TxDispatchTrace;
use crate::iface::TxMessage;
use crate::iface::TxMessageType;

use crate::packet::DestinationType;
use crate::packet::Packet;
use crate::packet::PacketContext;
use crate::packet::PacketDataBuffer;
use crate::packet::PacketType;
use crate::ratchets::{encrypt_for_public_key, now_secs, RatchetStore};
use crate::resource::{build_resource_request_packet, ResourceEvent, ResourceManager};

mod announce_limits;
mod announce_table;
mod diag;
mod link_table;
mod packet_cache;
mod packet_disk_cache;
mod path_requests;
mod path_table;
mod reticulum_announce_cache;
mod reticulum_path_store;
mod runtime_management;
mod tunnels;

pub use announce_limits::AnnounceRateTableEntry;
pub use packet_disk_cache::{CachedPacket, ReticulumPacketDiskCache};

pub use reticulum_path_store::{
    RestoredReticulumPathIdentity, ReticulumPathTableRestoreReport,
    ReticulumPathTableRestoreSkipped,
};

pub mod test_bridge {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    use crate::storage::messages::MessageRecord;

    pub trait InboundTestBridge: Send + Sync {
        fn accept_inbound_for_test(&self, record: MessageRecord) -> std::io::Result<()>;
    }

    thread_local! {
        static BRIDGE: RefCell<HashMap<String, Rc<dyn InboundTestBridge>>> =
            RefCell::new(HashMap::new());
    }

    pub fn reset() {
        BRIDGE.with(|bridge| bridge.borrow_mut().clear());
    }

    pub fn register(identity: impl Into<String>, daemon: Rc<dyn InboundTestBridge>) {
        BRIDGE.with(|bridge| {
            bridge.borrow_mut().insert(identity.into(), daemon);
        });
    }

    pub fn deliver_outbound(record: &MessageRecord) -> bool {
        let daemon = BRIDGE.with(|bridge| bridge.borrow().get(&record.destination).cloned());
        let Some(daemon) = daemon else {
            return false;
        };

        let inbound = MessageRecord {
            id: record.id.clone(),
            source: record.source.clone(),
            destination: record.destination.clone(),
            title: record.title.clone(),
            content: record.content.clone(),
            timestamp: record.timestamp,
            direction: "in".into(),
            fields: record.fields.clone(),
            receipt_status: None,
        };
        daemon.accept_inbound_for_test(inbound).is_ok()
    }
}

// Transport-wide packet tracing remains off by default to keep runtime noise low.
const PACKET_TRACE: bool = false;
pub const PATHFINDER_M: usize = 128; // Max hops

#[allow(dead_code)]
const INTERVAL_LINKS_CHECK: Duration = Duration::from_secs(1);
#[allow(dead_code)]
const INTERVAL_INPUT_LINK_CLEANUP: Duration = Duration::from_secs(20);
#[allow(dead_code)]
const INTERVAL_OUTPUT_LINK_REPEAT: Duration = Duration::from_secs(6);
const INTERVAL_IFACE_CLEANUP: Duration = Duration::from_secs(10);
const INTERVAL_ANNOUNCES_RETRANSMIT: Duration = Duration::from_secs(1);
const INTERVAL_KEEP_PACKET_CACHED: Duration = Duration::from_secs(180);
const INTERVAL_PACKET_CACHE_CLEANUP: Duration = Duration::from_secs(90);
const LOCAL_PATH_RESPONSE_COOLDOWN: Duration = Duration::from_millis(750);

/// How long an auto-spawned per-peer unicast UDP iface may sit idle
/// before `handle_cleanup` tears it down. Sized so typical peers'
/// announce cadences (default 300s) refresh it several times over.
const UNICAST_IFACE_IDLE_TIMEOUT: Duration = Duration::from_secs(1800);

// Other constants
#[allow(dead_code)]
const KEEP_ALIVE_REQUEST: u8 = 0xFF;
const KEEP_ALIVE_RESPONSE: u8 = 0xFE;

#[derive(Clone)]
pub struct ReceivedData {
    pub destination: AddressHash,
    pub data: PacketDataBuffer,
    pub payload_mode: ReceivedPayloadMode,
    pub ratchet_used: bool,
    pub context: Option<PacketContext>,
    pub request_id: Option<[u8; 16]>,
    pub hops: Option<u8>,
    pub interface: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceivedPayloadMode {
    FullWire,
    DestinationStripped,
}

pub struct TransportConfig {
    name: String,
    identity: PrivateIdentity,
    broadcast: bool,
    retransmit: bool,
    connected_to_shared_instance: bool,
    announce_cache_capacity: usize,
    announce_retry_limit: u8,
    announce_queue_len: usize,
    announce_cap: usize,
    path_request_timeout_secs: u64,
    link_proof_timeout_secs: u64,
    // Retains propagated LinkTable entries; direct-link watchdog timing stays RTT-driven in Link.
    link_idle_timeout_secs: u64,
    resource_retry_interval_secs: u64,
    resource_retry_limit: u8,
    ratchet_store_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryReceipt {
    /// Hash of the packet authenticated by the received proof.
    ///
    /// The field name is retained for API compatibility. Prefer
    /// [`DeliveryReceipt::packet_hash`] in new code.
    pub message_id: [u8; 32],
}

impl DeliveryReceipt {
    pub fn new(message_id: [u8; 32]) -> Self {
        Self { message_id }
    }

    pub fn packet_hash(&self) -> Hash {
        Hash::new(self.message_id)
    }

    pub fn matches_packet_hash(&self, packet_hash: &Hash) -> bool {
        self.message_id == packet_hash.to_bytes()
    }
}

pub trait ReceiptHandler: Send + Sync {
    fn on_receipt(&self, receipt: &DeliveryReceipt);
}

#[derive(Clone)]
pub struct AnnounceEvent {
    pub destination: Arc<Mutex<SingleOutputDestination>>,
    pub app_data: PacketDataBuffer,
    pub ratchet: Option<[u8; crate::destination::RATCHET_LENGTH]>,
    pub name_hash: [u8; crate::destination::NAME_HASH_LENGTH],
    pub hops: u8,
    pub interface: Vec<u8>,
}

pub(crate) struct TransportHandler {
    config: TransportConfig,
    iface_manager: Arc<Mutex<InterfaceManager>>,
    announce_tx: broadcast::Sender<AnnounceEvent>,

    path_table: PathTable,
    announce_table: AnnounceTable,
    link_table: LinkTable,
    single_in_destinations: HashMap<AddressHash, Arc<Mutex<SingleInputDestination>>>,
    single_in_destination_app_data: HashMap<AddressHash, Vec<u8>>,
    single_out_destinations: HashMap<AddressHash, Arc<Mutex<SingleOutputDestination>>>,

    announce_limits: AnnounceLimits,
    packet_signal_cache: VecDeque<(Hash, PacketSignal)>,
    network_identity: Option<PrivateIdentity>,
    discovery_enabled: bool,

    out_links: HashMap<AddressHash, Arc<Mutex<Link>>>,
    in_links: HashMap<AddressHash, Arc<Mutex<Link>>>,

    packet_cache: Mutex<PacketCache>,

    path_requests: PathRequests,

    link_in_event_tx: broadcast::Sender<LinkEventData>,
    received_data_tx: broadcast::Sender<ReceivedData>,
    ratchet_store: Option<RatchetStore>,

    resource_manager: ResourceManager,
    resource_response_packets: Vec<Packet>,
    resource_events_tx: broadcast::Sender<ResourceEvent>,

    fixed_dest_path_requests: AddressHash,
    fixed_dest_tunnel_synthesize: AddressHash,
    tunnel_table: TunnelTable,

    /// Per-peer *virtual* unicast UDP ifaces pinned to a specific
    /// `SocketAddr` when an announce arrives over a multicast iface.
    /// Keyed by the peer's socket address; value is
    /// `(virtual_iface_hash, last_seen)`. Refreshed on every announce
    /// from that peer and GC'd by `handle_cleanup` after
    /// `UNICAST_IFACE_IDLE_TIMEOUT`.
    ///
    /// The virtual iface shares its tx channel with the host multicast
    /// iface; actual wire routing (unicast vs multicast) is resolved
    /// inside the host `UdpInterface` by looking the hash up in its
    /// `PeerRouting` map (see `multicast_peer_routings`).
    unicast_udp_ifaces: HashMap<std::net::SocketAddr, (AddressHash, Instant)>,

    /// One per registered multicast `UdpInterface`: the shared
    /// `PeerRouting` that iface's tx/rx tasks consult. Populated by
    /// `Transport::add_multicast_udp_interface`; consulted by
    /// `unicast_iface_for_source` to register discovered peers.
    multicast_peer_routings: HashMap<AddressHash, Arc<Mutex<crate::iface::udp::PeerRouting>>>,

    cancel: CancellationToken,
    receipt_handler: Option<Arc<dyn ReceiptHandler>>,
}

pub struct Transport {
    name: String,
    link_in_event_tx: broadcast::Sender<LinkEventData>,
    link_out_event_tx: broadcast::Sender<LinkEventData>,
    received_data_tx: broadcast::Sender<ReceivedData>,
    iface_messages_tx: broadcast::Sender<RxMessage>,
    resource_events_tx: broadcast::Sender<ResourceEvent>,
    handler: Arc<Mutex<TransportHandler>>,
    iface_manager: Arc<Mutex<InterfaceManager>>,
    cancel: CancellationToken,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PacketSignal {
    pub rssi: Option<f64>,
    pub snr: Option<f64>,
    pub q: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NextHopMetrics {
    pub interface: AddressHash,
    pub bitrate: u64,
    pub hardware_mtu: Option<usize>,
    pub per_bit_latency: Option<f64>,
}

impl NextHopMetrics {
    pub fn per_byte_latency(self) -> Option<f64> {
        self.per_bit_latency.map(|latency| latency * 8.0)
    }

    pub fn first_hop_timeout(self, mtu: usize, default_timeout: Duration) -> Duration {
        self.per_byte_latency()
            .map(|latency| default_timeout + Duration::from_secs_f64(mtu as f64 * latency))
            .unwrap_or(default_timeout)
    }

    pub fn extra_link_proof_timeout(self, mtu: usize) -> Duration {
        self.per_byte_latency()
            .map(|latency| Duration::from_secs_f64(mtu as f64 * latency))
            .unwrap_or_default()
    }
}

#[derive(Clone)]
pub struct TransportChannel {
    handler: Arc<Mutex<TransportHandler>>,
    link_id: AddressHash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendPacketOutcome {
    SentDirect,
    SentBroadcast,
    DroppedMissingDestinationIdentity,
    DroppedCiphertextTooLarge,
    DroppedEncryptFailed,
    DroppedNoRoute,
}

/// Aggregate outcome for a best-effort send across active Reticulum links.
///
/// Fan-out can partially succeed, so a single `Result` cannot represent the
/// outcome without discarding useful information about the other links.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LinkSendReport {
    /// Active links that matched the requested destination, if any.
    pub matched_links: usize,
    /// Matching links whose packet reached an interface queue.
    pub sent_links: usize,
    /// Matching links whose packet could not be built or dispatched.
    pub failed_links: usize,
}

impl LinkSendReport {
    pub fn is_complete(self) -> bool {
        self.matched_links > 0 && self.sent_links == self.matched_links && self.failed_links == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendPacketTrace {
    pub outcome: SendPacketOutcome,
    /// Hash of the packet after transport preparation, including encryption.
    ///
    /// This is `None` only when packet preparation failed before a stable
    /// outbound identity could be produced.
    pub packet_hash: Option<Hash>,
    pub direct_iface: Option<AddressHash>,
    pub broadcast: bool,
    pub dispatch: TxDispatchTrace,
}

impl SendPacketTrace {
    fn without_packet_hash(outcome: SendPacketOutcome) -> Self {
        Self {
            outcome,
            packet_hash: None,
            direct_iface: None,
            broadcast: false,
            dispatch: TxDispatchTrace::default(),
        }
    }

    fn with_packet_hash(
        outcome: SendPacketOutcome,
        packet_hash: Hash,
        direct_iface: Option<AddressHash>,
        broadcast: bool,
        dispatch: TxDispatchTrace,
    ) -> Self {
        Self { outcome, packet_hash: Some(packet_hash), direct_iface, broadcast, dispatch }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportPathStatus {
    pub destination: AddressHash,
    pub path_found: bool,
    pub next_hop: Option<AddressHash>,
    pub interface: Option<AddressHash>,
    pub hops: Option<u8>,
}

// Transport internals are decomposed by concern for testability and bounded change sets.
// announce: announce handling and retransmit scheduling primitives.
mod announce;
// config: transport configuration builders and defaults.
mod config;
// core: construction and minimal high-level transport API methods.
mod core;
// handler: packet send pipeline and routing/encryption outcomes.
mod handler;
// jobs: background maintenance loops and periodic work.
mod jobs;
// links: link lifecycle and link-scoped data/resource operations.
mod links;
// path: path request/response forwarding and intermediate handling.
mod path;
// resource_wire: link-scoped resource packet handling on inbound wire paths.
mod resource_wire;
// wire_encryption: packet-context policy for transport-layer encryption.
mod wire_encryption;
// wire_receipt: proof correlation and cryptographic receipt validation.
mod wire_receipt;
// wire: inbound packet handlers and wire-level packet logic.
mod wire;

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{:02x}", byte);
    }
    out
}

#[cfg(test)]
mod tests;

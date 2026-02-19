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
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

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
use crate::destination::SingleInputDestination;
use crate::destination::SingleOutputDestination;

use crate::error::RnsError;
use crate::hash::{AddressHash, Hash, HASH_SIZE};
use crate::identity::{Identity, PrivateIdentity};

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
pub mod announce_table;
pub mod discovery;
mod link_table;
mod packet_cache;
mod path_requests;
pub mod path_table;

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

// TODO: Configure via features
const PACKET_TRACE: bool = false;
pub const PATHFINDER_M: usize = 128; // Max hops

const INTERVAL_LINKS_CHECK: Duration = Duration::from_secs(1);
const INTERVAL_INPUT_LINK_CLEANUP: Duration = Duration::from_secs(20);
const INTERVAL_OUTPUT_LINK_RESTART: Duration = Duration::from_secs(60);
const INTERVAL_OUTPUT_LINK_REPEAT: Duration = Duration::from_secs(6);
const INTERVAL_OUTPUT_LINK_KEEP: Duration = Duration::from_secs(5);
const INTERVAL_IFACE_CLEANUP: Duration = Duration::from_secs(10);
const INTERVAL_ANNOUNCES_RETRANSMIT: Duration = Duration::from_secs(1);
const INTERVAL_KEEP_PACKET_CACHED: Duration = Duration::from_secs(180);
const INTERVAL_PACKET_CACHE_CLEANUP: Duration = Duration::from_secs(90);

// Other constants
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
    announce_cache_capacity: usize,
    announce_retry_limit: u8,
    announce_queue_len: usize,
    announce_cap: usize,
    path_request_timeout_secs: u64,
    link_proof_timeout_secs: u64,
    link_idle_timeout_secs: u64,
    resource_retry_interval_secs: u64,
    resource_retry_limit: u8,
    ratchet_store_path: Option<PathBuf>,
}

pub struct DeliveryReceipt {
    pub message_id: [u8; 32],
}

impl DeliveryReceipt {
    pub fn new(message_id: [u8; 32]) -> Self {
        Self { message_id }
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
    single_out_destinations: HashMap<AddressHash, Arc<Mutex<SingleOutputDestination>>>,

    announce_limits: AnnounceLimits,

    out_links: HashMap<AddressHash, Arc<Mutex<Link>>>,
    in_links: HashMap<AddressHash, Arc<Mutex<Link>>>,

    packet_cache: Mutex<PacketCache>,

    path_requests: PathRequests,

    link_in_event_tx: broadcast::Sender<LinkEventData>,
    received_data_tx: broadcast::Sender<ReceivedData>,
    ratchet_store: Option<RatchetStore>,

    resource_manager: ResourceManager,
    resource_events_tx: broadcast::Sender<ResourceEvent>,

    fixed_dest_path_requests: AddressHash,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendPacketOutcome {
    SentDirect,
    SentBroadcast,
    DroppedMissingDestinationIdentity,
    DroppedCiphertextTooLarge,
    DroppedEncryptFailed,
    DroppedNoRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendPacketTrace {
    pub outcome: SendPacketOutcome,
    pub direct_iface: Option<AddressHash>,
    pub broadcast: bool,
    pub dispatch: TxDispatchTrace,
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

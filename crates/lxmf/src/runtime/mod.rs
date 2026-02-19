mod announce_helpers;
mod announce_rate_limit;
mod bootstrap;
mod config;
mod delivery_options;
mod handle;
mod identity_io;
mod inbound_helpers;
mod peer_cache;
mod propagation_link;
mod public_types;
mod receipt_flow;
mod receipt_helpers;
mod relay_helpers;
mod request_handlers;
mod rpc_helpers;
mod runtime_loop;
mod send_helpers;
mod send_pipeline;
mod startup_workers;
mod state;
mod support;
mod wire_codec;

use crate::cli::daemon::DaemonStatus;
use crate::cli::profile::{
    load_profile_settings, load_reticulum_config, profile_paths, resolve_identity_path,
    resolve_runtime_profile_name, InterfaceEntry, ProfilePaths, ProfileSettings,
};
use crate::helpers::normalize_display_name;
#[cfg(test)]
use crate::inbound_decode::InboundPayloadMode;
use crate::payload_fields::WireFields;
use crate::LxmfError;
use announce_helpers::{
    annotate_peer_records_with_announce_metadata, encode_delivery_display_name_app_data,
    encode_propagation_node_app_data,
};
use announce_rate_limit::trigger_rate_limited_announce;
use delivery_options::merge_outbound_delivery_options;
use identity_io::{drop_empty_identity_stub, load_or_create_identity};
use inbound_helpers::build_propagation_envelope;
#[cfg(test)]
use inbound_helpers::decode_inbound_payload;
use peer_cache::{
    apply_runtime_identity_restore, load_peer_identity_cache, persist_peer_identity_cache,
};
use receipt_flow::{handle_receipt_event, resolve_link_destination, ReceiptBridge, ReceiptEvent};
use receipt_helpers::{
    format_relay_request_status, is_message_marked_delivered,
    parse_alternative_relay_request_status, prune_receipt_mappings_for_message,
    track_outbound_resource_mapping, track_receipt_mapping,
};
use relay_helpers::{
    normalize_relay_destination_hash, propagation_relay_candidates, short_hash_prefix,
    wait_for_external_relay_selection,
};
use request_handlers::handle_runtime_request;
use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::identity::{Identity, PrivateIdentity};
use reticulum::iface::tcp_client::TcpClient;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::rpc::{
    AnnounceBridge, InterfaceRecord, OutboundBridge, RpcDaemon, RpcEvent, RpcRequest,
};
use reticulum::storage::messages::{MessageRecord, MessagesStore};
use reticulum::transport::{Transport, TransportConfig};
use rpc_helpers::{annotate_response_meta, build_send_params_with_source, resolve_transport};
use runtime_loop::runtime_thread;
use send_helpers::{
    can_send_opportunistic, opportunistic_payload, parse_delivery_method, send_outcome_is_sent,
    send_outcome_status, DeliveryMethod,
};
use serde::Deserialize;
use serde_json::{json, Value};
use startup_workers::{
    spawn_receipt_worker, spawn_startup_announce_burst, spawn_transport_workers,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use support::{
    clean_non_empty, extract_identity_hash, generate_message_id, interface_to_rpc, now_epoch_secs,
    parse_bind_host_port, source_hash_from_private_key_hex,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::watch;
use wire_codec::{build_wire_message, sanitize_outbound_wire_fields};
#[cfg(test)]
use wire_codec::{json_to_rmpv, rmpv_to_json};

pub use config::RuntimeConfig;
pub use public_types::{
    EventsProbeReport, RpcProbeReport, RuntimeProbeReport, SendCommandRequest, SendMessageRequest,
    SendMessageResponse,
};

const INFERRED_TRANSPORT_BIND: &str = "127.0.0.1:0";
const DEFAULT_ANNOUNCE_INTERVAL_SECS: u64 = 60;
const STARTUP_ANNOUNCE_BURST_DELAYS_SECS: &[u64] = &[5, 15, 30];
const POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS: u64 = 20;
const MAX_ALTERNATIVE_PROPAGATION_RELAYS: usize = 3;
const PROPAGATION_PATH_TIMEOUT: Duration = Duration::from_secs(8);
const PROPAGATION_LINK_TIMEOUT: Duration = Duration::from_secs(15);
const PROPAGATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const OUTBOUND_DELIVERY_OPTIONS_FIELD: &str = "__delivery_options";

const PR_IDLE: u32 = 0x00;
const PR_PATH_REQUESTED: u32 = 0x01;
const PR_LINK_ESTABLISHING: u32 = 0x02;
const PR_LINK_ESTABLISHED: u32 = 0x03;
const PR_REQUEST_SENT: u32 = 0x04;
const PR_RECEIVING: u32 = 0x05;
const PR_RESPONSE_RECEIVED: u32 = 0x06;
const PR_COMPLETE: u32 = 0x07;
const PR_NO_PATH: u32 = 0xF0;
const PR_LINK_FAILED: u32 = 0xF1;
const PR_TRANSFER_FAILED: u32 = 0xF2;
const PR_NO_IDENTITY_RCVD: u32 = 0xF3;
const PR_NO_ACCESS: u32 = 0xF4;

pub use handle::{start, RuntimeHandle};
use state::{
    AnnounceTarget, EmbeddedTransportBridge, OutboundDeliveryOptionsCompat, PeerAnnounceMeta,
    PeerCrypto, PreparedSendMessage, RuntimeCommand, RuntimePropagationSyncParams,
    RuntimePropagationSyncState, RuntimeRequest, RuntimeResponse, WorkerInit, WorkerState,
};

#[cfg(test)]
mod tests;

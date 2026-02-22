pub mod codec;
mod daemon;
pub mod event_sink;
pub mod http;
pub mod replay;
mod send_request;

use rmpv::Value as MsgPackValue;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};

use crate::storage::messages::{AnnounceRecord, MessageRecord, MessagesStore};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::time::Duration;

use send_request::parse_outbound_send_request;

include!("types.rs");
include!("params.rs");
include!("helpers.rs");

pub fn handle_framed_request(daemon: &RpcDaemon, bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    daemon.handle_framed_request(bytes)
}

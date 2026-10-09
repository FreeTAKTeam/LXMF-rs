#![allow(clippy::items_after_test_module)]

pub mod codec;
mod daemon;
pub mod event_sink;
pub mod http;
pub mod replay;
mod send_request;
pub mod zmq;
mod zmq_complexity;
pub mod zmq_metrics;

use rmpv::Value as MsgPackValue;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};

use crate::storage::messages::{
    AnnounceRecord, MessageRecord, MessagesStore, PropagationEntryRecord,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::broadcast;
use tokio::time::Duration;

use send_request::{
    parse_outbound_send_batch_request, parse_outbound_send_request, NormalizedSendBatchRequest,
};

include!("types.rs");
include!("params.rs");
include!("helpers.rs");

pub fn handle_framed_request(daemon: &RpcDaemon, bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    daemon.handle_framed_request(bytes)
}

thread_local! {
    static RPC_ZMQ_PRINCIPAL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static RPC_SESSION_CONTEXT: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

const LEGACY_RPC_SESSION_ID: &str = "legacy-rpc";

fn with_rpc_session<T>(session_id: &str, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            RPC_SESSION_CONTEXT.with(|context| {
                context.replace(self.0.take());
            });
        }
    }
    let _restore =
        Restore(RPC_SESSION_CONTEXT.with(|context| context.replace(Some(session_id.to_owned()))));
    operation()
}

fn current_rpc_session_id() -> String {
    RPC_SESSION_CONTEXT
        .with(|context| context.borrow().clone())
        .unwrap_or_else(|| LEGACY_RPC_SESSION_ID.to_owned())
}

fn with_zmq_principal<T>(principal: &str, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            RPC_ZMQ_PRINCIPAL.with(|context| {
                context.replace(self.0.take());
            });
        }
    }
    let _restore =
        Restore(RPC_ZMQ_PRINCIPAL.with(|context| context.replace(Some(principal.to_owned()))));
    operation()
}

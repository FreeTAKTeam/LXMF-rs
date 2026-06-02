use super::*;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use hmac::Mac;

mod cursor_utils;
mod sdk_helpers;

use cursor_utils::*;
use sdk_helpers::{python_reference_meta, SDK_VERSION, STORE_FORWARD_MAX_MESSAGES_LIMIT};
mod dispatch;
mod dispatch_legacy_clear;
mod dispatch_legacy_messages;
mod dispatch_legacy_misc;
mod dispatch_legacy_propagation;
mod dispatch_legacy_router;
mod events;
mod events_redaction;
mod events_sink;
mod init;
mod metrics;
mod sdk_attachments;
mod sdk_auth_http;
mod sdk_capabilities;
mod sdk_identity;
mod sdk_markers;
mod sdk_negotiate_poll;
mod sdk_operations;
mod sdk_outbound;
mod sdk_paper_command;
mod sdk_runtime;
mod sdk_topics;
mod sdk_voice;

include!("daemon/tests.rs");

use crate::http::TransportAuthContext;
use crate::{InterfaceRecord as RpcInterfaceRecord, RpcDaemon, RpcError, RpcRequest, RpcResponse};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::convert::TryFrom;
use std::fs;
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tonic::transport::{Certificate, Identity as TlsIdentity, ServerTlsConfig};
use tonic::{Request, Response, Status};
use x509_parser::extensions::ParsedExtension;
use x509_parser::prelude::{FromDer, GeneralName, X509Certificate};

const FILE_DESCRIPTOR_SET: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/lxmf-reflection-descriptor.bin"));

pub mod lxmf {
    pub mod common {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.common.v1.rs"));
        }
    }

    pub mod runtime {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.runtime.v1.rs"));
        }
    }

    pub mod delivery {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.delivery.v1.rs"));
        }
    }

    pub mod command {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.command.v1.rs"));
        }
    }

    pub mod admin {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.admin.v1.rs"));
        }
    }

    pub mod topics {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.topics.v1.rs"));
        }
    }

    pub mod attachments {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.attachments.v1.rs"));
        }
    }

    pub mod events {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.events.v1.rs"));
        }
    }

    pub mod identity {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.identity.v1.rs"));
        }
    }

    pub mod markers {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.markers.v1.rs"));
        }
    }

    pub mod peers {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.peers.v1.rs"));
        }
    }
}

use lxmf::admin::v1::interface_admin_service_server::{
    InterfaceAdminService, InterfaceAdminServiceServer,
};
use lxmf::admin::v1::{
    ListInterfacesRequest, ListInterfacesResponse, ReloadConfigRequest, ReloadConfigResponse,
    SetInterfacesRequest, SetInterfacesResponse,
};
use lxmf::attachments::v1::attachment_service_server::{
    AttachmentService, AttachmentServiceServer,
};
use lxmf::attachments::v1::{
    Attachment, DeleteAttachmentRequest, DeleteAttachmentResponse, DownloadAttachmentRequest,
    DownloadAttachmentResponse, DownloadChunkRequest, DownloadChunkResponse, GetAttachmentRequest,
    GetAttachmentResponse, ListAttachmentsRequest, ListAttachmentsResponse, StoreAttachmentRequest,
    StoreAttachmentResponse, UploadChunkRequest, UploadChunkResponse, UploadCommitRequest,
    UploadCommitResponse, UploadHandle, UploadStartRequest, UploadStartResponse,
};
use lxmf::command::v1::command_service_server::{CommandService, CommandServiceServer};
use lxmf::command::v1::{
    CommandSession as CommandSessionProto, GetCommandSessionRequest, GetCommandSessionResponse,
    InvokeCommandRequest, InvokeCommandResponse, ListCommandSessionsRequest,
    ListCommandSessionsResponse, ReplyCommandRequest, ReplyCommandResponse,
};
use lxmf::common::v1::InterfaceRecord;
use lxmf::delivery::v1::delivery_service_server::{DeliveryService, DeliveryServiceServer};
use lxmf::delivery::v1::{
    CancelRequest, CancelResponse, GetStatusRequest, GetStatusResponse,
    MessageRecord as DeliveryMessageRecord, SendRequest, SendResponse,
};
use lxmf::events::v1::event_service_server::{EventService, EventServiceServer};
use lxmf::events::v1::{
    EventEnvelope, PollEventsRequest, PollEventsResponse, SubscribeEventsRequest,
};
use lxmf::identity::v1::identity_service_server::{IdentityService, IdentityServiceServer};
use lxmf::identity::v1::{
    ActivateIdentityRequest, ActivateIdentityResponse, AnnounceNowRequest, AnnounceNowResponse,
    BootstrapIdentityRequest, BootstrapIdentityResponse, Contact, ExportIdentityRequest,
    ExportIdentityResponse, Identity, IdentityBundle, ImportIdentityRequest,
    ImportIdentityResponse, ListContactsRequest, ListContactsResponse, ListIdentitiesRequest,
    ListIdentitiesResponse, ListPresenceRequest, ListPresenceResponse, PresenceRecord,
    ResolveIdentityRequest, ResolveIdentityResponse, UpdateContactRequest, UpdateContactResponse,
};
use lxmf::markers::v1::marker_service_server::{MarkerService, MarkerServiceServer};
use lxmf::markers::v1::{
    CreateMarkerRequest, CreateMarkerResponse, DeleteMarkerRequest, DeleteMarkerResponse, GeoPoint,
    ListMarkersRequest, ListMarkersResponse, Marker, UpdateMarkerPositionRequest,
    UpdateMarkerPositionResponse,
};
use lxmf::peers::v1::peer_service_server::{PeerService, PeerServiceServer};
use lxmf::peers::v1::{
    ClearPeersRequest, ClearPeersResponse, ListPeersRequest, ListPeersResponse, Peer,
    SearchPeersRequest, SearchPeersResponse, SyncPeerRequest, SyncPeerResponse, UnpeerRequest,
    UnpeerResponse,
};
use lxmf::runtime::v1::runtime_service_server::{RuntimeService, RuntimeServiceServer};
use lxmf::runtime::v1::{
    GetDaemonStatusRequest, GetDaemonStatusResponse, GetPropagationStatusRequest,
    GetPropagationStatusResponse, GetSnapshotRequest, GetSnapshotResponse, NegotiateMtlsAuthConfig,
    NegotiateRequest, NegotiateResponse, NegotiateRpcBackendConfig, NegotiateRuntimeConfig,
    NegotiateStoreForwardConfig, NegotiateTokenAuthConfig, SetPropagationRequest,
    SetPropagationResponse,
};
use lxmf::topics::v1::topic_service_server::{TopicService, TopicServiceServer};
use lxmf::topics::v1::{
    CreateTopicRequest, CreateTopicResponse, GetTopicRequest, GetTopicResponse, ListTopicsRequest,
    ListTopicsResponse, PublishTopicRequest, PublishTopicResponse, SubscribeTopicRequest,
    SubscribeTopicResponse, Topic, UnsubscribeTopicRequest, UnsubscribeTopicResponse,
};

#[derive(Clone, Debug)]
pub struct GrpcTlsConfig {
    pub cert_chain_path: PathBuf,
    pub private_key_path: PathBuf,
    pub client_ca_path: Option<PathBuf>,
}

#[derive(Clone)]
struct GrpcBridge {
    daemon: Arc<RpcDaemon>,
    next_request_id: Arc<AtomicU64>,
}

#[derive(Debug)]
struct GrpcRequestLogMeta {
    peer: Option<String>,
    grpc_method: &'static str,
    rpc_method: Option<&'static str>,
    rpc_request_id: Option<u64>,
}

impl GrpcBridge {
    fn new(daemon: Arc<RpcDaemon>) -> Self {
        Self { daemon, next_request_id: Arc::new(AtomicU64::new(1)) }
    }

    async fn invoke(
        &self,
        grpc_method: &'static str,
        peer: Option<String>,
        rpc_method: &'static str,
        params: Option<JsonValue>,
    ) -> Result<RpcResponse, Status> {
        let daemon = self.daemon.clone();
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let meta = GrpcRequestLogMeta {
            peer,
            grpc_method,
            rpc_method: Some(rpc_method),
            rpc_request_id: Some(id),
        };
        let started_at = std::time::Instant::now();
        let result = tokio::task::spawn_blocking(move || {
            daemon.handle_rpc(RpcRequest { id, method: rpc_method.to_string(), params })
        })
        .await
        .map_err(|err| Status::internal(format!("gRPC worker join failed: {err}")))?
        .map_err(|err| map_io_error(&err))
        .and_then(ensure_rpc_success);
        emit_grpc_access_log(&meta, &result, started_at.elapsed().as_millis() as u64);
        result
    }
}

fn grpc_peer<T>(request: &Request<T>) -> Option<String> {
    request.remote_addr().map(|addr| addr.to_string())
}

fn emit_grpc_access_log(
    meta: &GrpcRequestLogMeta,
    result: &Result<RpcResponse, Status>,
    elapsed_ms: u64,
) {
    let (status_code, error_text) = match result {
        Ok(_) => ("OK".to_string(), None),
        Err(status) => (status.code().to_string(), Some(status.message().to_string())),
    };
    if pretty_console_logs_enabled() {
        eprintln!(
            "{} {} {} {} {}{}{}",
            pretty_tag("grpc", 36),
            pretty_status(&status_code, grpc_status_color(&status_code)),
            pretty_elapsed(elapsed_ms),
            pretty_method(short_grpc_method(meta.grpc_method)),
            pretty_secondary(&format!("peer={}", meta.peer.as_deref().unwrap_or("-"))),
            meta.rpc_method
                .map(|method| format!(" {}", pretty_secondary(&format!("rpc={method}"))))
                .unwrap_or_default(),
            error_text
                .as_ref()
                .map(|error| format!(" {}", pretty_error(error)))
                .unwrap_or_default()
        );
        return;
    }
    let payload = json!({
        "event": "grpc_request",
        "peer": meta.peer,
        "grpc_method": meta.grpc_method,
        "rpc_method": meta.rpc_method,
        "rpc_request_id": meta.rpc_request_id,
        "trace_ref": serde_json::Value::Null,
        "status_code": status_code,
        "elapsed_ms": elapsed_ms,
        "ok": result.is_ok(),
        "error": error_text,
    });
    eprintln!("{}", payload);
}

fn emit_grpc_status_log(meta: &GrpcRequestLogMeta, status: &Status, elapsed_ms: u64) {
    if pretty_console_logs_enabled() {
        eprintln!(
            "{} {} {} {} {}{}{}",
            pretty_tag("grpc", 36),
            pretty_status(
                &status.code().to_string(),
                grpc_status_color(&status.code().to_string())
            ),
            pretty_elapsed(elapsed_ms),
            pretty_method(short_grpc_method(meta.grpc_method)),
            pretty_secondary(&format!("peer={}", meta.peer.as_deref().unwrap_or("-"))),
            meta.rpc_method
                .map(|method| format!(" {}", pretty_secondary(&format!("rpc={method}"))))
                .unwrap_or_default(),
            if status.message().is_empty() {
                String::new()
            } else {
                format!(" {}", pretty_error(status.message()))
            }
        );
        return;
    }
    let payload = json!({
        "event": "grpc_request",
        "peer": meta.peer,
        "grpc_method": meta.grpc_method,
        "rpc_method": meta.rpc_method,
        "rpc_request_id": meta.rpc_request_id,
        "trace_ref": serde_json::Value::Null,
        "status_code": status.code().to_string(),
        "elapsed_ms": elapsed_ms,
        "ok": false,
        "error": status.message(),
    });
    eprintln!("{}", payload);
}

fn emit_grpc_ok_log(meta: &GrpcRequestLogMeta, elapsed_ms: u64) {
    if pretty_console_logs_enabled() {
        eprintln!(
            "{} {} {} {} {}{}",
            pretty_tag("grpc", 36),
            pretty_status("OK", 32),
            pretty_elapsed(elapsed_ms),
            pretty_method(short_grpc_method(meta.grpc_method)),
            pretty_secondary(&format!("peer={}", meta.peer.as_deref().unwrap_or("-"))),
            meta.rpc_method
                .map(|method| format!(" {}", pretty_secondary(&format!("rpc={method}"))))
                .unwrap_or_default()
        );
        return;
    }
    let payload = json!({
        "event": "grpc_request",
        "peer": meta.peer,
        "grpc_method": meta.grpc_method,
        "rpc_method": meta.rpc_method,
        "rpc_request_id": meta.rpc_request_id,
        "trace_ref": serde_json::Value::Null,
        "status_code": "OK",
        "elapsed_ms": elapsed_ms,
        "ok": true,
        "error": serde_json::Value::Null,
    });
    eprintln!("{}", payload);
}

fn pretty_console_logs_enabled() -> bool {
    matches!(
        std::env::var("LXMF_LOG_PRETTY").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

fn pretty_color_enabled() -> bool {
    if matches!(
        std::env::var("LXMF_LOG_COLOR").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON" | "always" | "ALWAYS")
    ) {
        return true;
    }
    if matches!(
        std::env::var("LXMF_LOG_COLOR").ok().as_deref(),
        Some("0" | "false" | "FALSE" | "no" | "NO" | "off" | "OFF" | "never" | "NEVER")
    ) {
        return false;
    }
    pretty_console_logs_enabled() && std::io::stderr().is_terminal()
}

fn ansi(text: &str, code: &str) -> String {
    if pretty_color_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn pretty_tag(label: &str, color: u8) -> String {
    ansi(&format!("[{label:<4}]"), &color.to_string())
}

fn pretty_status(label: &str, color: u8) -> String {
    ansi(&format!("{label:<12}"), &format!("1;{color}"))
}

fn pretty_elapsed(elapsed_ms: u64) -> String {
    ansi(&format!("{elapsed_ms:>4}ms"), "2")
}

fn pretty_method(method: &str) -> String {
    ansi(method, "1")
}

fn pretty_secondary(value: &str) -> String {
    ansi(value, "2")
}

fn pretty_error(value: &str) -> String {
    ansi(&format!("error={value}"), "31")
}

fn short_grpc_method(method: &str) -> &str {
    method.strip_prefix('/').unwrap_or(method)
}

fn grpc_status_color(status_code: &str) -> u8 {
    match status_code {
        "OK" => 32,
        "InvalidArgument" | "Unauthenticated" | "PermissionDenied" | "NotFound"
        | "FailedPrecondition" | "AlreadyExists" => 33,
        _ => 31,
    }
}

#[derive(Clone)]
struct RuntimeGrpcService {
    bridge: GrpcBridge,
}

#[derive(Clone)]
struct DeliveryGrpcService {
    bridge: GrpcBridge,
}

#[derive(Clone)]
struct CommandGrpcService {
    bridge: GrpcBridge,
}

#[derive(Clone)]
struct InterfaceAdminGrpcService {
    bridge: GrpcBridge,
}

#[derive(Clone)]
struct TopicGrpcService {
    bridge: GrpcBridge,
}

#[derive(Clone)]
struct AttachmentGrpcService {
    bridge: GrpcBridge,
}

#[derive(Clone)]
struct EventGrpcService {
    bridge: GrpcBridge,
}

#[derive(Clone)]
struct IdentityGrpcService {
    bridge: GrpcBridge,
}

#[derive(Clone)]
struct MarkerGrpcService {
    bridge: GrpcBridge,
}

#[derive(Clone)]
struct PeerGrpcService {
    bridge: GrpcBridge,
}

#[derive(Debug, Deserialize)]
struct SnapshotPayload {
    runtime_id: String,
    state: String,
    active_contract_version: u32,
    config_revision: u64,
    #[serde(default)]
    effective_capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DeliverySendPayload {
    message_id: String,
}

#[derive(Debug, Deserialize)]
struct DeliveryMessagePayload {
    id: String,
    source: String,
    destination: String,
    title: String,
    content: String,
    timestamp: i64,
    direction: String,
    #[serde(default)]
    fields: Option<JsonValue>,
    #[serde(default)]
    receipt_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeliveryStatusPayload {
    #[serde(default)]
    message: Option<DeliveryMessagePayload>,
    #[serde(default)]
    meta: JsonValue,
}

#[derive(Debug, Deserialize)]
struct DeliveryCancelPayload {
    message_id: String,
    result: String,
}

#[derive(Debug, Deserialize)]
struct CommandSessionPayload {
    command_id: String,
    correlation_id: String,
    command: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    delivery_state: Option<String>,
    command_state: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    request_payload: JsonValue,
    #[serde(default)]
    response_payload: Option<JsonValue>,
    #[serde(default)]
    accepted: Option<bool>,
    #[serde(default)]
    extensions: JsonValue,
}

#[derive(Debug, Deserialize)]
struct CommandInvokeEnvelopePayload {
    accepted: bool,
    payload: JsonValue,
    #[serde(default)]
    extensions: JsonValue,
    session: CommandSessionPayload,
}

#[derive(Debug, Deserialize)]
struct CommandInvokeResultPayload {
    response: CommandInvokeEnvelopePayload,
}

#[derive(Debug, Deserialize)]
struct CommandReplyResultPayload {
    accepted: bool,
    correlation_id: String,
    reply_accepted: bool,
    payload: JsonValue,
    session: CommandSessionPayload,
}

#[derive(Debug, Deserialize)]
struct CommandSessionGetPayload {
    #[serde(default)]
    session: Option<CommandSessionPayload>,
}

#[derive(Debug, Deserialize)]
struct CommandSessionListEnvelopePayload {
    #[serde(default)]
    sessions: Vec<CommandSessionPayload>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommandSessionListPayload {
    session_list: CommandSessionListEnvelopePayload,
}

#[derive(Debug, Deserialize)]
struct NegotiatePayload {
    runtime_id: String,
    active_contract_version: u32,
    #[serde(default)]
    effective_capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ListInterfacesPayload {
    interfaces: Vec<RpcInterfaceRecord>,
}

#[derive(Debug, Deserialize)]
struct TopicRecordPayload {
    topic_id: String,
    #[serde(default)]
    topic_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AttachmentRecordPayload {
    attachment_id: String,
    name: String,
    content_type: String,
    byte_len: u64,
    checksum_sha256: String,
    created_ts_ms: u64,
    #[serde(default)]
    expires_ts_ms: Option<u64>,
    #[serde(default)]
    topic_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TopicEnvelopePayload {
    topic: Option<TopicRecordPayload>,
}

#[derive(Debug, Deserialize)]
struct AttachmentEnvelopePayload {
    attachment: Option<AttachmentRecordPayload>,
}

#[derive(Debug, Deserialize)]
struct TopicListPayload {
    #[serde(default)]
    topics: Vec<TopicRecordPayload>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TopicAcceptedPayload {
    #[serde(default)]
    accepted: bool,
    #[serde(default)]
    topic_id: String,
}

#[derive(Debug, Deserialize)]
struct AttachmentListPayload {
    #[serde(default)]
    attachments: Vec<AttachmentRecordPayload>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeleteAttachmentPayload {
    #[serde(default)]
    accepted: bool,
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
struct DownloadAttachmentPayload {
    #[serde(default)]
    accepted: bool,
    attachment_id: String,
    #[serde(default)]
    bytes_base64: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UploadHandlePayload {
    upload_id: String,
    attachment_id: String,
    chunk_size_hint: u32,
    next_offset: u64,
}

#[derive(Debug, Deserialize)]
struct UploadStartPayload {
    upload: UploadHandlePayload,
}

#[derive(Debug, Deserialize)]
struct UploadChunkPayload {
    upload_chunk: UploadChunkStatePayload,
}

#[derive(Debug, Deserialize)]
struct UploadChunkStatePayload {
    #[serde(default)]
    accepted: bool,
    next_offset: u64,
    #[serde(default)]
    complete: bool,
}

#[derive(Debug, Deserialize)]
struct DownloadChunkPayload {
    download_chunk: DownloadChunkStatePayload,
}

#[derive(Debug, Deserialize)]
struct DownloadChunkStatePayload {
    attachment_id: String,
    offset: u64,
    next_offset: u64,
    total_size: u64,
    #[serde(default)]
    done: bool,
    checksum_sha256: String,
    bytes_base64: String,
}

#[derive(Debug, Deserialize)]
struct EventEnvelopePayload {
    event_type: String,
    #[serde(default)]
    payload: JsonValue,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    seq_no: Option<u64>,
    #[serde(default)]
    ts_ms: Option<u64>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    source_component: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PollEventsPayload {
    #[serde(default)]
    events: Vec<EventEnvelopePayload>,
    next_cursor: String,
    #[serde(default)]
    dropped_count: u64,
}

#[derive(Debug, Deserialize)]
struct IdentityListPayload {
    #[serde(default)]
    identities: Vec<IdentityBundlePayload>,
}

#[derive(Debug, Deserialize)]
struct ActivateIdentityPayload {
    #[serde(default)]
    accepted: bool,
    identity: String,
}

#[derive(Debug, Deserialize)]
struct IdentityBundlePayload {
    identity: String,
    public_key: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct IdentityEnvelopePayload {
    identity: Option<IdentityBundlePayload>,
}

#[derive(Debug, Deserialize)]
struct IdentityExportBundlePayload {
    bundle_base64: String,
    #[serde(default)]
    passphrase: Option<String>,
    #[serde(default)]
    extensions: JsonValue,
}

#[derive(Debug, Deserialize)]
struct IdentityExportPayload {
    bundle: IdentityExportBundlePayload,
}

#[derive(Debug, Deserialize)]
struct ResolveIdentityPayload {
    #[serde(default)]
    identity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnnounceNowPayload {
    #[serde(default)]
    accepted: bool,
    announce_id: u64,
}

#[derive(Debug, Deserialize)]
struct PresenceListEnvelopePayload {
    presence_list: PresenceListPayload,
}

#[derive(Debug, Deserialize)]
struct PresenceListPayload {
    #[serde(default)]
    peers: Vec<PresenceRecordPayload>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PresenceRecordPayload {
    peer_id: String,
    last_seen_ts_ms: i64,
    first_seen_ts_ms: i64,
    seen_count: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    name_source: Option<String>,
    #[serde(default)]
    trust_level: Option<String>,
    #[serde(default)]
    bootstrap: Option<bool>,
    #[serde(default)]
    extensions: JsonValue,
}

#[derive(Debug, Deserialize)]
struct ContactEnvelopePayload {
    contact: ContactPayload,
}

#[derive(Debug, Deserialize)]
struct ContactListEnvelopePayload {
    contact_list: ContactListPayload,
}

#[derive(Debug, Deserialize)]
struct ContactListPayload {
    #[serde(default)]
    contacts: Vec<ContactPayload>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContactPayload {
    identity: String,
    #[serde(default)]
    display_name: Option<String>,
    trust_level: String,
    bootstrap: bool,
    updated_ts_ms: u64,
    #[serde(default)]
    metadata: JsonValue,
    #[serde(default)]
    extensions: JsonValue,
}

#[derive(Debug, Deserialize)]
struct BootstrapIdentityPayload {
    contact: ContactPayload,
    #[serde(default)]
    synced: bool,
}

#[derive(Debug, Deserialize)]
struct MarkerRecordPayload {
    marker_id: String,
    label: String,
    position: GeoPointPayload,
    #[serde(default)]
    topic_id: Option<String>,
    revision: u64,
    updated_ts_ms: u64,
}

#[derive(Debug, Deserialize)]
struct GeoPointPayload {
    lat: f64,
    lon: f64,
    #[serde(default)]
    alt_m: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct MarkerEnvelopePayload {
    marker: MarkerRecordPayload,
}

#[derive(Debug, Deserialize)]
struct MarkerListPayload {
    #[serde(default)]
    markers: Vec<MarkerRecordPayload>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeleteMarkerPayload {
    #[serde(default)]
    accepted: bool,
    marker_id: String,
}

#[derive(Debug, Deserialize)]
struct PeerRecordPayload {
    peer: String,
    last_seen: i64,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    name_source: Option<String>,
    #[serde(default)]
    peer_type: Option<String>,
    #[serde(default)]
    alive: bool,
    #[serde(default)]
    last_sync_attempt: i64,
    #[serde(default)]
    next_sync_attempt: i64,
    #[serde(default)]
    sync_backoff: u32,
    #[serde(default)]
    network_distance: u32,
    #[serde(default)]
    rx_bytes: u64,
    #[serde(default)]
    tx_bytes: u64,
    #[serde(default)]
    acceptance_rate: f64,
    #[serde(default)]
    first_seen: i64,
    #[serde(default)]
    seen_count: u64,
}

#[derive(Debug, Deserialize)]
struct PeerListPayload {
    #[serde(default)]
    peers: Vec<PeerRecordPayload>,
}

#[derive(Debug, Deserialize)]
struct SyncPeerPayload {
    peer: String,
    #[serde(default)]
    synced: bool,
}

#[derive(Debug, Deserialize)]
struct UnpeerPayload {
    #[serde(default)]
    removed: bool,
}

#[derive(Debug, Deserialize)]
struct ClearPeersPayload {
    cleared: String,
}

#[derive(Debug, Deserialize, Default)]
struct SetInterfacesPayload {
    #[serde(default)]
    updated: bool,
    #[serde(default)]
    applied_interfaces: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ReloadConfigPayload {
    #[serde(default)]
    reloaded: bool,
    #[serde(default)]
    hot_applied_legacy_tcp_only: bool,
}

#[tonic::async_trait]
impl RuntimeService for RuntimeGrpcService {
    async fn negotiate(
        &self,
        request: Request<NegotiateRequest>,
    ) -> Result<Response<NegotiateResponse>, Status> {
        let peer = grpc_peer(&request);
        let params = negotiate_request_to_json(request.into_inner())?;
        let response = self
            .bridge
            .invoke(
                "/lxmf.runtime.v1.RuntimeService/Negotiate",
                peer,
                "sdk_negotiate_v2",
                Some(params),
            )
            .await?;
        let payload: NegotiatePayload = parse_result(response)?;
        Ok(Response::new(NegotiateResponse {
            runtime_id: payload.runtime_id,
            active_contract_version: payload.active_contract_version,
            effective_capabilities: payload.effective_capabilities,
        }))
    }

    async fn get_snapshot(
        &self,
        request: Request<GetSnapshotRequest>,
    ) -> Result<Response<GetSnapshotResponse>, Status> {
        let peer = grpc_peer(&request);
        let include_counts = request.into_inner().include_counts;
        let snapshot = self
            .bridge
            .invoke(
                "/lxmf.runtime.v1.RuntimeService/GetSnapshot",
                peer,
                "sdk_snapshot_v2",
                Some(json!({ "include_counts": include_counts })),
            )
            .await?;
        let payload: SnapshotPayload = parse_result(snapshot)?;
        Ok(Response::new(GetSnapshotResponse {
            runtime_id: payload.runtime_id,
            state: payload.state,
            active_contract_version: payload.active_contract_version,
            config_revision: payload.config_revision,
            effective_capabilities: payload.effective_capabilities,
        }))
    }

    async fn get_daemon_status(
        &self,
        request: Request<GetDaemonStatusRequest>,
    ) -> Result<Response<GetDaemonStatusResponse>, Status> {
        let peer = grpc_peer(&request);
        let response = self
            .bridge
            .invoke(
                "/lxmf.runtime.v1.RuntimeService/GetDaemonStatus",
                peer,
                "daemon_status_ex",
                None,
            )
            .await?;
        let status: JsonValue = parse_result(response)?;
        Ok(Response::new(GetDaemonStatusResponse { status: json_to_struct(status) }))
    }

    async fn get_propagation_status(
        &self,
        request: Request<GetPropagationStatusRequest>,
    ) -> Result<Response<GetPropagationStatusResponse>, Status> {
        let peer = grpc_peer(&request);
        let response = self
            .bridge
            .invoke(
                "/lxmf.runtime.v1.RuntimeService/GetPropagationStatus",
                peer,
                "propagation_status",
                None,
            )
            .await?;
        let payload: JsonValue = parse_result(response)?;
        let propagation = payload.get("propagation").cloned().unwrap_or(payload);
        Ok(Response::new(GetPropagationStatusResponse { propagation: json_to_struct(propagation) }))
    }

    async fn set_propagation(
        &self,
        request: Request<SetPropagationRequest>,
    ) -> Result<Response<SetPropagationResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        params.insert("enabled".to_string(), JsonValue::Bool(request.enabled));
        if let Some(store_root) = request.store_root {
            params.insert("store_root".to_string(), JsonValue::String(store_root));
        }
        if let Some(target_cost) = request.target_cost {
            params.insert(
                "target_cost".to_string(),
                JsonValue::Number(serde_json::Number::from(target_cost)),
            );
        }
        if let Some(limit) = request.message_storage_limit_mb {
            params.insert(
                "message_storage_limit_mb".to_string(),
                JsonValue::Number(serde_json::Number::from(limit)),
            );
        }
        if let Some(autopeer) = request.autopeer {
            params.insert("autopeer".to_string(), JsonValue::Bool(autopeer));
        }
        if let Some(autopeer_maxdepth) = request.autopeer_maxdepth {
            params.insert(
                "autopeer_maxdepth".to_string(),
                JsonValue::Number(serde_json::Number::from(autopeer_maxdepth)),
            );
        }
        if !request.static_peers.is_empty() {
            params.insert(
                "static_peers".to_string(),
                JsonValue::Array(request.static_peers.into_iter().map(JsonValue::String).collect()),
            );
        }
        if let Some(max_peers) = request.max_peers {
            params.insert(
                "max_peers".to_string(),
                JsonValue::Number(serde_json::Number::from(max_peers)),
            );
        }
        if let Some(from_static_only) = request.from_static_only {
            params.insert("from_static_only".to_string(), JsonValue::Bool(from_static_only));
        }
        if let Some(peering_cost) = request.peering_cost {
            params.insert(
                "peering_cost".to_string(),
                JsonValue::Number(serde_json::Number::from(peering_cost)),
            );
        }
        if let Some(remote_peering_cost_max) = request.remote_peering_cost_max {
            params.insert(
                "remote_peering_cost_max".to_string(),
                JsonValue::Number(serde_json::Number::from(remote_peering_cost_max)),
            );
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.runtime.v1.RuntimeService/SetPropagation",
                peer,
                "propagation_enable",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: JsonValue = parse_result(response)?;
        let propagation = payload.get("propagation").cloned().unwrap_or(payload);
        Ok(Response::new(SetPropagationResponse { propagation: json_to_struct(propagation) }))
    }
}

#[tonic::async_trait]
impl CommandService for CommandGrpcService {
    async fn invoke_command(
        &self,
        request: Request<InvokeCommandRequest>,
    ) -> Result<Response<InvokeCommandResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let payload = request
            .payload
            .ok_or_else(|| Status::invalid_argument("command payload is required"))?;
        let mut params = serde_json::Map::new();
        params.insert("command".to_string(), JsonValue::String(request.command));
        params.insert("payload".to_string(), struct_to_json(payload));
        if let Some(target) = request.target {
            params.insert("target".to_string(), JsonValue::String(target));
        }
        if let Some(timeout_ms) = request.timeout_ms {
            params.insert(
                "timeout_ms".to_string(),
                JsonValue::Number(serde_json::Number::from(timeout_ms)),
            );
        }
        if let Some(extensions) = request.extensions {
            params.insert("extensions".to_string(), struct_to_json(extensions));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.command.v1.CommandService/InvokeCommand",
                peer,
                "sdk_command_invoke_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: CommandInvokeResultPayload = parse_result(response)?;
        Ok(Response::new(InvokeCommandResponse {
            accepted: payload.response.accepted,
            payload: json_to_struct(payload.response.payload),
            extensions: json_to_struct(payload.response.extensions),
            session: Some(payload.response.session.into()),
        }))
    }

    async fn reply_command(
        &self,
        request: Request<ReplyCommandRequest>,
    ) -> Result<Response<ReplyCommandResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let payload =
            request.payload.ok_or_else(|| Status::invalid_argument("reply payload is required"))?;
        let mut params = serde_json::Map::new();
        params.insert("correlation_id".to_string(), JsonValue::String(request.correlation_id));
        params.insert("accepted".to_string(), JsonValue::Bool(request.accepted));
        params.insert("payload".to_string(), struct_to_json(payload));
        if let Some(extensions) = request.extensions {
            params.insert("extensions".to_string(), struct_to_json(extensions));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.command.v1.CommandService/ReplyCommand",
                peer,
                "sdk_command_reply_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: CommandReplyResultPayload = parse_result(response)?;
        Ok(Response::new(ReplyCommandResponse {
            accepted: payload.accepted,
            correlation_id: payload.correlation_id,
            reply_accepted: payload.reply_accepted,
            payload: json_to_struct(payload.payload),
            session: Some(payload.session.into()),
        }))
    }

    async fn get_command_session(
        &self,
        request: Request<GetCommandSessionRequest>,
    ) -> Result<Response<GetCommandSessionResponse>, Status> {
        let peer = grpc_peer(&request);
        let correlation_id = request.into_inner().correlation_id;
        let response = self
            .bridge
            .invoke(
                "/lxmf.command.v1.CommandService/GetCommandSession",
                peer,
                "sdk_command_session_get_v2",
                Some(json!({ "correlation_id": correlation_id })),
            )
            .await?;
        let payload: CommandSessionGetPayload = parse_result(response)?;
        Ok(Response::new(GetCommandSessionResponse { session: payload.session.map(Into::into) }))
    }

    async fn list_command_sessions(
        &self,
        request: Request<ListCommandSessionsRequest>,
    ) -> Result<Response<ListCommandSessionsResponse>, Status> {
        let peer = grpc_peer(&request);
        let page = request.into_inner().page;
        let params = page.as_ref().map_or_else(
            || json!({}),
            |page| {
                let mut params = serde_json::Map::new();
                if !page.page_token.is_empty() {
                    params.insert("cursor".to_string(), JsonValue::String(page.page_token.clone()));
                }
                if page.page_size > 0 {
                    params.insert(
                        "limit".to_string(),
                        JsonValue::Number(serde_json::Number::from(page.page_size)),
                    );
                }
                JsonValue::Object(params)
            },
        );
        let response = self
            .bridge
            .invoke(
                "/lxmf.command.v1.CommandService/ListCommandSessions",
                peer,
                "sdk_command_session_list_v2",
                Some(params),
            )
            .await?;
        let payload: CommandSessionListPayload = parse_result(response)?;
        Ok(Response::new(ListCommandSessionsResponse {
            sessions: payload.session_list.sessions.into_iter().map(Into::into).collect(),
            page_info: Some(lxmf::common::v1::PageInfo {
                next_page_token: payload.session_list.next_cursor.unwrap_or_default(),
            }),
        }))
    }
}

#[tonic::async_trait]
impl DeliveryService for DeliveryGrpcService {
    async fn send(&self, request: Request<SendRequest>) -> Result<Response<SendResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        params.insert("id".to_string(), JsonValue::String(request.id));
        params.insert("source".to_string(), JsonValue::String(request.source));
        params.insert("destination".to_string(), JsonValue::String(request.destination));
        params.insert("content".to_string(), JsonValue::String(request.content));
        if !request.title.is_empty() {
            params.insert("title".to_string(), JsonValue::String(request.title));
        }
        if let Some(fields) = request.fields {
            params.insert("fields".to_string(), struct_to_json(fields));
        }
        if let Some(method) = request.method {
            params.insert("method".to_string(), JsonValue::String(method));
        }
        if let Some(stamp_cost) = request.stamp_cost {
            params.insert(
                "stamp_cost".to_string(),
                JsonValue::Number(serde_json::Number::from(stamp_cost)),
            );
        }
        if let Some(include_ticket) = request.include_ticket {
            params.insert("include_ticket".to_string(), JsonValue::Bool(include_ticket));
        }
        if let Some(try_propagation_on_fail) = request.try_propagation_on_fail {
            params.insert(
                "try_propagation_on_fail".to_string(),
                JsonValue::Bool(try_propagation_on_fail),
            );
        }

        let response = self
            .bridge
            .invoke(
                "/lxmf.delivery.v1.DeliveryService/Send",
                peer,
                "sdk_send_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: DeliverySendPayload = parse_result(response)?;
        Ok(Response::new(SendResponse { message_id: payload.message_id }))
    }

    async fn get_status(
        &self,
        request: Request<GetStatusRequest>,
    ) -> Result<Response<GetStatusResponse>, Status> {
        let peer = grpc_peer(&request);
        let message_id = request.into_inner().message_id;
        let response = self
            .bridge
            .invoke(
                "/lxmf.delivery.v1.DeliveryService/GetStatus",
                peer,
                "sdk_status_v2",
                Some(json!({ "message_id": message_id })),
            )
            .await?;
        let payload: DeliveryStatusPayload = parse_result(response)?;
        Ok(Response::new(GetStatusResponse {
            message: payload.message.map(Into::into),
            meta: json_to_struct(payload.meta),
        }))
    }

    async fn cancel(
        &self,
        request: Request<CancelRequest>,
    ) -> Result<Response<CancelResponse>, Status> {
        let peer = grpc_peer(&request);
        let message_id = request.into_inner().message_id;
        let response = self
            .bridge
            .invoke(
                "/lxmf.delivery.v1.DeliveryService/Cancel",
                peer,
                "sdk_cancel_message_v2",
                Some(json!({ "message_id": message_id })),
            )
            .await?;
        let payload: DeliveryCancelPayload = parse_result(response)?;
        Ok(Response::new(CancelResponse { message_id: payload.message_id, result: payload.result }))
    }
}

#[tonic::async_trait]
impl InterfaceAdminService for InterfaceAdminGrpcService {
    async fn list_interfaces(
        &self,
        request: Request<ListInterfacesRequest>,
    ) -> Result<Response<ListInterfacesResponse>, Status> {
        let response = self
            .bridge
            .invoke(
                "/lxmf.admin.v1.InterfaceAdminService/ListInterfaces",
                grpc_peer(&request),
                "list_interfaces",
                None,
            )
            .await?;
        let payload: ListInterfacesPayload = parse_result(response)?;
        Ok(Response::new(ListInterfacesResponse {
            interfaces: payload.interfaces.into_iter().map(Into::into).collect(),
        }))
    }

    async fn set_interfaces(
        &self,
        request: Request<SetInterfacesRequest>,
    ) -> Result<Response<SetInterfacesResponse>, Status> {
        let peer = grpc_peer(&request);
        let interfaces = request
            .into_inner()
            .interfaces
            .into_iter()
            .map(RpcInterfaceRecord::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        match self
            .bridge
            .invoke(
                "/lxmf.admin.v1.InterfaceAdminService/SetInterfaces",
                peer,
                "set_interfaces",
                Some(json!({ "interfaces": interfaces })),
            )
            .await
        {
            Ok(response) => {
                let payload: SetInterfacesPayload = parse_result(response)?;
                Ok(Response::new(SetInterfacesResponse {
                    updated: payload.updated,
                    restart_required: false,
                    applied_interfaces: payload.applied_interfaces,
                    affected_interfaces: Vec::new(),
                }))
            }
            Err(status) if restart_required(&status) => Ok(Response::new(SetInterfacesResponse {
                updated: false,
                restart_required: true,
                applied_interfaces: Vec::new(),
                affected_interfaces: extract_affected_interfaces(&status),
            })),
            Err(status) => Err(status),
        }
    }

    async fn reload_config(
        &self,
        request: Request<ReloadConfigRequest>,
    ) -> Result<Response<ReloadConfigResponse>, Status> {
        let peer = grpc_peer(&request);
        let params = match request.into_inner().desired_interfaces {
            Some(desired_interfaces) => Some(json!({
                "interfaces": desired_interfaces
                    .interfaces
                    .into_iter()
                    .map(RpcInterfaceRecord::try_from)
                    .collect::<Result<Vec<_>, _>>()?,
            })),
            None => None,
        };
        match self
            .bridge
            .invoke(
                "/lxmf.admin.v1.InterfaceAdminService/ReloadConfig",
                peer,
                "reload_config",
                params,
            )
            .await
        {
            Ok(response) => {
                let payload: ReloadConfigPayload = parse_result(response)?;
                Ok(Response::new(ReloadConfigResponse {
                    reloaded: payload.reloaded,
                    hot_applied_legacy_tcp_only: payload.hot_applied_legacy_tcp_only,
                    restart_required: false,
                    affected_interfaces: Vec::new(),
                }))
            }
            Err(status) if restart_required(&status) => Ok(Response::new(ReloadConfigResponse {
                reloaded: false,
                hot_applied_legacy_tcp_only: false,
                restart_required: true,
                affected_interfaces: extract_affected_interfaces(&status),
            })),
            Err(status) => Err(status),
        }
    }
}

#[tonic::async_trait]
impl TopicService for TopicGrpcService {
    async fn create_topic(
        &self,
        request: Request<CreateTopicRequest>,
    ) -> Result<Response<CreateTopicResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let params = if request.topic_path.trim().is_empty() {
            json!({})
        } else {
            json!({ "topic_path": request.topic_path })
        };
        let response = self
            .bridge
            .invoke(
                "/lxmf.topics.v1.TopicService/CreateTopic",
                peer,
                "sdk_topic_create_v2",
                Some(params),
            )
            .await?;
        let payload: TopicEnvelopePayload = parse_result(response)?;
        Ok(Response::new(CreateTopicResponse { topic: payload.topic.map(Into::into) }))
    }

    async fn list_topics(
        &self,
        request: Request<ListTopicsRequest>,
    ) -> Result<Response<ListTopicsResponse>, Status> {
        let peer = grpc_peer(&request);
        let page = request.into_inner().page;
        let params = page.as_ref().map_or_else(
            || json!({}),
            |page| {
                let mut params = serde_json::Map::new();
                if !page.page_token.is_empty() {
                    params.insert("cursor".to_string(), JsonValue::String(page.page_token.clone()));
                }
                if page.page_size > 0 {
                    params.insert(
                        "limit".to_string(),
                        JsonValue::Number(serde_json::Number::from(page.page_size)),
                    );
                }
                JsonValue::Object(params)
            },
        );
        let response = self
            .bridge
            .invoke(
                "/lxmf.topics.v1.TopicService/ListTopics",
                peer,
                "sdk_topic_list_v2",
                Some(params),
            )
            .await?;
        let payload: TopicListPayload = parse_result(response)?;
        Ok(Response::new(ListTopicsResponse {
            topics: payload.topics.into_iter().map(Into::into).collect(),
            page_info: Some(lxmf::common::v1::PageInfo {
                next_page_token: payload.next_cursor.unwrap_or_default(),
            }),
        }))
    }

    async fn get_topic(
        &self,
        request: Request<GetTopicRequest>,
    ) -> Result<Response<GetTopicResponse>, Status> {
        let peer = grpc_peer(&request);
        let topic_id = request.into_inner().topic_id;
        let response = self
            .bridge
            .invoke(
                "/lxmf.topics.v1.TopicService/GetTopic",
                peer,
                "sdk_topic_get_v2",
                Some(json!({ "topic_id": topic_id })),
            )
            .await?;
        let payload: TopicEnvelopePayload = parse_result(response)?;
        Ok(Response::new(GetTopicResponse { topic: payload.topic.map(Into::into) }))
    }

    async fn subscribe_topic(
        &self,
        request: Request<SubscribeTopicRequest>,
    ) -> Result<Response<SubscribeTopicResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        params.insert("topic_id".to_string(), JsonValue::String(request.topic_id));
        if let Some(cursor) = request.cursor.filter(|value| !value.trim().is_empty()) {
            params.insert("cursor".to_string(), JsonValue::String(cursor));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.topics.v1.TopicService/SubscribeTopic",
                peer,
                "sdk_topic_subscribe_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: TopicAcceptedPayload = parse_result(response)?;
        Ok(Response::new(SubscribeTopicResponse {
            accepted: payload.accepted,
            topic_id: payload.topic_id,
        }))
    }

    async fn unsubscribe_topic(
        &self,
        request: Request<UnsubscribeTopicRequest>,
    ) -> Result<Response<UnsubscribeTopicResponse>, Status> {
        let peer = grpc_peer(&request);
        let topic_id = request.into_inner().topic_id;
        let response = self
            .bridge
            .invoke(
                "/lxmf.topics.v1.TopicService/UnsubscribeTopic",
                peer,
                "sdk_topic_unsubscribe_v2",
                Some(json!({ "topic_id": topic_id })),
            )
            .await?;
        let payload: TopicAcceptedPayload = parse_result(response)?;
        Ok(Response::new(UnsubscribeTopicResponse {
            accepted: payload.accepted,
            topic_id: payload.topic_id,
        }))
    }

    async fn publish_topic(
        &self,
        request: Request<PublishTopicRequest>,
    ) -> Result<Response<PublishTopicResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let payload = request
            .payload
            .ok_or_else(|| Status::invalid_argument("publish payload is required"))?;
        let mut params = serde_json::Map::new();
        params.insert("topic_id".to_string(), JsonValue::String(request.topic_id));
        params.insert("payload".to_string(), struct_to_json(payload));
        if let Some(correlation_id) = request.correlation_id {
            params.insert("correlation_id".to_string(), JsonValue::String(correlation_id));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.topics.v1.TopicService/PublishTopic",
                peer,
                "sdk_topic_publish_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: TopicAcceptedPayload = parse_result(response)?;
        Ok(Response::new(PublishTopicResponse { accepted: payload.accepted }))
    }
}

#[tonic::async_trait]
impl AttachmentService for AttachmentGrpcService {
    async fn store_attachment(
        &self,
        request: Request<StoreAttachmentRequest>,
    ) -> Result<Response<StoreAttachmentResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        params.insert("name".to_string(), JsonValue::String(request.name));
        params.insert("content_type".to_string(), JsonValue::String(request.content_type));
        params.insert("bytes_base64".to_string(), JsonValue::String(request.bytes_base64));
        if let Some(expires_ts_ms) = request.expires_ts_ms {
            params.insert(
                "expires_ts_ms".to_string(),
                JsonValue::Number(serde_json::Number::from(expires_ts_ms)),
            );
        }
        if !request.topic_ids.is_empty() {
            params.insert(
                "topic_ids".to_string(),
                JsonValue::Array(request.topic_ids.into_iter().map(JsonValue::String).collect()),
            );
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.attachments.v1.AttachmentService/StoreAttachment",
                peer,
                "sdk_attachment_store_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: AttachmentEnvelopePayload = parse_result(response)?;
        Ok(Response::new(StoreAttachmentResponse {
            attachment: payload.attachment.map(Into::into),
        }))
    }

    async fn get_attachment(
        &self,
        request: Request<GetAttachmentRequest>,
    ) -> Result<Response<GetAttachmentResponse>, Status> {
        let peer = grpc_peer(&request);
        let attachment_id = request.into_inner().attachment_id;
        let response = self
            .bridge
            .invoke(
                "/lxmf.attachments.v1.AttachmentService/GetAttachment",
                peer,
                "sdk_attachment_get_v2",
                Some(json!({ "attachment_id": attachment_id })),
            )
            .await?;
        let payload: AttachmentEnvelopePayload = parse_result(response)?;
        Ok(Response::new(GetAttachmentResponse { attachment: payload.attachment.map(Into::into) }))
    }

    async fn delete_attachment(
        &self,
        request: Request<DeleteAttachmentRequest>,
    ) -> Result<Response<DeleteAttachmentResponse>, Status> {
        let peer = grpc_peer(&request);
        let attachment_id = request.into_inner().attachment_id;
        let response = self
            .bridge
            .invoke(
                "/lxmf.attachments.v1.AttachmentService/DeleteAttachment",
                peer,
                "sdk_attachment_delete_v2",
                Some(json!({ "attachment_id": attachment_id })),
            )
            .await?;
        let payload: DeleteAttachmentPayload = parse_result(response)?;
        Ok(Response::new(DeleteAttachmentResponse {
            accepted: payload.accepted,
            attachment_id: payload.attachment_id,
        }))
    }

    async fn download_attachment(
        &self,
        request: Request<DownloadAttachmentRequest>,
    ) -> Result<Response<DownloadAttachmentResponse>, Status> {
        let peer = grpc_peer(&request);
        let attachment_id = request.into_inner().attachment_id;
        let response = self
            .bridge
            .invoke(
                "/lxmf.attachments.v1.AttachmentService/DownloadAttachment",
                peer,
                "sdk_attachment_download_v2",
                Some(json!({ "attachment_id": attachment_id })),
            )
            .await?;
        let payload: DownloadAttachmentPayload = parse_result(response)?;
        Ok(Response::new(DownloadAttachmentResponse {
            accepted: payload.accepted,
            attachment_id: payload.attachment_id,
            bytes_base64: payload.bytes_base64.unwrap_or_default(),
        }))
    }

    async fn upload_start(
        &self,
        request: Request<UploadStartRequest>,
    ) -> Result<Response<UploadStartResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        params.insert("name".to_string(), JsonValue::String(request.name));
        params.insert("content_type".to_string(), JsonValue::String(request.content_type));
        params.insert(
            "total_size".to_string(),
            JsonValue::Number(serde_json::Number::from(request.total_size)),
        );
        params.insert("checksum_sha256".to_string(), JsonValue::String(request.checksum_sha256));
        if let Some(expires_ts_ms) = request.expires_ts_ms {
            params.insert(
                "expires_ts_ms".to_string(),
                JsonValue::Number(serde_json::Number::from(expires_ts_ms)),
            );
        }
        if !request.topic_ids.is_empty() {
            params.insert(
                "topic_ids".to_string(),
                JsonValue::Array(request.topic_ids.into_iter().map(JsonValue::String).collect()),
            );
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.attachments.v1.AttachmentService/UploadStart",
                peer,
                "sdk_attachment_upload_start_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: UploadStartPayload = parse_result(response)?;
        Ok(Response::new(UploadStartResponse {
            upload: Some(UploadHandle {
                upload_id: payload.upload.upload_id,
                attachment_id: payload.upload.attachment_id,
                chunk_size_hint: payload.upload.chunk_size_hint,
                next_offset: payload.upload.next_offset,
            }),
        }))
    }

    async fn upload_chunk(
        &self,
        request: Request<UploadChunkRequest>,
    ) -> Result<Response<UploadChunkResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let response = self
            .bridge
            .invoke(
                "/lxmf.attachments.v1.AttachmentService/UploadChunk",
                peer,
                "sdk_attachment_upload_chunk_v2",
                Some(json!({
                    "upload_id": request.upload_id,
                    "offset": request.offset,
                    "bytes_base64": request.bytes_base64,
                })),
            )
            .await?;
        let payload: UploadChunkPayload = parse_result(response)?;
        Ok(Response::new(UploadChunkResponse {
            accepted: payload.upload_chunk.accepted,
            next_offset: payload.upload_chunk.next_offset,
            complete: payload.upload_chunk.complete,
        }))
    }

    async fn upload_commit(
        &self,
        request: Request<UploadCommitRequest>,
    ) -> Result<Response<UploadCommitResponse>, Status> {
        let peer = grpc_peer(&request);
        let upload_id = request.into_inner().upload_id;
        let response = self
            .bridge
            .invoke(
                "/lxmf.attachments.v1.AttachmentService/UploadCommit",
                peer,
                "sdk_attachment_upload_commit_v2",
                Some(json!({ "upload_id": upload_id })),
            )
            .await?;
        let payload: AttachmentEnvelopePayload = parse_result(response)?;
        Ok(Response::new(UploadCommitResponse { attachment: payload.attachment.map(Into::into) }))
    }

    async fn download_chunk(
        &self,
        request: Request<DownloadChunkRequest>,
    ) -> Result<Response<DownloadChunkResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        params.insert("attachment_id".to_string(), JsonValue::String(request.attachment_id));
        if let Some(offset) = request.offset {
            params
                .insert("offset".to_string(), JsonValue::Number(serde_json::Number::from(offset)));
        }
        if let Some(max_bytes) = request.max_bytes {
            params.insert(
                "max_bytes".to_string(),
                JsonValue::Number(serde_json::Number::from(max_bytes)),
            );
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.attachments.v1.AttachmentService/DownloadChunk",
                peer,
                "sdk_attachment_download_chunk_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: DownloadChunkPayload = parse_result(response)?;
        Ok(Response::new(DownloadChunkResponse {
            attachment_id: payload.download_chunk.attachment_id,
            offset: payload.download_chunk.offset,
            next_offset: payload.download_chunk.next_offset,
            total_size: payload.download_chunk.total_size,
            done: payload.download_chunk.done,
            checksum_sha256: payload.download_chunk.checksum_sha256,
            bytes_base64: payload.download_chunk.bytes_base64,
        }))
    }

    async fn list_attachments(
        &self,
        request: Request<ListAttachmentsRequest>,
    ) -> Result<Response<ListAttachmentsResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        if let Some(topic_id) = request.topic_id.filter(|value| !value.trim().is_empty()) {
            params.insert("topic_id".to_string(), JsonValue::String(topic_id));
        }
        if let Some(page) = request.page {
            if !page.page_token.is_empty() {
                params.insert("cursor".to_string(), JsonValue::String(page.page_token));
            }
            if page.page_size > 0 {
                params.insert(
                    "limit".to_string(),
                    JsonValue::Number(serde_json::Number::from(page.page_size)),
                );
            }
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.attachments.v1.AttachmentService/ListAttachments",
                peer,
                "sdk_attachment_list_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: AttachmentListPayload = parse_result(response)?;
        Ok(Response::new(ListAttachmentsResponse {
            attachments: payload.attachments.into_iter().map(Into::into).collect(),
            page_info: Some(lxmf::common::v1::PageInfo {
                next_page_token: payload.next_cursor.unwrap_or_default(),
            }),
        }))
    }
}

#[tonic::async_trait]
impl EventService for EventGrpcService {
    type SubscribeEventsStream =
        Pin<Box<dyn Stream<Item = Result<EventEnvelope, Status>> + Send + 'static>>;

    async fn poll_events(
        &self,
        request: Request<PollEventsRequest>,
    ) -> Result<Response<PollEventsResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        if request.max == 0 {
            return Err(Status::invalid_argument("poll max must be greater than zero"));
        }
        let mut params = serde_json::Map::new();
        params.insert("max".to_string(), JsonValue::Number(serde_json::Number::from(request.max)));
        if let Some(cursor) = request.cursor.filter(|value| !value.trim().is_empty()) {
            params.insert("cursor".to_string(), JsonValue::String(cursor));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.events.v1.EventService/PollEvents",
                peer,
                "sdk_poll_events_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: PollEventsPayload = parse_result(response)?;
        Ok(Response::new(PollEventsResponse {
            events: payload.events.into_iter().map(Into::into).collect(),
            next_cursor: payload.next_cursor,
            dropped_count: payload.dropped_count,
        }))
    }

    async fn subscribe_events(
        &self,
        request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        let meta = GrpcRequestLogMeta {
            peer: grpc_peer(&request),
            grpc_method: "/lxmf.events.v1.EventService/SubscribeEvents",
            rpc_method: None,
            rpc_request_id: None,
        };
        let started_at = std::time::Instant::now();
        let request = request.into_inner();
        if request.resume_token.as_deref().is_some_and(|token| !token.trim().is_empty()) {
            let status = Status::unimplemented(
                "resume_token is not supported for live SubscribeEvents; use PollEvents for replay",
            );
            emit_grpc_status_log(&meta, &status, started_at.elapsed().as_millis() as u64);
            return Err(status);
        }
        let receiver = self.bridge.daemon.subscribe_events();
        let stream = BroadcastStream::new(receiver).map(|item| match item {
            Ok(event) => Ok(EventEnvelope::from(event)),
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(skipped)) => Err(
                Status::unavailable(format!("event stream lagged behind by {skipped} messages")),
            ),
        });
        emit_grpc_ok_log(&meta, started_at.elapsed().as_millis() as u64);
        Ok(Response::new(Box::pin(stream)))
    }
}

#[tonic::async_trait]
impl IdentityService for IdentityGrpcService {
    async fn list_identities(
        &self,
        request: Request<ListIdentitiesRequest>,
    ) -> Result<Response<ListIdentitiesResponse>, Status> {
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/ListIdentities",
                grpc_peer(&request),
                "sdk_identity_list_v2",
                Some(json!({})),
            )
            .await?;
        let payload: IdentityListPayload = parse_result(response)?;
        Ok(Response::new(ListIdentitiesResponse {
            identities: payload.identities.into_iter().map(Into::into).collect(),
        }))
    }

    async fn activate_identity(
        &self,
        request: Request<ActivateIdentityRequest>,
    ) -> Result<Response<ActivateIdentityResponse>, Status> {
        let peer = grpc_peer(&request);
        let identity = request.into_inner().identity;
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/ActivateIdentity",
                peer,
                "sdk_identity_activate_v2",
                Some(json!({ "identity": identity })),
            )
            .await?;
        let payload: ActivateIdentityPayload = parse_result(response)?;
        Ok(Response::new(ActivateIdentityResponse {
            accepted: payload.accepted,
            identity: payload.identity,
        }))
    }

    async fn import_identity(
        &self,
        request: Request<ImportIdentityRequest>,
    ) -> Result<Response<ImportIdentityResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        params.insert("bundle_base64".to_string(), JsonValue::String(request.bundle_base64));
        if let Some(passphrase) = request.passphrase {
            params.insert("passphrase".to_string(), JsonValue::String(passphrase));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/ImportIdentity",
                peer,
                "sdk_identity_import_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: IdentityEnvelopePayload = parse_result(response)?;
        Ok(Response::new(ImportIdentityResponse { identity: payload.identity.map(Into::into) }))
    }

    async fn export_identity(
        &self,
        request: Request<ExportIdentityRequest>,
    ) -> Result<Response<ExportIdentityResponse>, Status> {
        let peer = grpc_peer(&request);
        let identity = request.into_inner().identity;
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/ExportIdentity",
                peer,
                "sdk_identity_export_v2",
                Some(json!({ "identity": identity })),
            )
            .await?;
        let payload: IdentityExportPayload = parse_result(response)?;
        Ok(Response::new(ExportIdentityResponse {
            bundle: Some(IdentityBundle {
                bundle_base64: payload.bundle.bundle_base64,
                passphrase: payload.bundle.passphrase,
                extensions: json_to_struct(payload.bundle.extensions),
            }),
        }))
    }

    async fn resolve_identity(
        &self,
        request: Request<ResolveIdentityRequest>,
    ) -> Result<Response<ResolveIdentityResponse>, Status> {
        let peer = grpc_peer(&request);
        let hash = request.into_inner().hash;
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/ResolveIdentity",
                peer,
                "sdk_identity_resolve_v2",
                Some(json!({ "hash": hash })),
            )
            .await?;
        let payload: ResolveIdentityPayload = parse_result(response)?;
        Ok(Response::new(ResolveIdentityResponse { identity: payload.identity }))
    }

    async fn announce_now(
        &self,
        request: Request<AnnounceNowRequest>,
    ) -> Result<Response<AnnounceNowResponse>, Status> {
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/AnnounceNow",
                grpc_peer(&request),
                "sdk_identity_announce_now_v2",
                Some(json!({})),
            )
            .await?;
        let payload: AnnounceNowPayload = parse_result(response)?;
        Ok(Response::new(AnnounceNowResponse {
            accepted: payload.accepted,
            announce_id: payload.announce_id,
        }))
    }

    async fn list_presence(
        &self,
        request: Request<ListPresenceRequest>,
    ) -> Result<Response<ListPresenceResponse>, Status> {
        let peer = grpc_peer(&request);
        let page = request.into_inner().page;
        let params = page.as_ref().map_or_else(
            || json!({}),
            |page| {
                let mut params = serde_json::Map::new();
                if !page.page_token.is_empty() {
                    params.insert("cursor".to_string(), JsonValue::String(page.page_token.clone()));
                }
                if page.page_size > 0 {
                    params.insert(
                        "limit".to_string(),
                        JsonValue::Number(serde_json::Number::from(page.page_size)),
                    );
                }
                JsonValue::Object(params)
            },
        );
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/ListPresence",
                peer,
                "sdk_identity_presence_list_v2",
                Some(params),
            )
            .await?;
        let payload: PresenceListEnvelopePayload = parse_result(response)?;
        Ok(Response::new(ListPresenceResponse {
            peers: payload.presence_list.peers.into_iter().map(Into::into).collect(),
            page_info: Some(lxmf::common::v1::PageInfo {
                next_page_token: payload.presence_list.next_cursor.unwrap_or_default(),
            }),
        }))
    }

    async fn update_contact(
        &self,
        request: Request<UpdateContactRequest>,
    ) -> Result<Response<UpdateContactResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        params.insert("identity".to_string(), JsonValue::String(request.identity));
        if let Some(display_name) = request.display_name {
            params.insert("display_name".to_string(), JsonValue::String(display_name));
        }
        if let Some(trust_level) = request.trust_level {
            params.insert("trust_level".to_string(), JsonValue::String(trust_level));
        }
        if let Some(bootstrap) = request.bootstrap {
            params.insert("bootstrap".to_string(), JsonValue::Bool(bootstrap));
        }
        if let Some(metadata) = request.metadata {
            params.insert("metadata".to_string(), struct_to_json(metadata));
        }
        if let Some(extensions) = request.extensions {
            params.insert("extensions".to_string(), struct_to_json(extensions));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/UpdateContact",
                peer,
                "sdk_identity_contact_update_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: ContactEnvelopePayload = parse_result(response)?;
        Ok(Response::new(UpdateContactResponse { contact: Some(payload.contact.into()) }))
    }

    async fn list_contacts(
        &self,
        request: Request<ListContactsRequest>,
    ) -> Result<Response<ListContactsResponse>, Status> {
        let peer = grpc_peer(&request);
        let page = request.into_inner().page;
        let params = page.as_ref().map_or_else(
            || json!({}),
            |page| {
                let mut params = serde_json::Map::new();
                if !page.page_token.is_empty() {
                    params.insert("cursor".to_string(), JsonValue::String(page.page_token.clone()));
                }
                if page.page_size > 0 {
                    params.insert(
                        "limit".to_string(),
                        JsonValue::Number(serde_json::Number::from(page.page_size)),
                    );
                }
                JsonValue::Object(params)
            },
        );
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/ListContacts",
                peer,
                "sdk_identity_contact_list_v2",
                Some(params),
            )
            .await?;
        let payload: ContactListEnvelopePayload = parse_result(response)?;
        Ok(Response::new(ListContactsResponse {
            contacts: payload.contact_list.contacts.into_iter().map(Into::into).collect(),
            page_info: Some(lxmf::common::v1::PageInfo {
                next_page_token: payload.contact_list.next_cursor.unwrap_or_default(),
            }),
        }))
    }

    async fn bootstrap_identity(
        &self,
        request: Request<BootstrapIdentityRequest>,
    ) -> Result<Response<BootstrapIdentityResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let response = self
            .bridge
            .invoke(
                "/lxmf.identity.v1.IdentityService/BootstrapIdentity",
                peer,
                "sdk_identity_bootstrap_v2",
                Some(json!({
                    "identity": request.identity,
                    "auto_sync": request.auto_sync,
                })),
            )
            .await?;
        let payload: BootstrapIdentityPayload = parse_result(response)?;
        Ok(Response::new(BootstrapIdentityResponse {
            contact: Some(payload.contact.into()),
            synced: payload.synced,
        }))
    }
}

#[tonic::async_trait]
impl MarkerService for MarkerGrpcService {
    async fn create_marker(
        &self,
        request: Request<CreateMarkerRequest>,
    ) -> Result<Response<CreateMarkerResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        params.insert("label".to_string(), JsonValue::String(request.label));
        params.insert("position".to_string(), geo_point_to_json(request.position));
        if let Some(topic_id) = request.topic_id.filter(|value| !value.trim().is_empty()) {
            params.insert("topic_id".to_string(), JsonValue::String(topic_id));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.markers.v1.MarkerService/CreateMarker",
                peer,
                "sdk_marker_create_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: MarkerEnvelopePayload = parse_result(response)?;
        Ok(Response::new(CreateMarkerResponse { marker: Some(payload.marker.into()) }))
    }

    async fn list_markers(
        &self,
        request: Request<ListMarkersRequest>,
    ) -> Result<Response<ListMarkersResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let mut params = serde_json::Map::new();
        if let Some(topic_id) = request.topic_id.filter(|value| !value.trim().is_empty()) {
            params.insert("topic_id".to_string(), JsonValue::String(topic_id));
        }
        if let Some(page) = request.page {
            if !page.page_token.is_empty() {
                params.insert("cursor".to_string(), JsonValue::String(page.page_token));
            }
            if page.page_size > 0 {
                params.insert(
                    "limit".to_string(),
                    JsonValue::Number(serde_json::Number::from(page.page_size)),
                );
            }
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.markers.v1.MarkerService/ListMarkers",
                peer,
                "sdk_marker_list_v2",
                Some(JsonValue::Object(params)),
            )
            .await?;
        let payload: MarkerListPayload = parse_result(response)?;
        Ok(Response::new(ListMarkersResponse {
            markers: payload.markers.into_iter().map(Into::into).collect(),
            page_info: Some(lxmf::common::v1::PageInfo {
                next_page_token: payload.next_cursor.unwrap_or_default(),
            }),
        }))
    }

    async fn update_marker_position(
        &self,
        request: Request<UpdateMarkerPositionRequest>,
    ) -> Result<Response<UpdateMarkerPositionResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let response = self
            .bridge
            .invoke(
                "/lxmf.markers.v1.MarkerService/UpdateMarkerPosition",
                peer,
                "sdk_marker_update_position_v2",
                Some(json!({
                    "marker_id": request.marker_id,
                    "expected_revision": request.expected_revision,
                    "position": geo_point_to_json(request.position),
                })),
            )
            .await?;
        let payload: MarkerEnvelopePayload = parse_result(response)?;
        Ok(Response::new(UpdateMarkerPositionResponse { marker: Some(payload.marker.into()) }))
    }

    async fn delete_marker(
        &self,
        request: Request<DeleteMarkerRequest>,
    ) -> Result<Response<DeleteMarkerResponse>, Status> {
        let peer = grpc_peer(&request);
        let request = request.into_inner();
        let response = self
            .bridge
            .invoke(
                "/lxmf.markers.v1.MarkerService/DeleteMarker",
                peer,
                "sdk_marker_delete_v2",
                Some(json!({
                    "marker_id": request.marker_id,
                    "expected_revision": request.expected_revision,
                })),
            )
            .await?;
        let payload: DeleteMarkerPayload = parse_result(response)?;
        Ok(Response::new(DeleteMarkerResponse {
            accepted: payload.accepted,
            marker_id: payload.marker_id,
        }))
    }
}

#[tonic::async_trait]
impl PeerService for PeerGrpcService {
    async fn list_peers(
        &self,
        request: Request<ListPeersRequest>,
    ) -> Result<Response<ListPeersResponse>, Status> {
        let response = self
            .bridge
            .invoke("/lxmf.peers.v1.PeerService/ListPeers", grpc_peer(&request), "list_peers", None)
            .await?;
        let payload: PeerListPayload = parse_result(response)?;
        Ok(Response::new(ListPeersResponse {
            peers: payload
                .peers
                .into_iter()
                .filter(|peer| !peer.peer.trim().is_empty())
                .map(Into::into)
                .collect(),
        }))
    }

    async fn search_peers(
        &self,
        request: Request<SearchPeersRequest>,
    ) -> Result<Response<SearchPeersResponse>, Status> {
        let criteria = request.into_inner();
        let required_capabilities = criteria
            .required_capabilities
            .iter()
            .map(|capability| capability.trim().to_ascii_lowercase())
            .filter(|capability| !capability.is_empty())
            .collect::<Vec<_>>();
        let response = self
            .bridge
            .invoke("/lxmf.peers.v1.PeerService/SearchPeers", None, "list_peers", None)
            .await?;
        let payload: PeerListPayload = parse_result(response)?;
        let query = criteria.query.trim().to_ascii_lowercase();
        let peers = payload
            .peers
            .into_iter()
            .filter(|peer| !peer.peer.trim().is_empty())
            .map(Peer::from)
            .filter(|peer| {
                if criteria.alive_only && !peer.alive {
                    return false;
                }
                if !required_capabilities.is_empty()
                    && !required_capabilities.iter().all(|required| {
                        peer.capabilities
                            .iter()
                            .any(|capability| capability.to_ascii_lowercase() == *required)
                    })
                {
                    return false;
                }
                if query.is_empty() {
                    return true;
                }
                [
                    peer.peer_id.as_str(),
                    peer.name.as_deref().unwrap_or_default(),
                    peer.name_source.as_deref().unwrap_or_default(),
                    peer.peer_type.as_deref().unwrap_or_default(),
                ]
                .iter()
                .any(|field| field.to_ascii_lowercase().contains(&query))
            })
            .collect();
        Ok(Response::new(SearchPeersResponse { peers }))
    }

    async fn sync_peer(
        &self,
        request: Request<SyncPeerRequest>,
    ) -> Result<Response<SyncPeerResponse>, Status> {
        let peer = grpc_peer(&request);
        let peer_id = request.into_inner().peer_id.trim().to_string();
        if peer_id.is_empty() {
            return Err(Status::invalid_argument("peer_id is required"));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.peers.v1.PeerService/SyncPeer",
                peer,
                "peer_sync",
                Some(json!({ "peer": peer_id })),
            )
            .await?;
        let payload: SyncPeerPayload = parse_result(response)?;
        Ok(Response::new(SyncPeerResponse { peer_id: payload.peer, synced: payload.synced }))
    }

    async fn unpeer(
        &self,
        request: Request<UnpeerRequest>,
    ) -> Result<Response<UnpeerResponse>, Status> {
        let peer = grpc_peer(&request);
        let peer_id = request.into_inner().peer_id.trim().to_string();
        if peer_id.is_empty() {
            return Err(Status::invalid_argument("peer_id is required"));
        }
        let response = self
            .bridge
            .invoke(
                "/lxmf.peers.v1.PeerService/Unpeer",
                peer,
                "peer_unpeer",
                Some(json!({ "peer": peer_id })),
            )
            .await?;
        let payload: UnpeerPayload = parse_result(response)?;
        Ok(Response::new(UnpeerResponse { removed: payload.removed }))
    }

    async fn clear_peers(
        &self,
        request: Request<ClearPeersRequest>,
    ) -> Result<Response<ClearPeersResponse>, Status> {
        let response = self
            .bridge
            .invoke(
                "/lxmf.peers.v1.PeerService/ClearPeers",
                grpc_peer(&request),
                "clear_peers",
                None,
            )
            .await?;
        let payload: ClearPeersPayload = parse_result(response)?;
        Ok(Response::new(ClearPeersResponse { cleared: payload.cleared }))
    }
}

pub async fn serve(
    addr: SocketAddr,
    daemon: Arc<RpcDaemon>,
    tls: Option<GrpcTlsConfig>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bridge = GrpcBridge::new(daemon);
    let auth_interceptor = GrpcAuthInterceptor::new(bridge.daemon.clone());
    let runtime_service = RuntimeServiceServer::with_interceptor(
        RuntimeGrpcService { bridge: bridge.clone() },
        auth_interceptor.clone(),
    );
    let command_service = CommandServiceServer::with_interceptor(
        CommandGrpcService { bridge: bridge.clone() },
        auth_interceptor.clone(),
    );
    let delivery_service = DeliveryServiceServer::with_interceptor(
        DeliveryGrpcService { bridge: bridge.clone() },
        auth_interceptor.clone(),
    );
    let admin_service = InterfaceAdminServiceServer::with_interceptor(
        InterfaceAdminGrpcService { bridge: bridge.clone() },
        auth_interceptor.clone(),
    );
    let topic_service = TopicServiceServer::with_interceptor(
        TopicGrpcService { bridge: bridge.clone() },
        auth_interceptor.clone(),
    );
    let attachment_service = AttachmentServiceServer::with_interceptor(
        AttachmentGrpcService { bridge: bridge.clone() },
        auth_interceptor.clone(),
    );
    let event_service = EventServiceServer::with_interceptor(
        EventGrpcService { bridge: bridge.clone() },
        auth_interceptor.clone(),
    );
    let identity_service = IdentityServiceServer::with_interceptor(
        IdentityGrpcService { bridge: bridge.clone() },
        auth_interceptor.clone(),
    );
    let marker_service = MarkerServiceServer::with_interceptor(
        MarkerGrpcService { bridge: bridge.clone() },
        auth_interceptor,
    );
    let peer_service = PeerServiceServer::with_interceptor(
        PeerGrpcService { bridge: bridge.clone() },
        GrpcAuthInterceptor::new(bridge.daemon.clone()),
    );
    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()
        .map_err(|err| -> Box<dyn std::error::Error + Send + Sync> { Box::new(err) })?;
    let reflection_service = tonic::service::interceptor::InterceptedService::new(
        reflection_service,
        GrpcAuthInterceptor::new(bridge.daemon.clone()),
    );
    let mut server = tonic::transport::Server::builder();
    let grpc_scheme = if let Some(tls) = tls {
        server = server.tls_config(build_tls_config(&tls)?)?;
        "https"
    } else {
        "http"
    };
    println!("reticulumd gRPC listening on {}://{}", grpc_scheme, addr);
    server
        .add_service(reflection_service)
        .add_service(runtime_service)
        .add_service(command_service)
        .add_service(delivery_service)
        .add_service(admin_service)
        .add_service(topic_service)
        .add_service(attachment_service)
        .add_service(event_service)
        .add_service(identity_service)
        .add_service(marker_service)
        .add_service(peer_service)
        .serve(addr)
        .await
        .map_err(Into::into)
}

#[derive(Clone)]
struct GrpcAuthInterceptor {
    daemon: Arc<RpcDaemon>,
}

impl GrpcAuthInterceptor {
    fn new(daemon: Arc<RpcDaemon>) -> Self {
        Self { daemon }
    }
}

impl tonic::service::Interceptor for GrpcAuthInterceptor {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        authorize_grpc_request(self.daemon.as_ref(), &request)?;
        Ok(request)
    }
}

fn ensure_rpc_success(response: RpcResponse) -> Result<RpcResponse, Status> {
    match response.error {
        Some(error) => Err(map_rpc_error(&error)),
        None => Ok(response),
    }
}

fn parse_result<T>(response: RpcResponse) -> Result<T, Status>
where
    T: serde::de::DeserializeOwned,
{
    let result = response
        .result
        .ok_or_else(|| Status::internal("RPC response did not include a result payload"))?;
    serde_json::from_value(result)
        .map_err(|err| Status::internal(format!("failed to decode RPC result payload: {err}")))
}

fn map_io_error(error: &std::io::Error) -> Status {
    match error.kind() {
        std::io::ErrorKind::InvalidInput => Status::invalid_argument(error.to_string()),
        std::io::ErrorKind::TimedOut => Status::deadline_exceeded(error.to_string()),
        _ => Status::internal(error.to_string()),
    }
}

fn authorize_grpc_request(daemon: &RpcDaemon, request: &Request<()>) -> Result<(), Status> {
    let mut headers = Vec::new();
    for name in ["authorization", "x-forwarded-for", "x-real-ip"] {
        if let Some(value) = request.metadata().get(name) {
            let value = value.to_str().map_err(|_| {
                Status::invalid_argument(format!("gRPC metadata '{name}' is not valid ASCII"))
            })?;
            headers.push((name.to_string(), value.to_string()));
        }
    }
    let peer_ip = request.remote_addr().map(|addr| addr.ip().to_string());
    let transport_auth = transport_auth_from_request(request);
    daemon
        .authorize_http_request_with_transport(
            &headers,
            peer_ip.as_deref(),
            transport_auth.as_ref(),
        )
        .map_err(|error| map_rpc_error(&error))
}

fn map_rpc_error(error: &RpcError) -> Status {
    let status = match error.category.as_deref() {
        Some("Validation") => Status::invalid_argument(error.message.clone()),
        Some("Capability") | Some("Config") | Some("Policy") => {
            Status::failed_precondition(error.message.clone())
        }
        Some("Timeout") => Status::deadline_exceeded(error.message.clone()),
        Some("Security") => Status::permission_denied(error.message.clone()),
        Some("Storage") | Some("Transport") | Some("Runtime") | Some("Crypto") => {
            Status::unavailable(error.message.clone())
        }
        _ => Status::internal(error.message.clone()),
    };
    if let Some(machine_code) = error.machine_code.as_deref() {
        if machine_code == "UNSUPPORTED_MUTATION_KIND_REQUIRES_RESTART" {
            return Status::with_details(
                status.code(),
                status.message().to_string(),
                format_affected_interfaces(error),
            );
        }
    }
    status
}

fn restart_required(status: &Status) -> bool {
    status.code() == tonic::Code::FailedPrecondition && !status.details().is_empty()
}

fn negotiate_request_to_json(request: NegotiateRequest) -> Result<JsonValue, Status> {
    let config =
        request.config.ok_or_else(|| Status::invalid_argument("negotiate config is required"))?;
    if config.profile.trim().is_empty() {
        return Err(Status::invalid_argument("negotiate config.profile must not be empty"));
    }
    Ok(json!({
        "supported_contract_versions": request.supported_contract_versions,
        "requested_capabilities": request.requested_capabilities,
        "config": negotiate_runtime_config_to_json(config),
    }))
}

fn negotiate_runtime_config_to_json(config: NegotiateRuntimeConfig) -> JsonValue {
    json!({
        "profile": config.profile,
        "bind_mode": config.bind_mode,
        "auth_mode": config.auth_mode,
        "overflow_policy": config.overflow_policy,
        "block_timeout_ms": config.block_timeout_ms,
        "store_forward": config.store_forward.map(negotiate_store_forward_to_json),
        "rpc_backend": config.rpc_backend.map(negotiate_rpc_backend_to_json),
    })
}

fn negotiate_store_forward_to_json(config: NegotiateStoreForwardConfig) -> JsonValue {
    json!({
        "max_messages": config.max_messages,
        "max_message_age_ms": config.max_message_age_ms,
        "capacity_policy": config.capacity_policy,
        "eviction_priority": config.eviction_priority,
    })
}

fn negotiate_rpc_backend_to_json(config: NegotiateRpcBackendConfig) -> JsonValue {
    json!({
        "listen_addr": config.listen_addr,
        "read_timeout_ms": config.read_timeout_ms,
        "write_timeout_ms": config.write_timeout_ms,
        "max_header_bytes": config.max_header_bytes,
        "max_body_bytes": config.max_body_bytes,
        "token_auth": config.token_auth.map(negotiate_token_auth_to_json),
        "mtls_auth": config.mtls_auth.map(negotiate_mtls_auth_to_json),
    })
}

fn negotiate_token_auth_to_json(config: NegotiateTokenAuthConfig) -> JsonValue {
    json!({
        "issuer": config.issuer,
        "audience": config.audience,
        "jti_cache_ttl_ms": config.jti_cache_ttl_ms,
        "clock_skew_ms": config.clock_skew_ms,
        "shared_secret": config.shared_secret,
    })
}

fn negotiate_mtls_auth_to_json(config: NegotiateMtlsAuthConfig) -> JsonValue {
    json!({
        "ca_bundle_path": config.ca_bundle_path,
        "require_client_cert": config.require_client_cert,
        "allowed_san": config.allowed_san,
        "client_cert_path": config.client_cert_path,
        "client_key_path": config.client_key_path,
    })
}

fn extract_affected_interfaces(status: &Status) -> Vec<String> {
    let text = String::from_utf8_lossy(status.details());
    if text.is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
}

fn format_affected_interfaces(error: &RpcError) -> tonic::codegen::Bytes {
    let affected = error
        .details
        .as_deref()
        .and_then(|details| details.get("affected_interfaces"))
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    tonic::codegen::Bytes::from(serde_json::to_vec(&affected).unwrap_or_default())
}

impl From<RpcInterfaceRecord> for InterfaceRecord {
    fn from(value: RpcInterfaceRecord) -> Self {
        Self {
            r#type: value.kind,
            enabled: value.enabled,
            host: value.host,
            port: value.port.map(u32::from),
            name: value.name,
            settings: value.settings.and_then(json_to_struct),
        }
    }
}

impl TryFrom<InterfaceRecord> for RpcInterfaceRecord {
    type Error = Status;

    fn try_from(value: InterfaceRecord) -> Result<Self, Self::Error> {
        let port = value
            .port
            .map(|port| {
                u16::try_from(port)
                    .map_err(|_| Status::invalid_argument("interface port exceeds u16 range"))
            })
            .transpose()?;
        Ok(Self {
            kind: value.r#type,
            enabled: value.enabled,
            host: value.host,
            port,
            name: value.name,
            settings: value.settings.map(struct_to_json),
        })
    }
}

impl From<TopicRecordPayload> for Topic {
    fn from(value: TopicRecordPayload) -> Self {
        Self { topic_id: value.topic_id, topic_path: value.topic_path.unwrap_or_default() }
    }
}

impl From<DeliveryMessagePayload> for DeliveryMessageRecord {
    fn from(value: DeliveryMessagePayload) -> Self {
        Self {
            id: value.id,
            source: value.source,
            destination: value.destination,
            title: value.title,
            content: value.content,
            timestamp: value.timestamp,
            direction: value.direction,
            fields: value.fields.and_then(json_to_struct),
            receipt_status: value.receipt_status,
        }
    }
}

impl From<CommandSessionPayload> for CommandSessionProto {
    fn from(value: CommandSessionPayload) -> Self {
        Self {
            command_id: value.command_id,
            correlation_id: value.correlation_id,
            command: value.command,
            target: value.target,
            timeout_ms: value.timeout_ms,
            delivery_state: value.delivery_state,
            command_state: value.command_state,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
            request_payload: json_to_struct(value.request_payload),
            response_payload: value.response_payload.and_then(json_to_struct),
            accepted: value.accepted,
            extensions: json_to_struct(value.extensions),
        }
    }
}

impl From<AttachmentRecordPayload> for Attachment {
    fn from(value: AttachmentRecordPayload) -> Self {
        Self {
            attachment_id: value.attachment_id,
            name: value.name,
            content_type: value.content_type,
            byte_len: value.byte_len,
            checksum_sha256: value.checksum_sha256,
            created_ts_ms: value.created_ts_ms,
            expires_ts_ms: value.expires_ts_ms,
            topic_ids: value.topic_ids,
        }
    }
}

impl From<EventEnvelopePayload> for EventEnvelope {
    fn from(value: EventEnvelopePayload) -> Self {
        Self {
            event_type: value.event_type,
            payload: json_to_struct(value.payload),
            event_id: value.event_id.unwrap_or_default(),
            seq_no: value.seq_no.unwrap_or_default(),
            ts_ms: value.ts_ms.unwrap_or_default(),
            severity: value.severity.unwrap_or_default(),
            source_component: value.source_component.unwrap_or_default(),
        }
    }
}

impl From<crate::RpcEvent> for EventEnvelope {
    fn from(value: crate::RpcEvent) -> Self {
        Self {
            event_type: value.event_type,
            payload: json_to_struct(value.payload),
            event_id: String::new(),
            seq_no: 0,
            ts_ms: 0,
            severity: String::new(),
            source_component: "rns-rpc".to_string(),
        }
    }
}

impl From<IdentityBundlePayload> for Identity {
    fn from(value: IdentityBundlePayload) -> Self {
        Self {
            identity: value.identity,
            public_key: value.public_key,
            display_name: value.display_name,
            capabilities: value.capabilities,
        }
    }
}

impl From<ContactPayload> for Contact {
    fn from(value: ContactPayload) -> Self {
        Self {
            identity: value.identity,
            display_name: value.display_name,
            trust_level: value.trust_level,
            bootstrap: value.bootstrap,
            updated_ts_ms: value.updated_ts_ms,
            metadata: json_to_struct(value.metadata),
            extensions: json_to_struct(value.extensions),
        }
    }
}

impl From<PresenceRecordPayload> for PresenceRecord {
    fn from(value: PresenceRecordPayload) -> Self {
        Self {
            peer_id: value.peer_id,
            last_seen_ts_ms: value.last_seen_ts_ms,
            first_seen_ts_ms: value.first_seen_ts_ms,
            seen_count: value.seen_count,
            name: value.name,
            name_source: value.name_source,
            trust_level: value.trust_level,
            bootstrap: value.bootstrap,
            extensions: json_to_struct(value.extensions),
        }
    }
}

impl From<MarkerRecordPayload> for Marker {
    fn from(value: MarkerRecordPayload) -> Self {
        Self {
            marker_id: value.marker_id,
            label: value.label,
            position: Some(value.position.into()),
            topic_id: value.topic_id,
            revision: value.revision,
            updated_ts_ms: value.updated_ts_ms,
        }
    }
}

impl From<GeoPointPayload> for GeoPoint {
    fn from(value: GeoPointPayload) -> Self {
        Self { lat: value.lat, lon: value.lon, alt_m: value.alt_m }
    }
}

impl From<PeerRecordPayload> for Peer {
    fn from(value: PeerRecordPayload) -> Self {
        let capability_count = value.capabilities.len() as u32;
        Self {
            peer_id: value.peer,
            last_seen_ts_ms: value.last_seen,
            capabilities: value.capabilities,
            has_capabilities: capability_count > 0,
            capability_count,
            name: value.name,
            name_source: value.name_source,
            peer_type: value.peer_type,
            alive: value.alive,
            last_sync_attempt_ts_ms: value.last_sync_attempt,
            next_sync_attempt_ts_ms: value.next_sync_attempt,
            sync_backoff: value.sync_backoff,
            network_distance: value.network_distance,
            rx_bytes: value.rx_bytes,
            tx_bytes: value.tx_bytes,
            acceptance_rate: value.acceptance_rate,
            first_seen_ts_ms: value.first_seen,
            seen_count: value.seen_count,
        }
    }
}

fn geo_point_to_json(point: Option<GeoPoint>) -> JsonValue {
    let point = point.unwrap_or(GeoPoint { lat: 0.0, lon: 0.0, alt_m: None });
    json!({
        "lat": point.lat,
        "lon": point.lon,
        "alt_m": point.alt_m,
    })
}

fn json_to_struct(value: JsonValue) -> Option<prost_types::Struct> {
    match value {
        JsonValue::Object(fields) => Some(prost_types::Struct {
            fields: fields
                .into_iter()
                .map(|(key, value)| (key, json_to_prost_value(value)))
                .collect(),
        }),
        _ => None,
    }
}

fn struct_to_json(value: prost_types::Struct) -> JsonValue {
    JsonValue::Object(
        value.fields.into_iter().map(|(key, value)| (key, prost_value_to_json(value))).collect(),
    )
}

fn json_to_prost_value(value: JsonValue) -> prost_types::Value {
    use prost_types::value::Kind;

    prost_types::Value {
        kind: Some(match value {
            JsonValue::Null => Kind::NullValue(0),
            JsonValue::Bool(value) => Kind::BoolValue(value),
            JsonValue::Number(value) => Kind::NumberValue(value.as_f64().unwrap_or_default()),
            JsonValue::String(value) => Kind::StringValue(value),
            JsonValue::Array(values) => Kind::ListValue(prost_types::ListValue {
                values: values.into_iter().map(json_to_prost_value).collect(),
            }),
            JsonValue::Object(fields) => Kind::StructValue(prost_types::Struct {
                fields: fields
                    .into_iter()
                    .map(|(key, value)| (key, json_to_prost_value(value)))
                    .collect(),
            }),
        }),
    }
}

fn prost_value_to_json(value: prost_types::Value) -> JsonValue {
    use prost_types::value::Kind;

    match value.kind {
        Some(Kind::NullValue(_)) | None => JsonValue::Null,
        Some(Kind::BoolValue(value)) => JsonValue::Bool(value),
        Some(Kind::NumberValue(value)) => {
            serde_json::Number::from_f64(value).map_or(JsonValue::Null, JsonValue::Number)
        }
        Some(Kind::StringValue(value)) => JsonValue::String(value),
        Some(Kind::StructValue(value)) => struct_to_json(value),
        Some(Kind::ListValue(value)) => {
            JsonValue::Array(value.values.into_iter().map(prost_value_to_json).collect())
        }
    }
}

fn transport_auth_from_request(request: &Request<()>) -> Option<TransportAuthContext> {
    let peer_certs = request.peer_certs()?;
    let leaf = peer_certs.first()?;
    let (subject, sans) = parse_client_identity(leaf.as_ref());
    Some(TransportAuthContext {
        client_cert_present: true,
        client_subject: subject,
        client_sans: sans,
    })
}

fn parse_client_identity(cert_der: &[u8]) -> (Option<String>, Vec<String>) {
    let Ok((_remaining, cert)) = X509Certificate::from_der(cert_der) else {
        return (None, Vec::new());
    };
    let subject = cert
        .subject()
        .iter_common_name()
        .find_map(|name| name.as_str().ok().map(str::to_string))
        .or_else(|| Some(cert.subject().to_string()));
    let sans = parse_subject_alt_names(&cert);
    (subject, sans)
}

fn parse_subject_alt_names(cert: &X509Certificate<'_>) -> Vec<String> {
    let mut sans = Vec::new();
    for extension in cert.extensions() {
        if let ParsedExtension::SubjectAlternativeName(subject_alt_name) =
            extension.parsed_extension()
        {
            for name in &subject_alt_name.general_names {
                let value = match name {
                    GeneralName::DNSName(value) => Some((*value).to_string()),
                    GeneralName::URI(value) => Some((*value).to_string()),
                    GeneralName::RFC822Name(value) => Some((*value).to_string()),
                    GeneralName::IPAddress(raw) if raw.len() == 4 => {
                        Some(std::net::IpAddr::from([raw[0], raw[1], raw[2], raw[3]]).to_string())
                    }
                    GeneralName::IPAddress(raw) if raw.len() == 16 => {
                        let mut octets = [0_u8; 16];
                        octets.copy_from_slice(raw);
                        Some(std::net::IpAddr::from(octets).to_string())
                    }
                    _ => None,
                };
                if let Some(value) = value {
                    let value = value.trim();
                    if !value.is_empty() {
                        sans.push(value.to_string());
                    }
                }
            }
        }
    }
    sans
}

fn build_tls_config(
    config: &GrpcTlsConfig,
) -> Result<ServerTlsConfig, Box<dyn std::error::Error + Send + Sync>> {
    let cert = fs::read_to_string(config.cert_chain_path.as_path()).map_err(|err| {
        std::io::Error::other(format!(
            "read gRPC cert chain {}: {err}",
            config.cert_chain_path.display()
        ))
    })?;
    let key = fs::read_to_string(config.private_key_path.as_path()).map_err(|err| {
        std::io::Error::other(format!(
            "read gRPC private key {}: {err}",
            config.private_key_path.display()
        ))
    })?;

    let mut tls = ServerTlsConfig::new().identity(TlsIdentity::from_pem(cert, key));
    if let Some(client_ca_path) = config.client_ca_path.as_ref() {
        let client_ca = fs::read_to_string(client_ca_path.as_path()).map_err(|err| {
            std::io::Error::other(format!(
                "read gRPC client CA {}: {err}",
                client_ca_path.display()
            ))
        })?;
        tls = tls.client_ca_root(Certificate::from_pem(client_ca)).client_auth_optional(true);
    }
    Ok(tls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_restart_required_details_from_rpc_error() {
        let mut error = RpcError::new(
            "CONFIG_RESTART_REQUIRED",
            "requested interface mutation requires daemon restart",
        );
        error.machine_code = Some("UNSUPPORTED_MUTATION_KIND_REQUIRES_RESTART".to_string());
        error.category = Some("Config".to_string());
        error.details = Some(Box::new(serde_json::Map::from_iter([(
            "affected_interfaces".to_string(),
            json!(["primary", "backup"]),
        )])));

        let status = map_rpc_error(&error);
        assert!(restart_required(&status));
        assert_eq!(extract_affected_interfaces(&status), vec!["primary", "backup"]);
    }

    #[tokio::test]
    async fn negotiate_calls_sdk_negotiate_handler() {
        let service =
            RuntimeGrpcService { bridge: GrpcBridge::new(Arc::new(RpcDaemon::test_instance())) };
        let response = service
            .negotiate(Request::new(NegotiateRequest {
                supported_contract_versions: vec![2],
                requested_capabilities: Vec::new(),
                config: Some(NegotiateRuntimeConfig {
                    profile: "desktop-local-runtime".to_string(),
                    bind_mode: Some("local_only".to_string()),
                    auth_mode: Some("local_trusted".to_string()),
                    overflow_policy: Some("reject".to_string()),
                    block_timeout_ms: None,
                    store_forward: None,
                    rpc_backend: None,
                }),
            }))
            .await
            .expect("negotiate should succeed")
            .into_inner();

        assert_eq!(response.active_contract_version, 2);
        assert_eq!(response.runtime_id, "test-identity");
    }

    #[tokio::test]
    async fn runtime_status_and_propagation_diagnostics_map_raw_structs() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![RpcInterfaceRecord {
            kind: "tcp_client".to_string(),
            enabled: true,
            host: Some("rmap.world".to_string()),
            port: Some(4242),
            name: Some("primary".to_string()),
            settings: Some(json!({
                "_runtime": {
                    "startup_status": "active",
                    "iface": "tcp_client#0"
                }
            })),
        }]);
        daemon
            .handle_rpc(RpcRequest {
                id: 7,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "target_cost": 9,
                    "autopeer": true,
                    "max_peers": 20,
                })),
            })
            .expect("enable propagation");

        let service = RuntimeGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let status = service
            .get_daemon_status(Request::new(GetDaemonStatusRequest {}))
            .await
            .expect("daemon status")
            .into_inner();
        let status = struct_to_json(status.status.expect("status struct"));
        assert_eq!(status["interfaces"][0]["name"].as_str(), Some("primary"));
        assert_eq!(
            status["interfaces"][0]["settings"]["_runtime"]["startup_status"].as_str(),
            Some("active")
        );

        let propagation = service
            .get_propagation_status(Request::new(GetPropagationStatusRequest {}))
            .await
            .expect("propagation status")
            .into_inner();
        let propagation = struct_to_json(propagation.propagation.expect("propagation struct"));
        assert_eq!(propagation["enabled"].as_bool(), Some(true));
        assert_eq!(propagation["target_cost"].as_f64(), Some(9.0));
        assert_eq!(propagation["max_peers"].as_f64(), Some(20.0));
    }

    #[tokio::test]
    async fn set_propagation_updates_runtime_state() {
        let daemon = RpcDaemon::test_instance();
        let service = RuntimeGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let updated = service
            .set_propagation(Request::new(SetPropagationRequest {
                enabled: true,
                store_root: None,
                target_cost: Some(0),
                message_storage_limit_mb: None,
                autopeer: Some(true),
                autopeer_maxdepth: Some(6),
                static_peers: Vec::new(),
                max_peers: Some(20),
                from_static_only: Some(false),
                peering_cost: None,
                remote_peering_cost_max: None,
            }))
            .await
            .expect("set propagation")
            .into_inner();
        let propagation = struct_to_json(updated.propagation.expect("propagation struct"));
        assert_eq!(propagation["enabled"].as_bool(), Some(true));
        assert_eq!(propagation["autopeer"].as_bool(), Some(true));
        assert_eq!(propagation["max_peers"].as_f64(), Some(20.0));
    }

    #[tokio::test]
    async fn delivery_send_status_and_cancel_map_daemon_payloads() {
        let service =
            DeliveryGrpcService { bridge: GrpcBridge::new(Arc::new(RpcDaemon::test_instance())) };

        let sent = service
            .send(Request::new(SendRequest {
                id: "msg-1".to_string(),
                source: "node-a".to_string(),
                destination: "node-b".to_string(),
                title: "Test".to_string(),
                content: "hello".to_string(),
                fields: None,
                method: Some("direct".to_string()),
                stamp_cost: None,
                include_ticket: None,
                try_propagation_on_fail: None,
            }))
            .await
            .expect("send should succeed")
            .into_inner();
        assert_eq!(sent.message_id, "msg-1");

        let status = service
            .get_status(Request::new(GetStatusRequest { message_id: "msg-1".to_string() }))
            .await
            .expect("status should succeed")
            .into_inner();
        let message = status.message.expect("message record");
        assert_eq!(message.id, "msg-1");
        assert_eq!(message.destination, "node-b");
        assert_eq!(message.content, "hello");

        let cancelled = service
            .cancel(Request::new(CancelRequest { message_id: "msg-1".to_string() }))
            .await
            .expect("cancel should succeed")
            .into_inner();
        assert_eq!(cancelled.message_id, "msg-1");
        assert_eq!(cancelled.result, "TooLateToCancel");
    }

    #[tokio::test]
    async fn command_invoke_reply_get_and_list_sessions_map_payloads() {
        let daemon = RpcDaemon::test_instance();
        negotiate_remote_commands_capability(&daemon);
        let service = CommandGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let invoked = service
            .invoke_command(Request::new(InvokeCommandRequest {
                command: "status".to_string(),
                target: Some("peer-a".to_string()),
                payload: Some(json_to_struct(json!({"mode":"quick"})).expect("payload")),
                timeout_ms: Some(5_000),
                extensions: None,
            }))
            .await
            .expect("invoke command should succeed")
            .into_inner();
        assert!(invoked.accepted);
        let session = invoked.session.expect("session");
        assert_eq!(session.command, "status");
        assert_eq!(session.target.as_deref(), Some("peer-a"));

        let replied = service
            .reply_command(Request::new(ReplyCommandRequest {
                correlation_id: session.correlation_id.clone(),
                accepted: true,
                payload: Some(json_to_struct(json!({"ok":true})).expect("reply payload")),
                extensions: None,
            }))
            .await
            .expect("reply command should succeed")
            .into_inner();
        assert!(replied.accepted);
        assert!(replied.reply_accepted);

        let fetched = service
            .get_command_session(Request::new(GetCommandSessionRequest {
                correlation_id: session.correlation_id.clone(),
            }))
            .await
            .expect("get command session should succeed")
            .into_inner();
        let fetched_session = fetched.session.expect("session");
        assert_eq!(fetched_session.command_state, "completed");
        assert_eq!(fetched_session.accepted, Some(true));

        let listed = service
            .list_command_sessions(Request::new(ListCommandSessionsRequest {
                page: Some(lxmf::common::v1::PageRequest {
                    page_token: String::new(),
                    page_size: 10,
                }),
            }))
            .await
            .expect("list command sessions should succeed")
            .into_inner();
        assert_eq!(listed.sessions.len(), 1);
        assert_eq!(listed.sessions[0].correlation_id, session.correlation_id);
    }

    #[tokio::test]
    async fn list_interfaces_returns_daemon_interfaces() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![RpcInterfaceRecord {
            kind: "tcp_client".to_string(),
            enabled: true,
            host: Some("127.0.0.1".to_string()),
            port: Some(4242),
            name: Some("primary".to_string()),
            settings: None,
        }]);
        let service = InterfaceAdminGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let response = service
            .list_interfaces(Request::new(ListInterfacesRequest {}))
            .await
            .expect("list interfaces should succeed")
            .into_inner();

        assert_eq!(response.interfaces.len(), 1);
        assert_eq!(response.interfaces[0].name.as_deref(), Some("primary"));
    }

    #[tokio::test]
    async fn set_interfaces_reports_restart_required_for_non_legacy_kinds() {
        let service = InterfaceAdminGrpcService {
            bridge: GrpcBridge::new(Arc::new(RpcDaemon::test_instance())),
        };

        let response = service
            .set_interfaces(Request::new(SetInterfacesRequest {
                interfaces: vec![InterfaceRecord {
                    r#type: "ble".to_string(),
                    enabled: true,
                    host: None,
                    port: None,
                    name: Some("ble-main".to_string()),
                    settings: None,
                }],
            }))
            .await
            .expect("set interfaces should return response")
            .into_inner();

        assert!(response.restart_required);
        assert!(!response.updated);
        assert_eq!(response.affected_interfaces, vec!["ble-main"]);
    }

    #[tokio::test]
    async fn reload_config_surfaces_hot_apply_flag_from_daemon_result() {
        let daemon = RpcDaemon::test_instance();
        daemon.replace_interfaces(vec![RpcInterfaceRecord {
            kind: "tcp_client".to_string(),
            enabled: true,
            host: Some("127.0.0.1".to_string()),
            port: Some(4242),
            name: Some("primary".to_string()),
            settings: None,
        }]);
        let service = InterfaceAdminGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let response = service
            .reload_config(Request::new(ReloadConfigRequest {
                desired_interfaces: Some(lxmf::admin::v1::InterfaceSet {
                    interfaces: vec![InterfaceRecord {
                        r#type: "tcp_client".to_string(),
                        enabled: true,
                        host: Some("127.0.0.1".to_string()),
                        port: Some(4242),
                        name: Some("primary".to_string()),
                        settings: None,
                    }],
                }),
            }))
            .await
            .expect("reload config should succeed")
            .into_inner();

        assert!(response.reloaded);
        assert!(response.hot_applied_legacy_tcp_only);
        assert!(!response.restart_required);
    }

    #[tokio::test]
    async fn interface_settings_round_trip_through_proto_conversion() {
        let original = RpcInterfaceRecord {
            kind: "tcp_client".to_string(),
            enabled: true,
            host: Some("127.0.0.1".to_string()),
            port: Some(4242),
            name: Some("primary".to_string()),
            settings: Some(json!({
                "tls": { "enabled": true },
                "priority": 3,
            })),
        };

        let proto: InterfaceRecord = original.clone().into();
        let round_trip = RpcInterfaceRecord::try_from(proto).expect("convert back from proto");
        let settings = round_trip.settings.expect("settings should be preserved");
        assert_eq!(settings["tls"]["enabled"], json!(true));
        assert_eq!(settings["priority"], json!(3.0));
    }

    fn negotiate_topics_capability(daemon: &RpcDaemon) {
        daemon
            .handle_rpc(RpcRequest {
                id: 99,
                method: "sdk_negotiate_v2".to_string(),
                params: Some(json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [
                        "sdk.capability.topics",
                        "sdk.capability.topic_subscriptions",
                        "sdk.capability.topic_fanout"
                    ],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "local_only",
                        "auth_mode": "local_trusted",
                        "overflow_policy": "reject"
                    }
                })),
            })
            .expect("negotiate topics capability");
    }

    fn negotiate_remote_commands_capability(daemon: &RpcDaemon) {
        daemon
            .handle_rpc(RpcRequest {
                id: 98,
                method: "sdk_negotiate_v2".to_string(),
                params: Some(json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": ["sdk.capability.remote_commands"],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "local_only",
                        "auth_mode": "local_trusted",
                        "overflow_policy": "reject"
                    }
                })),
            })
            .expect("negotiate remote command capability");
    }

    fn negotiate_attachments_capability(daemon: &RpcDaemon) {
        daemon
            .handle_rpc(RpcRequest {
                id: 100,
                method: "sdk_negotiate_v2".to_string(),
                params: Some(json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [
                        "sdk.capability.attachments",
                        "sdk.capability.attachment_delete",
                        "sdk.capability.attachment_streaming"
                    ],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "local_only",
                        "auth_mode": "local_trusted",
                        "overflow_policy": "reject"
                    }
                })),
            })
            .expect("negotiate attachments capability");
    }

    fn negotiate_markers_capability(daemon: &RpcDaemon) {
        daemon
            .handle_rpc(RpcRequest {
                id: 101,
                method: "sdk_negotiate_v2".to_string(),
                params: Some(json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": ["sdk.capability.markers"],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "local_only",
                        "auth_mode": "local_trusted",
                        "overflow_policy": "reject"
                    }
                })),
            })
            .expect("negotiate markers capability");
    }

    #[test]
    fn authorize_grpc_request_blocks_remote_source_in_local_only_mode() {
        let daemon = RpcDaemon::test_instance();
        let mut request = Request::new(());
        request.extensions_mut().insert(tonic::transport::server::TcpConnectInfo {
            local_addr: None,
            remote_addr: Some("10.1.2.3:7777".parse().expect("remote addr")),
        });

        let error =
            authorize_grpc_request(&daemon, &request).expect_err("remote source must be blocked");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn authorize_grpc_request_enforces_token_metadata() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "sdk_negotiate_v2".to_string(),
                params: Some(json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": ["sdk.capability.token_auth"],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "remote",
                        "auth_mode": "token",
                        "overflow_policy": "reject",
                        "rpc_backend": {
                            "token_auth": {
                                "issuer": "lxmf-test",
                                "audience": "sdk-clients",
                                "jti_cache_ttl_ms": 60000,
                                "shared_secret": "test-secret",
                            }
                        }
                    }
                })),
            })
            .expect("negotiate token mode");

        let mut request = Request::new(());
        request.extensions_mut().insert(tonic::transport::server::TcpConnectInfo {
            local_addr: None,
            remote_addr: Some("10.5.6.7:7000".parse().expect("remote addr")),
        });

        let error = authorize_grpc_request(&daemon, &request)
            .expect_err("missing authorization metadata must be rejected");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn create_topic_maps_topic_payload() {
        let daemon = RpcDaemon::test_instance();
        negotiate_topics_capability(&daemon);
        let service = TopicGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let response = service
            .create_topic(Request::new(CreateTopicRequest { topic_path: "tak/alpha".to_string() }))
            .await
            .expect("create topic should succeed")
            .into_inner();

        let topic = response.topic.expect("topic");
        assert_eq!(topic.topic_path, "tak/alpha");
        assert!(topic.topic_id.starts_with("topic-"));
    }

    #[tokio::test]
    async fn topic_get_subscribe_publish_and_unsubscribe_map_payloads() {
        let daemon = RpcDaemon::test_instance();
        negotiate_topics_capability(&daemon);
        let service = TopicGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let created = service
            .create_topic(Request::new(CreateTopicRequest { topic_path: "tak/bravo".to_string() }))
            .await
            .expect("create topic should succeed")
            .into_inner();
        let topic = created.topic.expect("topic");

        let fetched = service
            .get_topic(Request::new(GetTopicRequest { topic_id: topic.topic_id.clone() }))
            .await
            .expect("get topic should succeed")
            .into_inner();
        assert_eq!(fetched.topic.expect("topic").topic_path, "tak/bravo");

        let subscribed = service
            .subscribe_topic(Request::new(SubscribeTopicRequest {
                topic_id: topic.topic_id.clone(),
                cursor: None,
            }))
            .await
            .expect("subscribe topic should succeed")
            .into_inner();
        assert!(subscribed.accepted);

        let published = service
            .publish_topic(Request::new(PublishTopicRequest {
                topic_id: topic.topic_id.clone(),
                payload: Some(
                    json_to_struct(json!({"kind":"test","value":1})).expect("payload struct"),
                ),
                correlation_id: Some("corr-1".to_string()),
            }))
            .await
            .expect("publish topic should succeed")
            .into_inner();
        assert!(published.accepted);

        let unsubscribed = service
            .unsubscribe_topic(Request::new(UnsubscribeTopicRequest {
                topic_id: topic.topic_id.clone(),
            }))
            .await
            .expect("unsubscribe topic should succeed")
            .into_inner();
        assert!(unsubscribed.accepted);
        assert_eq!(unsubscribed.topic_id, topic.topic_id);
    }

    #[tokio::test]
    async fn list_topics_maps_cursor_to_page_info() {
        let daemon = RpcDaemon::test_instance();
        negotiate_topics_capability(&daemon);
        let service = TopicGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        for name in ["one", "two", "three"] {
            service
                .create_topic(Request::new(CreateTopicRequest { topic_path: name.to_string() }))
                .await
                .expect("seed topic");
        }

        let response = service
            .list_topics(Request::new(ListTopicsRequest {
                page: Some(lxmf::common::v1::PageRequest {
                    page_token: String::new(),
                    page_size: 2,
                }),
            }))
            .await
            .expect("list topics should succeed")
            .into_inner();

        assert_eq!(response.topics.len(), 2);
        let page_info = response.page_info.expect("page info");
        assert!(!page_info.next_page_token.is_empty());
    }

    #[tokio::test]
    async fn store_attachment_maps_payload() {
        let daemon = RpcDaemon::test_instance();
        negotiate_attachments_capability(&daemon);
        let service = AttachmentGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let response = service
            .store_attachment(Request::new(StoreAttachmentRequest {
                name: "brief.txt".to_string(),
                content_type: "text/plain".to_string(),
                bytes_base64: "aGVsbG8=".to_string(),
                expires_ts_ms: None,
                topic_ids: Vec::new(),
            }))
            .await
            .expect("store attachment should succeed")
            .into_inner();

        let attachment = response.attachment.expect("attachment");
        assert_eq!(attachment.name, "brief.txt");
        assert_eq!(attachment.content_type, "text/plain");
        assert_eq!(attachment.byte_len, 5);
        assert!(attachment.attachment_id.starts_with("attachment-"));
    }

    #[tokio::test]
    async fn get_attachment_returns_record() {
        let daemon = RpcDaemon::test_instance();
        negotiate_attachments_capability(&daemon);
        let service = AttachmentGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let stored = service
            .store_attachment(Request::new(StoreAttachmentRequest {
                name: "report.txt".to_string(),
                content_type: "text/plain".to_string(),
                bytes_base64: "aGVsbG8=".to_string(),
                expires_ts_ms: None,
                topic_ids: Vec::new(),
            }))
            .await
            .expect("store attachment should succeed")
            .into_inner();

        let response = service
            .get_attachment(Request::new(GetAttachmentRequest {
                attachment_id: stored.attachment.expect("stored attachment").attachment_id,
            }))
            .await
            .expect("get attachment should succeed")
            .into_inner();

        assert_eq!(response.attachment.expect("attachment").name, "report.txt");
    }

    #[tokio::test]
    async fn list_attachments_maps_cursor_to_page_info() {
        let daemon = RpcDaemon::test_instance();
        negotiate_attachments_capability(&daemon);
        let service = AttachmentGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        for name in ["one.bin", "two.bin", "three.bin"] {
            service
                .store_attachment(Request::new(StoreAttachmentRequest {
                    name: name.to_string(),
                    content_type: "application/octet-stream".to_string(),
                    bytes_base64: "aGVsbG8=".to_string(),
                    expires_ts_ms: None,
                    topic_ids: Vec::new(),
                }))
                .await
                .expect("seed attachment");
        }

        let response = service
            .list_attachments(Request::new(ListAttachmentsRequest {
                topic_id: None,
                page: Some(lxmf::common::v1::PageRequest {
                    page_token: String::new(),
                    page_size: 2,
                }),
            }))
            .await
            .expect("list attachments should succeed")
            .into_inner();

        assert_eq!(response.attachments.len(), 2);
        let page_info = response.page_info.expect("page info");
        assert!(!page_info.next_page_token.is_empty());
    }

    #[tokio::test]
    async fn delete_attachment_returns_acceptance() {
        let daemon = RpcDaemon::test_instance();
        negotiate_attachments_capability(&daemon);
        let service = AttachmentGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let stored = service
            .store_attachment(Request::new(StoreAttachmentRequest {
                name: "temp.txt".to_string(),
                content_type: "text/plain".to_string(),
                bytes_base64: "aGVsbG8=".to_string(),
                expires_ts_ms: None,
                topic_ids: Vec::new(),
            }))
            .await
            .expect("store attachment should succeed")
            .into_inner();

        let response = service
            .delete_attachment(Request::new(DeleteAttachmentRequest {
                attachment_id: stored.attachment.expect("attachment").attachment_id,
            }))
            .await
            .expect("delete attachment should succeed")
            .into_inner();

        assert!(response.accepted);
        assert!(response.attachment_id.starts_with("attachment-"));
    }

    #[tokio::test]
    async fn download_attachment_returns_base64_payload() {
        let daemon = RpcDaemon::test_instance();
        negotiate_attachments_capability(&daemon);
        let service = AttachmentGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let stored = service
            .store_attachment(Request::new(StoreAttachmentRequest {
                name: "payload.txt".to_string(),
                content_type: "text/plain".to_string(),
                bytes_base64: "aGVsbG8=".to_string(),
                expires_ts_ms: None,
                topic_ids: Vec::new(),
            }))
            .await
            .expect("store attachment should succeed")
            .into_inner();

        let response = service
            .download_attachment(Request::new(DownloadAttachmentRequest {
                attachment_id: stored.attachment.expect("attachment").attachment_id,
            }))
            .await
            .expect("download attachment should succeed")
            .into_inner();

        assert!(response.accepted);
        assert_eq!(response.bytes_base64, "aGVsbG8=");
    }

    #[tokio::test]
    async fn attachment_streaming_round_trips_through_chunk_lifecycle() {
        let daemon = RpcDaemon::test_instance();
        negotiate_attachments_capability(&daemon);
        let service = AttachmentGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let upload = service
            .upload_start(Request::new(UploadStartRequest {
                name: "stream.bin".to_string(),
                content_type: "application/octet-stream".to_string(),
                total_size: 11,
                checksum_sha256: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
                    .to_string(),
                expires_ts_ms: None,
                topic_ids: Vec::new(),
            }))
            .await
            .expect("upload start should succeed")
            .into_inner()
            .upload
            .expect("upload handle");

        let chunk_1 = service
            .upload_chunk(Request::new(UploadChunkRequest {
                upload_id: upload.upload_id.clone(),
                offset: 0,
                bytes_base64: "aGVsbG8gd28=".to_string(),
            }))
            .await
            .expect("first upload chunk should succeed")
            .into_inner();
        assert!(chunk_1.accepted);
        assert_eq!(chunk_1.next_offset, 8);
        assert!(!chunk_1.complete);

        let chunk_2 = service
            .upload_chunk(Request::new(UploadChunkRequest {
                upload_id: upload.upload_id.clone(),
                offset: 8,
                bytes_base64: "cmxk".to_string(),
            }))
            .await
            .expect("second upload chunk should succeed")
            .into_inner();
        assert!(chunk_2.accepted);
        assert!(chunk_2.complete);

        let committed = service
            .upload_commit(Request::new(UploadCommitRequest { upload_id: upload.upload_id }))
            .await
            .expect("upload commit should succeed")
            .into_inner()
            .attachment
            .expect("attachment");

        let downloaded = service
            .download_chunk(Request::new(DownloadChunkRequest {
                attachment_id: committed.attachment_id,
                offset: Some(0),
                max_bytes: Some(5),
            }))
            .await
            .expect("download chunk should succeed")
            .into_inner();

        assert_eq!(downloaded.offset, 0);
        assert_eq!(downloaded.next_offset, 5);
        assert!(!downloaded.done);
        assert_eq!(downloaded.bytes_base64, "aGVsbG8=");
    }

    #[tokio::test]
    async fn poll_events_maps_rpc_event_rows() {
        let daemon = RpcDaemon::test_instance();
        let service = EventGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };
        service.bridge.daemon.publish_event(crate::RpcEvent {
            event_type: "sdk_attachment_stored".to_string(),
            payload: json!({ "attachment_id": "attachment-1" }),
        });

        let response = service
            .poll_events(Request::new(PollEventsRequest { cursor: None, max: 16 }))
            .await
            .expect("poll events should succeed")
            .into_inner();

        assert!(!response.events.is_empty());
        assert!(!response.next_cursor.is_empty());
        assert!(response.events.iter().any(|event| event.event_type == "sdk_attachment_stored"));
    }

    #[tokio::test]
    async fn subscribe_events_streams_live_broadcast_events() {
        let daemon = Arc::new(RpcDaemon::test_instance());
        let service = EventGrpcService { bridge: GrpcBridge::new(daemon.clone()) };
        let response = service
            .subscribe_events(Request::new(SubscribeEventsRequest { resume_token: None }))
            .await
            .expect("subscribe events should succeed");
        let mut stream = response.into_inner();

        daemon.publish_event(crate::RpcEvent {
            event_type: "sdk_topic_created".to_string(),
            payload: json!({ "topic_id": "topic-1" }),
        });

        let event = stream.next().await.expect("stream item").expect("event payload");
        assert_eq!(event.event_type, "sdk_topic_created");
        let payload = struct_to_json(event.payload.expect("payload"));
        assert_eq!(payload["topic_id"], json!("topic-1"));
    }

    fn negotiate_identity_capabilities(daemon: &RpcDaemon) {
        daemon
            .handle_rpc(RpcRequest {
                id: 101,
                method: "sdk_negotiate_v2".to_string(),
                params: Some(json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [
                        "sdk.capability.identity_multi",
                        "sdk.capability.identity_discovery",
                        "sdk.capability.identity_hash_resolution",
                        "sdk.capability.contact_management",
                        "sdk.capability.identity_import_export"
                    ],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "local_only",
                        "auth_mode": "local_trusted",
                        "overflow_policy": "reject"
                    }
                })),
            })
            .expect("negotiate identity capabilities");
    }

    #[tokio::test]
    async fn list_identities_and_resolve_identity_map_payloads() {
        let daemon = RpcDaemon::test_instance();
        negotiate_identity_capabilities(&daemon);
        let service = IdentityGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let imported = service
            .bridge
            .invoke(
                "/tests/identity/import",
                None,
                "sdk_identity_import_v2",
                Some(json!({
                    "bundle_base64": "eyJpZGVudGl0eSI6Im5vZGUtYiIsInB1YmxpY19rZXkiOiJub2RlLWItcHViIiwiZGlzcGxheV9uYW1lIjoiTm9kZSBCIiwiY2FwYWJpbGl0aWVzIjpbIm9wcyJdLCJleHRlbnNpb25zIjp7fX0="
                })),
            )
            .await
            .expect("import identity");
        let _: RpcResponse = imported;

        let listed = service
            .list_identities(Request::new(ListIdentitiesRequest {}))
            .await
            .expect("list identities")
            .into_inner();
        assert!(listed.identities.iter().any(|identity| identity.identity == "node-b"));

        let resolved = service
            .resolve_identity(Request::new(ResolveIdentityRequest {
                hash: "node-b-pub".to_string(),
            }))
            .await
            .expect("resolve identity")
            .into_inner();
        assert_eq!(resolved.identity.as_deref(), Some("node-b"));
    }

    #[tokio::test]
    async fn activate_import_and_export_identity_map_payloads() {
        let daemon = RpcDaemon::test_instance();
        negotiate_identity_capabilities(&daemon);
        let service = IdentityGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let imported = service
            .import_identity(Request::new(ImportIdentityRequest {
                bundle_base64: "eyJpZGVudGl0eSI6Im5vZGUtYyIsInB1YmxpY19rZXkiOiJub2RlLWMtcHViIiwiZGlzcGxheV9uYW1lIjoiTm9kZSBDIiwiY2FwYWJpbGl0aWVzIjpbIm9wcyJdLCJleHRlbnNpb25zIjp7fX0=".to_string(),
                passphrase: None,
            }))
            .await
            .expect("import identity should succeed")
            .into_inner();
        let imported_identity = imported.identity.expect("identity");
        assert_eq!(imported_identity.identity, "node-c");

        let activated = service
            .activate_identity(Request::new(ActivateIdentityRequest {
                identity: "node-c".to_string(),
            }))
            .await
            .expect("activate identity should succeed")
            .into_inner();
        assert!(activated.accepted);
        assert_eq!(activated.identity, "node-c");

        let exported = service
            .export_identity(Request::new(ExportIdentityRequest { identity: "node-c".to_string() }))
            .await
            .expect("export identity should succeed")
            .into_inner();
        let bundle = exported.bundle.expect("bundle");
        assert!(!bundle.bundle_base64.is_empty());
        assert!(bundle.passphrase.is_none());
    }

    #[tokio::test]
    async fn announce_now_returns_acceptance() {
        let daemon = RpcDaemon::test_instance();
        negotiate_identity_capabilities(&daemon);
        let service = IdentityGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let response = service
            .announce_now(Request::new(AnnounceNowRequest {}))
            .await
            .expect("announce now")
            .into_inner();

        assert!(response.accepted);
        assert!(response.announce_id > 0);
    }

    #[tokio::test]
    async fn contact_update_list_bootstrap_and_presence_map_correctly() {
        let daemon = RpcDaemon::test_instance();
        negotiate_identity_capabilities(&daemon);
        let service = IdentityGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        service
            .bridge
            .invoke(
                "/tests/identity/import",
                None,
                "sdk_identity_import_v2",
                Some(json!({
                    "bundle_base64": "eyJpZGVudGl0eSI6Im5vZGUtYiIsInB1YmxpY19rZXkiOiJub2RlLWItcHViIiwiZGlzcGxheV9uYW1lIjoiTm9kZSBCIiwiY2FwYWJpbGl0aWVzIjpbIm9wcyJdLCJleHRlbnNpb25zIjp7fX0="
                })),
            )
            .await
            .expect("import identity");

        let updated = service
            .update_contact(Request::new(UpdateContactRequest {
                identity: "node-b".to_string(),
                display_name: Some("Node Bravo".to_string()),
                trust_level: Some("untrusted".to_string()),
                bootstrap: Some(false),
                metadata: Some(json_to_struct(json!({"source":"manual"})).expect("metadata")),
                extensions: None,
            }))
            .await
            .expect("update contact")
            .into_inner();
        assert_eq!(updated.contact.as_ref().expect("contact").trust_level, "untrusted");

        let contacts = service
            .list_contacts(Request::new(ListContactsRequest {
                page: Some(lxmf::common::v1::PageRequest {
                    page_token: String::new(),
                    page_size: 10,
                }),
            }))
            .await
            .expect("list contacts")
            .into_inner();
        assert!(contacts.contacts.iter().any(|contact| contact.identity == "node-b"));

        let bootstrapped = service
            .bootstrap_identity(Request::new(BootstrapIdentityRequest {
                identity: "node-b".to_string(),
                auto_sync: true,
            }))
            .await
            .expect("bootstrap identity")
            .into_inner();
        assert!(bootstrapped.synced);
        assert_eq!(bootstrapped.contact.as_ref().expect("contact").trust_level, "trusted");
        assert!(bootstrapped.contact.as_ref().expect("contact").bootstrap);

        let presence = service
            .list_presence(Request::new(ListPresenceRequest {
                page: Some(lxmf::common::v1::PageRequest {
                    page_token: String::new(),
                    page_size: 10,
                }),
            }))
            .await
            .expect("list presence")
            .into_inner();
        assert!(presence.peers.iter().any(|peer| {
            peer.peer_id == "node-b"
                && peer.trust_level.as_deref() == Some("trusted")
                && peer.bootstrap == Some(true)
        }));
    }

    #[tokio::test]
    async fn create_and_list_markers_map_payloads() {
        let daemon = RpcDaemon::test_instance();
        negotiate_markers_capability(&daemon);
        let service = MarkerGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let created = service
            .create_marker(Request::new(CreateMarkerRequest {
                label: "alpha".to_string(),
                position: Some(GeoPoint { lat: 45.4215, lon: -75.6972, alt_m: Some(70.0) }),
                topic_id: None,
            }))
            .await
            .expect("create marker")
            .into_inner();
        let marker = created.marker.expect("marker");
        assert_eq!(marker.label, "alpha");
        assert_eq!(marker.revision, 1);

        let listed = service
            .list_markers(Request::new(ListMarkersRequest {
                topic_id: None,
                page: Some(lxmf::common::v1::PageRequest {
                    page_token: String::new(),
                    page_size: 10,
                }),
            }))
            .await
            .expect("list markers")
            .into_inner();
        assert!(listed.markers.iter().any(|candidate| candidate.marker_id == marker.marker_id));
    }

    #[tokio::test]
    async fn update_and_delete_marker_map_payloads() {
        let daemon = RpcDaemon::test_instance();
        negotiate_markers_capability(&daemon);
        let service = MarkerGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let created = service
            .create_marker(Request::new(CreateMarkerRequest {
                label: "bravo".to_string(),
                position: Some(GeoPoint { lat: 45.0, lon: -75.0, alt_m: None }),
                topic_id: None,
            }))
            .await
            .expect("create marker")
            .into_inner()
            .marker
            .expect("marker");

        let updated = service
            .update_marker_position(Request::new(UpdateMarkerPositionRequest {
                marker_id: created.marker_id.clone(),
                expected_revision: created.revision,
                position: Some(GeoPoint { lat: 46.0, lon: -76.0, alt_m: Some(10.0) }),
            }))
            .await
            .expect("update marker")
            .into_inner()
            .marker
            .expect("marker");
        assert_eq!(updated.revision, 2);
        assert_eq!(updated.position.as_ref().map(|point| point.lat), Some(46.0));

        let deleted = service
            .delete_marker(Request::new(DeleteMarkerRequest {
                marker_id: updated.marker_id.clone(),
                expected_revision: updated.revision,
            }))
            .await
            .expect("delete marker")
            .into_inner();
        assert!(deleted.accepted);
        assert_eq!(deleted.marker_id, updated.marker_id);
    }

    #[tokio::test]
    async fn list_peers_maps_peer_payloads() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 200,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": "peer-a" })),
            })
            .expect("sync peer");
        let service = PeerGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let response = service
            .list_peers(Request::new(ListPeersRequest {}))
            .await
            .expect("list peers")
            .into_inner();
        assert_eq!(response.peers.len(), 1);
        assert_eq!(response.peers[0].peer_id, "peer-a");
        assert_eq!(response.peers[0].peer_type.as_deref(), Some("manual"));
    }

    #[tokio::test]
    async fn sync_unpeer_and_clear_peers_map_results() {
        let daemon = RpcDaemon::test_instance();
        let service = PeerGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let synced = service
            .sync_peer(Request::new(SyncPeerRequest { peer_id: "peer-b".to_string() }))
            .await
            .expect("sync peer")
            .into_inner();
        assert!(synced.synced);
        assert_eq!(synced.peer_id, "peer-b");

        let unpeered = service
            .unpeer(Request::new(UnpeerRequest { peer_id: "peer-b".to_string() }))
            .await
            .expect("unpeer")
            .into_inner();
        assert!(unpeered.removed);

        let cleared = service
            .clear_peers(Request::new(ClearPeersRequest {}))
            .await
            .expect("clear peers")
            .into_inner();
        assert_eq!(cleared.cleared, "peers");
    }

    #[tokio::test]
    async fn search_peers_filters_live_peer_table() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .accept_announce_with_metadata(
                "peer-rmap".to_string(),
                210,
                Some("rmap.world".to_string()),
                Some("announce".to_string()),
                None,
                Some(vec!["propagation".to_string(), "ops".to_string()]),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept announce");
        daemon
            .accept_announce_with_metadata(
                "peer-other".to_string(),
                211,
                Some("other".to_string()),
                Some("announce".to_string()),
                None,
                Some(vec!["chat".to_string()]),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept announce");
        let service = PeerGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let response = service
            .search_peers(Request::new(SearchPeersRequest {
                query: "rmap".to_string(),
                alive_only: true,
                required_capabilities: vec!["propagation".to_string()],
            }))
            .await
            .expect("search peers")
            .into_inner();
        assert_eq!(response.peers.len(), 1);
        assert_eq!(response.peers[0].peer_id, "peer-rmap");
        assert!(response.peers[0].alive);
        assert!(response.peers[0]
            .capabilities
            .iter()
            .any(|capability| capability == "propagation"));
    }

    #[tokio::test]
    async fn sync_and_unpeer_reject_blank_peer_ids() {
        let daemon = RpcDaemon::test_instance();
        let service = PeerGrpcService { bridge: GrpcBridge::new(Arc::new(daemon)) };

        let sync_err = service
            .sync_peer(Request::new(SyncPeerRequest { peer_id: "   ".to_string() }))
            .await
            .expect_err("blank peer_id should be rejected");
        assert_eq!(sync_err.code(), tonic::Code::InvalidArgument);

        let unpeer_err = service
            .unpeer(Request::new(UnpeerRequest { peer_id: "".to_string() }))
            .await
            .expect_err("blank peer_id should be rejected");
        assert_eq!(unpeer_err.code(), tonic::Code::InvalidArgument);
    }
}

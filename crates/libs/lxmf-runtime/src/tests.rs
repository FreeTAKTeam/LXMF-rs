use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use lxmf_core::{MessageMethod, TransportMethod};
use lxmf_sdk::{
    Client, DeliveryState, LxmfSdk, MessageId, RouterStoragePolicyPatch, SdkBackend, SdkConfig,
    SendRequest, Severity, StartRequest,
};
use rand_core::OsRng;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::hash::{AddressHash, Hash};
use rns_transport::resource::{ResourceEvent, ResourceEventKind};
use rns_transport::transport::{Transport, TransportConfig};
use serde_json::json;
use tokio::sync::broadcast;

use crate::delivery::{
    forced_representation, request_link_attempts, request_link_timeout, request_resource_timeout,
    requested_method, resolve_destination,
};
use crate::link_delivery::{await_resource_completion, await_resource_completion_with_cancel};
use crate::{
    InProcessBackend, InProcessBackendConfig, EXT_ACCEPTED_RESULT_ACK,
    EXT_DIRECT_PACKET_MAX_WIRE_BYTES, EXT_LINK_CONNECT_TIMEOUT_MS,
};

fn request() -> SendRequest {
    SendRequest::new(
        "00112233445566778899aabbccddeeff",
        "ffeeddccbbaa99887766554433221100",
        json!({"content_base64": "aGVsbG8="}),
    )
}

#[test]
fn rnode_wire_budget_forces_direct_resource_representation() {
    let mut request = request();
    request.extensions =
        BTreeMap::from([(EXT_DIRECT_PACKET_MAX_WIRE_BYTES.to_owned(), json!(145))]);

    assert_eq!(
        forced_representation(&request, TransportMethod::Direct, MessageMethod::Packet, 146,),
        MessageMethod::Resource
    );
    assert_eq!(
        forced_representation(&request, TransportMethod::Direct, MessageMethod::Packet, 145,),
        MessageMethod::Packet
    );
}

#[test]
fn explicit_unknown_delivery_method_is_rejected() {
    let mut request = request();
    request.delivery_method = Some("teleport".to_owned());

    let error = requested_method(&request).expect_err("unknown mode must fail");
    assert_eq!(error.category, lxmf_sdk::ErrorCategory::Validation);
}

#[test]
fn accepted_result_uses_short_link_timeout_and_normal_resource_timeout() {
    let mut request = request();
    request.extensions.insert(EXT_ACCEPTED_RESULT_ACK.to_owned(), json!(true));
    assert_eq!(request_link_timeout(&request, Duration::from_secs(20)), Duration::from_secs(5));
    assert_eq!(request_link_attempts(&request, 3), 1);
    assert_eq!(
        request_resource_timeout(&request, Duration::from_secs(120)),
        Duration::from_secs(120)
    );

    request.extensions.insert(EXT_LINK_CONNECT_TIMEOUT_MS.to_owned(), json!(75_000));
    assert_eq!(request_link_timeout(&request, Duration::from_secs(20)), Duration::from_secs(75));
}

#[tokio::test]
async fn resource_wait_failure_runs_cancellation_before_returning() {
    let (_sender, mut receiver) = broadcast::channel(2);
    let expected_hash = Hash::new_from_slice(b"expected");
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation_observer = cancelled.clone();

    let error = await_resource_completion_with_cancel(
        &mut receiver,
        expected_hash,
        Duration::from_millis(10),
        async move {
            cancellation_observer.store(true, Ordering::SeqCst);
            Ok(())
        },
    )
    .await
    .expect_err("timed-out resource must fail");

    assert_eq!(error.category, lxmf_sdk::ErrorCategory::Transport);
    assert!(cancelled.load(Ordering::SeqCst));
}

#[tokio::test]
async fn resource_wait_success_does_not_cancel_completed_transfer() {
    let (sender, mut receiver) = broadcast::channel(2);
    let expected_hash = Hash::new_from_slice(b"expected");
    sender
        .send(ResourceEvent {
            hash: expected_hash,
            link_id: AddressHash::new_from_slice(b"link"),
            kind: ResourceEventKind::OutboundComplete,
        })
        .expect("queue completion");
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation_observer = cancelled.clone();

    await_resource_completion_with_cancel(
        &mut receiver,
        expected_hash,
        Duration::from_secs(1),
        async move {
            cancellation_observer.store(true, Ordering::SeqCst);
            Ok(())
        },
    )
    .await
    .expect("matching completion succeeds");

    assert!(!cancelled.load(Ordering::SeqCst));
}

#[tokio::test]
async fn resource_wait_surfaces_cancellation_failure_with_transfer_error() {
    let (_sender, mut receiver) = broadcast::channel(2);
    let expected_hash = Hash::new_from_slice(b"expected");

    let error = await_resource_completion_with_cancel(
        &mut receiver,
        expected_hash,
        Duration::from_millis(10),
        async { Err(crate::delivery::transport_error("cancel dispatch failed")) },
    )
    .await
    .expect_err("failed cleanup must remain visible");

    assert_eq!(error.category, lxmf_sdk::ErrorCategory::Transport);
    assert!(error.message.contains("resource transfer timed out"));
    assert!(error.message.contains("cleanup failed: cancel dispatch failed"));
}

#[tokio::test]
async fn missing_destination_identity_requests_path_before_failing() {
    let identity = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new(
        "lxmf-runtime-identity-resolution-test",
        &identity,
        true,
    ));
    let mut iface = transport.iface_manager().lock().await.new_channel(4);
    let unknown = AddressHash::new([0x44; rns_transport::hash::ADDRESS_HASH_SIZE]);

    let error =
        match resolve_destination(&transport, unknown, "delivery", Duration::from_millis(55)).await
        {
            Ok(_) => panic!("unknown destination must remain unavailable"),
            Err(error) => error,
        };

    assert_eq!(error.category, lxmf_sdk::ErrorCategory::Transport);
    assert!(error.message.contains("after path resolution"));
    let path_request = iface.tx_channel.try_recv().expect("path request must be broadcast");
    assert_eq!(
        &path_request.packet.data.as_slice()[..rns_transport::hash::ADDRESS_HASH_SIZE],
        unknown.as_slice()
    );
}

#[tokio::test]
async fn resource_send_only_succeeds_after_matching_outbound_completion() {
    let (sender, mut receiver) = broadcast::channel(4);
    let expected_hash = Hash::new_from_slice(b"expected");
    sender
        .send(ResourceEvent {
            hash: Hash::new_from_slice(b"other"),
            link_id: AddressHash::new_from_slice(b"link"),
            kind: ResourceEventKind::OutboundComplete,
        })
        .expect("queue unrelated completion");
    sender
        .send(ResourceEvent {
            hash: expected_hash,
            link_id: AddressHash::new_from_slice(b"link"),
            kind: ResourceEventKind::OutboundComplete,
        })
        .expect("queue matching completion");

    await_resource_completion(&mut receiver, expected_hash, Duration::from_secs(1))
        .await
        .expect("matching completion acknowledges resource send");
}

#[tokio::test]
async fn resource_failure_is_not_reported_as_success() {
    let (sender, mut receiver) = broadcast::channel(2);
    let expected_hash = Hash::new_from_slice(b"expected");
    sender
        .send(ResourceEvent {
            hash: expected_hash,
            link_id: AddressHash::new_from_slice(b"link"),
            kind: ResourceEventKind::OutboundFailed,
        })
        .expect("queue failure");

    let error = await_resource_completion(&mut receiver, expected_hash, Duration::from_secs(1))
        .await
        .expect_err("failed resource must fail send");
    assert_eq!(error.category, lxmf_sdk::ErrorCategory::Transport);
}

#[tokio::test]
async fn delivery_updates_drive_status_snapshot_and_events() {
    let identity = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let transport = std::sync::Arc::new(Transport::new(TransportConfig::new(
        "lxmf-runtime-state-test",
        &identity,
        true,
    )));
    let source =
        SingleInputDestination::new(identity.clone(), DestinationName::new("lxmf", "delivery"))
            .desc
            .address_hash;
    let backend = InProcessBackend::new(InProcessBackendConfig::new(
        "runtime-test",
        tokio::runtime::Handle::current(),
        transport,
        identity,
        source,
    ));
    let message_id = MessageId("message-1".to_owned());

    backend
        .record_delivery(&message_id, DeliveryState::InFlight, None)
        .expect("record in-flight delivery");
    assert_eq!(backend.snapshot().expect("snapshot").in_flight_messages, 1);

    backend
        .record_delivery(&message_id, DeliveryState::Delivered, None)
        .expect("record delivered state");
    let status = backend.status(message_id).expect("status query").expect("delivery status");
    assert!(status.terminal);
    assert_eq!(status.state, DeliveryState::Delivered);
    assert_eq!(backend.snapshot().expect("snapshot").in_flight_messages, 0);
    backend
        .record_event("reticulum.packet_received", Severity::Info, json!({"bytes": 3}))
        .expect("record transport event");
    let events = backend.poll_events(None, 16).expect("events").events;
    assert_eq!(events.len(), 3);
    assert_eq!(events[2].event_type, "reticulum.packet_received");
}

#[tokio::test]
async fn sdk_client_starts_with_in_process_backend_capability_contract() {
    let identity = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let transport = std::sync::Arc::new(Transport::new(TransportConfig::new(
        "lxmf-runtime-lifecycle-test",
        &identity,
        true,
    )));
    let source =
        SingleInputDestination::new(identity.clone(), DestinationName::new("lxmf", "delivery"))
            .desc
            .address_hash;
    let backend = InProcessBackend::new(InProcessBackendConfig::new(
        "runtime-lifecycle-test",
        tokio::runtime::Handle::current(),
        transport,
        identity,
        source,
    ));
    let client = Client::new(backend);
    let mut config = SdkConfig::desktop_local_default();
    config.rpc_backend = None;

    let handle = client
        .start(StartRequest::new(config))
        .expect("in-process backend must satisfy the desktop local runtime contract");

    assert!(handle
        .effective_capabilities
        .iter()
        .any(|capability| capability == "sdk.capability.idempotency_ttl"));
}

#[tokio::test]
async fn in_process_backend_implements_typed_router_management_contract() {
    let identity = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let transport = std::sync::Arc::new(Transport::new(TransportConfig::new(
        "router-management-test",
        &identity,
        true,
    )));
    let source =
        SingleInputDestination::new(identity.clone(), DestinationName::new("lxmf", "delivery"))
            .desc
            .address_hash;
    let backend = InProcessBackend::new(InProcessBackendConfig::new(
        "router-management-test",
        tokio::runtime::Handle::current(),
        transport,
        identity,
        source,
    ));
    let policy = backend
        .set_router_storage_policy(RouterStoragePolicyPatch {
            message_limit_bytes: Some(2_000_000),
            information_limit_bytes: Some(500_000),
            retain_node_lxms: Some(true),
        })
        .expect("set router policy");
    assert_eq!(policy.message_limit_bytes, Some(2_000_000));
    assert!(policy.retain_node_lxms);
    assert_eq!(backend.router_stats().expect("stats").storage_policy, policy);
}

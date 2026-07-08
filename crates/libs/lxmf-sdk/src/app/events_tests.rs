use super::{map_sdk_event, EventKind, SubscriptionStart};
use crate::{SdkEvent, Severity as RawSeverity, SubscriptionStart as RawSubscriptionStart};
use serde_json::json;
use std::collections::BTreeMap;

fn base_event(event_type: &str, payload: serde_json::Value) -> SdkEvent {
    SdkEvent {
        event_id: "evt-1".to_owned(),
        runtime_id: "rt-1".to_owned(),
        stream_id: "stream-1".to_owned(),
        seq_no: 1,
        contract_version: 2,
        ts_ms: 10,
        event_type: event_type.to_owned(),
        severity: RawSeverity::Info,
        source_component: "test".to_owned(),
        operation_id: None,
        message_id: None,
        peer_id: None,
        correlation_id: None,
        trace_id: None,
        payload,
        extensions: BTreeMap::new(),
    }
}

#[test]
fn maps_runtime_state_change_to_started() {
    let mapped = map_sdk_event(
        base_event("RuntimeStateChanged", json!({ "from": "starting", "to": "running" })),
        "desktop_default",
    );
    assert!(matches!(mapped.kind, EventKind::RuntimeStarted));
}

#[test]
fn maps_stream_gap_to_typed_gap_event() {
    let mapped = map_sdk_event(
        base_event(
            "StreamGap",
            json!({ "expected_seq_no": 2, "observed_seq_no": 7, "dropped_count": 5 }),
        ),
        "desktop_default",
    );
    match mapped.kind {
        EventKind::StreamGapDetected(details) => {
            assert_eq!(details.expected_seq_no, Some(2));
            assert_eq!(details.observed_seq_no, Some(7));
            assert_eq!(details.dropped_count, 5);
            assert!(details.recovery_required);
        }
        other => panic!("expected stream gap event, got {other:?}"),
    }
}

#[test]
fn subscription_start_round_trips() {
    let raw: RawSubscriptionStart = SubscriptionStart::Tail.into();
    assert_eq!(SubscriptionStart::from(raw), SubscriptionStart::Tail);
}

#[test]
fn maps_runtime_degraded_and_reconnect_events() {
    let degraded = map_sdk_event(base_event("RuntimeDegraded", json!({})), "desktop_default");
    let reconnect = map_sdk_event(
        base_event("ReconnectScheduled", json!({ "delay_ms": 500 })),
        "desktop_default",
    );
    let recovered = map_sdk_event(base_event("RuntimeRecovered", json!({})), "desktop_default");

    assert!(matches!(degraded.kind, EventKind::RuntimeDegraded));
    assert!(matches!(reconnect.kind, EventKind::ReconnectScheduled));
    assert!(matches!(recovered.kind, EventKind::RuntimeRecovered));
}

#[test]
fn maps_discovery_events() {
    let announced = map_sdk_event(
        base_event("announce_received", json!({ "peer": "peer-a" })),
        "desktop_default",
    );
    let peer_sync =
        map_sdk_event(base_event("peer_sync", json!({ "peer": "peer-a" })), "desktop_default");
    let contact_update = map_sdk_event(
        base_event("contact_updated", json!({ "identity": "peer-a" })),
        "desktop_default",
    );

    assert!(matches!(announced.kind, EventKind::AnnounceReceived));
    assert!(matches!(peer_sync.kind, EventKind::PeerDiscovered));
    assert!(matches!(contact_update.kind, EventKind::ContactUpdated));
    assert_eq!(announced.metadata.peer_id.as_deref(), Some("peer-a"));
    assert_eq!(peer_sync.metadata.peer_id.as_deref(), Some("peer-a"));
    assert_eq!(contact_update.metadata.peer_id.as_deref(), Some("peer-a"));
}

#[test]
fn maps_command_domain_events() {
    let dispatched = map_sdk_event(
        base_event("command.dispatched", json!({ "correlation_id": "cmd-1", "target": "peer-a" })),
        "desktop_default",
    );
    let completed = map_sdk_event(
        base_event("command.completed", json!({ "correlation_id": "cmd-1", "target": "peer-a" })),
        "desktop_default",
    );
    let failed = map_sdk_event(
        base_event("command.failed", json!({ "correlation_id": "cmd-1", "target": "peer-a" })),
        "desktop_default",
    );

    assert!(matches!(dispatched.kind, EventKind::CommandDispatched));
    assert!(matches!(completed.kind, EventKind::CommandCompleted));
    assert!(matches!(failed.kind, EventKind::CommandFailed));
    assert_eq!(dispatched.metadata.peer_id.as_deref(), Some("peer-a"));
}

#[test]
fn maps_inbound_success_to_typed_details() {
    let mapped = map_sdk_event(
        base_event(
            "inbound",
            json!({
                "delivery_kind": "packet",
                "lxmf_bytes_hex": "aabbcc",
                "message": {
                    "id": "msg-1",
                    "source": "src-hash",
                    "destination": "dst-hash",
                    "receipt_status": "received",
                    "fields": {
                        "_lxmf": {
                            "signature_checked": true,
                            "signature_valid": true,
                            "signature_status": "verified",
                            "stamp_checked": true,
                            "stamp_valid": true,
                            "stamp_status": "accepted",
                            "propagation_stamp_checked": true,
                            "propagation_stamp_valid": false,
                            "method": 2,
                            "transport_encrypted": true,
                            "transport_encryption": "Curve25519"
                        }
                    }
                }
            }),
        ),
        "desktop_default",
    );

    assert!(matches!(mapped.kind, EventKind::InboundMessageReceived));
    assert_eq!(mapped.metadata.message_id.as_deref(), Some("msg-1"));
    assert_eq!(mapped.metadata.peer_id.as_deref(), Some("src-hash"));
    let details = mapped.inbound_message_details().expect("inbound details");
    assert_eq!(details.message_id.as_deref(), Some("msg-1"));
    assert_eq!(details.source_hash.as_deref(), Some("src-hash"));
    assert_eq!(details.destination_hash.as_deref(), Some("dst-hash"));
    assert_eq!(details.delivery_kind.as_deref(), Some("packet"));
    assert_eq!(details.lxmf_bytes_hex.as_deref(), Some("aabbcc"));
    assert_eq!(details.receipt_status.as_deref(), Some("received"));
    assert_eq!(details.signature_checked, Some(true));
    assert_eq!(details.signature_valid, Some(true));
    assert_eq!(details.signature_status.as_deref(), Some("verified"));
    assert_eq!(details.stamp_checked, Some(true));
    assert_eq!(details.stamp_valid, Some(true));
    assert_eq!(details.stamp_status.as_deref(), Some("accepted"));
    assert_eq!(details.propagation_stamp_checked, Some(true));
    assert_eq!(details.propagation_stamp_valid, Some(false));
    assert_eq!(details.method, Some(2));
    assert_eq!(details.transport_encrypted, Some(true));
    assert_eq!(details.transport_encryption.as_deref(), Some("Curve25519"));
}

#[test]
fn maps_inbound_drop_to_typed_details() {
    let mapped = map_sdk_event(
        base_event(
            "inbound_dropped",
            json!({
                "reason": "payload_too_short",
                "delivery_kind": "propagation",
                "raw_destination_hash": "sha256:raw",
                "resolved_destination_hash": "sha256:resolved",
                "source_hash": "sha256:source",
                "destination_hash": "sha256:destination",
                "dropped_message_id": "msg-drop-1",
                "payload_mode": "full_wire",
                "bytes_len": 24,
                "detail": "propagated LXMF payload too short"
            }),
        ),
        "desktop_default",
    );

    assert!(matches!(mapped.kind, EventKind::InboundMessageDropped));
    assert_eq!(mapped.metadata.message_id.as_deref(), Some("msg-drop-1"));
    assert_eq!(mapped.metadata.peer_id.as_deref(), Some("sha256:source"));
    let details = mapped.inbound_drop_details().expect("drop details");
    assert_eq!(details.reason.as_deref(), Some("payload_too_short"));
    assert_eq!(details.delivery_kind.as_deref(), Some("propagation"));
    assert_eq!(details.raw_destination_hash.as_deref(), Some("sha256:raw"));
    assert_eq!(details.resolved_destination_hash.as_deref(), Some("sha256:resolved"));
    assert_eq!(details.source_hash.as_deref(), Some("sha256:source"));
    assert_eq!(details.destination_hash.as_deref(), Some("sha256:destination"));
    assert_eq!(details.dropped_message_id.as_deref(), Some("msg-drop-1"));
    assert_eq!(details.payload_mode.as_deref(), Some("full_wire"));
    assert_eq!(details.bytes_len, Some(24));
    assert_eq!(details.detail.as_deref(), Some("propagated LXMF payload too short"));
}

#[test]
fn maps_delivery_lifecycle_to_typed_details() {
    let mapped = map_sdk_event(
        base_event(
            "DeliveryStateTransition",
            json!({
                "from": "queued",
                "to": "sent",
                "delivery_kind": "direct",
                "message": { "receipt_status": "sent:packet" }
            }),
        ),
        "desktop_default",
    );

    assert!(matches!(mapped.kind, EventKind::MessageSent));
    let details = mapped.delivery_lifecycle_details().expect("lifecycle details");
    assert_eq!(details.from.as_deref(), Some("queued"));
    assert_eq!(details.to.as_deref(), Some("sent"));
    assert_eq!(details.delivery_kind.as_deref(), Some("direct"));
    assert_eq!(details.receipt_status.as_deref(), Some("sent:packet"));
}

#[test]
fn maps_receipt_event_to_lifecycle_details() {
    let mapped = map_sdk_event(
        base_event(
            "receipt",
            json!({
                "status": "delivered:receipt",
                "message_id": "msg-receipt-1",
                "peer": "peer-dst",
                "delivery_kind": "link-packet",
                "method": "direct",
                "packet_hash": "packet-1",
                "resource_hash": "resource-1",
                "bytes": 128,
                "link_id": "link-1"
            }),
        ),
        "desktop_default",
    );

    assert!(matches!(mapped.kind, EventKind::MessageDelivered));
    assert_eq!(mapped.metadata.message_id.as_deref(), Some("msg-receipt-1"));
    assert_eq!(mapped.metadata.peer_id.as_deref(), Some("peer-dst"));
    let details = mapped.delivery_lifecycle_details().expect("receipt lifecycle");
    assert_eq!(details.state.as_deref(), Some("delivered"));
    assert_eq!(details.receipt_status.as_deref(), Some("delivered:receipt"));
    assert_eq!(details.delivery_kind.as_deref(), Some("link-packet"));
    assert_eq!(details.peer.as_deref(), Some("peer-dst"));
    assert_eq!(details.method.as_deref(), Some("direct"));
    assert_eq!(details.packet_hash.as_deref(), Some("packet-1"));
    assert_eq!(details.resource_hash.as_deref(), Some("resource-1"));
    assert_eq!(details.bytes, Some(128));
    assert_eq!(details.link_id.as_deref(), Some("link-1"));
}

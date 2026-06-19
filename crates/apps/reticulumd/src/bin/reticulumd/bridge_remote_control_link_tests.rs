use super::*;
use rns_transport::hash::Hash;
use rns_transport::packet::PacketDataBuffer;
use rns_transport::resource::{ResourceEvent, ResourceEventKind};
use rns_transport::transport::{ReceivedData, ReceivedPayloadMode};

#[test]
fn build_link_request_payload_encodes_peer_sync_data_as_msgpack_binary() {
    let peer_hash = vec![0xab; 16];
    let payload =
        build_link_request_payload("/pn/peer/sync", rmpv::Value::Binary(peer_hash.clone()))
            .expect("encode peer sync request");

    assert_eq!(payload.first(), Some(&0x93), "request frame must be a 3-item array");
    assert_eq!(
        payload.get(10..12),
        Some(&[0xc4, 0x10][..]),
        "path hash must be MessagePack bin8 bytes"
    );
    assert_eq!(
        payload.get(28..30),
        Some(&[0xc4, 0x10][..]),
        "peer sync data must be MessagePack bin8 bytes for Python LXMF"
    );
    assert_eq!(payload.get(30..46), Some(peer_hash.as_slice()));
}

async fn resource_terminal_error(kind: ResourceEventKind) -> std::io::Error {
    let (_data_tx, mut data_rx) = tokio::sync::broadcast::channel(4);
    let (resource_tx, mut resource_rx) = tokio::sync::broadcast::channel(4);
    let destination = AddressHash::new([0x11; 16]);
    let link_id = AddressHash::new([0x22; 16]);
    let request_id = [0x33; 16];

    resource_tx
        .send(ResourceEvent {
            hash: Hash::new_from_slice(b"terminal propagation control resource"),
            link_id,
            kind,
        })
        .expect("send terminal resource event");

    wait_for_link_request_response_with_terminal_policy(
        &mut data_rx,
        &mut resource_rx,
        destination,
        link_id,
        request_id,
        true,
        Duration::from_millis(50),
    )
    .await
    .expect_err("terminal resource event should fail immediately")
}

#[tokio::test]
async fn wait_for_link_request_response_fails_on_resource_failure() {
    let err = resource_terminal_error(ResourceEventKind::OutboundFailed).await;

    assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    assert_eq!(err.to_string(), "propagation control resource transfer failed");
}

#[tokio::test]
async fn wait_for_link_request_response_fails_on_resource_cancel() {
    let err = resource_terminal_error(ResourceEventKind::OutboundCancelled).await;

    assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    assert_eq!(err.to_string(), "propagation control resource transfer cancelled");
}

#[tokio::test]
async fn wait_for_link_request_response_ignores_terminal_resource_without_policy() {
    let (data_tx, mut data_rx) = tokio::sync::broadcast::channel(4);
    let (resource_tx, mut resource_rx) = tokio::sync::broadcast::channel(4);
    let destination = AddressHash::new([0x11; 16]);
    let link_id = AddressHash::new([0x22; 16]);
    let stale_request_id = [0x33; 16];
    let request_id = [0x44; 16];
    let response_payload = rmpv::Value::Array(vec![
        rmpv::Value::Binary(request_id.to_vec()),
        rmpv::Value::String("ok".into()),
    ]);
    let response_frame = rmp_serde::to_vec(&response_payload).expect("encode response frame");

    resource_tx
        .send(ResourceEvent {
            hash: Hash::new_from_slice(b"stale propagation control resource"),
            link_id,
            kind: ResourceEventKind::OutboundFailed,
        })
        .expect("send stale terminal resource event");
    assert!(data_tx
        .send(ReceivedData {
            destination: link_id,
            data: PacketDataBuffer::new_from_slice(&response_frame),
            payload_mode: ReceivedPayloadMode::FullWire,
            ratchet_used: false,
            context: Some(PacketContext::None),
            request_id: None,
            hops: None,
            interface: None,
        })
        .is_ok());

    let response = wait_for_link_request_response_with_terminal_policy(
        &mut data_rx,
        &mut resource_rx,
        destination,
        link_id,
        request_id,
        false,
        Duration::from_millis(50),
    )
    .await
    .expect("stale terminal event should not fail the current request");

    assert_eq!(response.as_str(), Some("ok"));
    assert_ne!(stale_request_id, request_id);
}

#[tokio::test]
async fn wait_for_link_request_response_fails_on_link_close_signal() {
    let (data_tx, mut data_rx) = tokio::sync::broadcast::channel(4);
    let (_resource_tx, mut resource_rx) = tokio::sync::broadcast::channel::<ResourceEvent>(4);
    let expected_destination = AddressHash::new_from_slice(&[0x11; 16]);
    let expected_link_id = AddressHash::new_from_slice(&[0x22; 16]);
    let request_id = [0x33; 16];
    let signal_payload = rmp_serde::to_vec(&vec![0xf1u8]).expect("signal msgpack");

    assert!(
        data_tx
            .send(ReceivedData {
                destination: expected_link_id,
                data: PacketDataBuffer::new_from_slice(&signal_payload),
                payload_mode: ReceivedPayloadMode::FullWire,
                ratchet_used: false,
                context: Some(PacketContext::LinkClose),
                request_id: None,
                hops: None,
                interface: None,
            })
            .is_ok(),
        "send link-close signal"
    );

    let err = wait_for_link_request_response(
        &mut data_rx,
        &mut resource_rx,
        expected_destination,
        expected_link_id,
        request_id,
        Duration::from_secs(10),
    )
    .await
    .expect_err("link-close signal should fail the active request");

    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(err.to_string().contains("propagation node denied access"));
}

use std::time::Duration;

use rns_transport::iface::meshtastic::{
    calc_meshtastic_index, MeshtasticDestination, MeshtasticInterfaceConfig,
    MeshtasticPacketHandler, MeshtasticReceivedFrame, MeshtasticTunnel,
};

#[test]
fn packet_handler_splits_and_reassembles_python_metadata_format() {
    let data = b"hello_world".repeat(20);
    let handler =
        MeshtasticPacketHandler::new_outgoing(&data, 7, 80).expect("split outgoing payload");

    assert_eq!(handler.positions(), vec![1, 2, -3]);
    assert_eq!(MeshtasticPacketHandler::metadata(handler.payload_at(1).unwrap()), Ok((7, 1)));
    assert_eq!(MeshtasticPacketHandler::metadata(handler.payload_at(3).unwrap()), Ok((7, -3)));

    let mut inbound = MeshtasticPacketHandler::new_inbound();
    assert_eq!(inbound.process_payload(handler.payload_at(1).unwrap()).unwrap(), None);
    assert_eq!(inbound.process_payload(handler.payload_at(2).unwrap()).unwrap(), None);
    assert_eq!(inbound.process_payload(handler.payload_at(3).unwrap()).unwrap(), Some(data));
}

#[test]
fn tunnel_requests_missing_chunks_and_completes_after_repair() {
    let mut tunnel = MeshtasticTunnel::new(MeshtasticInterfaceConfig {
        max_payload_bytes: 80,
        ..MeshtasticInterfaceConfig::default()
    });
    let data = b"reticulum-over-meshtastic".repeat(8);
    let handler =
        MeshtasticPacketHandler::new_outgoing(&data, 4, 80).expect("split outgoing payload");

    assert_eq!(
        tunnel
            .process_received(MeshtasticReceivedFrame::new(1001, handler.payload_at(1).unwrap()))
            .unwrap(),
        None
    );
    assert_eq!(
        tunnel
            .process_received(MeshtasticReceivedFrame::new(1001, handler.payload_at(3).unwrap()))
            .unwrap(),
        None
    );

    let request = tunnel.next_transmit().expect("missing chunk request");
    assert_eq!(request.destination, MeshtasticDestination::Broadcast);
    assert!(request.payload.starts_with(b"REQ"));
    assert_eq!(MeshtasticPacketHandler::metadata(&request.payload[3..]), Ok((4, 2)));

    assert_eq!(
        tunnel
            .process_received(MeshtasticReceivedFrame::new(1001, handler.payload_at(2).unwrap()))
            .unwrap(),
        Some(data)
    );
}

#[test]
fn tunnel_requests_next_gap_after_repaired_non_final_chunk() {
    let mut tunnel = MeshtasticTunnel::new(MeshtasticInterfaceConfig {
        max_payload_bytes: 80,
        ..MeshtasticInterfaceConfig::default()
    });
    let data = b"reticulum-over-meshtastic".repeat(12);
    let handler =
        MeshtasticPacketHandler::new_outgoing(&data, 9, 80).expect("split outgoing payload");

    assert_eq!(handler.positions(), vec![1, 2, 3, -4]);
    assert_eq!(
        tunnel
            .process_received(MeshtasticReceivedFrame::new(1001, handler.payload_at(1).unwrap()))
            .unwrap(),
        None
    );
    assert_eq!(
        tunnel
            .process_received(MeshtasticReceivedFrame::new(1001, handler.payload_at(4).unwrap()))
            .unwrap(),
        None
    );

    let first_request = tunnel.next_transmit().expect("first missing chunk request");
    assert_eq!(MeshtasticPacketHandler::metadata(&first_request.payload[3..]), Ok((9, 2)));
    assert_eq!(
        tunnel
            .process_received(MeshtasticReceivedFrame::new(1001, handler.payload_at(2).unwrap()))
            .unwrap(),
        None
    );

    let second_request = tunnel.next_transmit().expect("second missing chunk request");
    assert_eq!(MeshtasticPacketHandler::metadata(&second_request.payload[3..]), Ok((9, 3)));
    assert_eq!(
        tunnel
            .process_received(MeshtasticReceivedFrame::new(1001, handler.payload_at(3).unwrap()))
            .unwrap(),
        Some(data)
    );
}

#[test]
fn tunnel_rejects_packet_index_wrap_before_overwriting_queued_payloads() {
    let mut tunnel = MeshtasticTunnel::new(MeshtasticInterfaceConfig::default());

    for index in 0..=u8::MAX {
        let payload = format!("packet-{index:03}");
        tunnel.queue_outgoing_packet(payload.as_bytes()).expect("queue packet");
    }
    let err = tunnel.queue_outgoing_packet(b"overflow").expect_err("index space should be full");
    assert_eq!(err, "meshtastic outgoing packet index space is full");

    let first = tunnel.next_transmit().expect("first queued packet");
    assert_eq!(first.payload[0], 0);
    assert_eq!(first.payload[1] as i8, -1);
    assert_eq!(&first.payload[2..], b"packet-000");
    tunnel.queue_outgoing_packet(b"after-drain").expect("drained index can be reused");
}

#[test]
fn tunnel_handles_empty_reassembled_payload_without_destination_panic() {
    let mut tunnel = MeshtasticTunnel::new(MeshtasticInterfaceConfig::default());

    let received = tunnel
        .process_received(MeshtasticReceivedFrame::new(1001, &[7, 0xff]))
        .expect("empty final payload should not panic");

    assert_eq!(received, Some(Vec::new()));
    assert_eq!(tunnel.status().destination_routes, 0);
}

#[test]
fn tunnel_learns_link_destinations_for_direct_meshtastic_replies() {
    let learned_destination = [0x42; 16];
    let mut inbound = Vec::new();
    inbound.push(0b0000_1100);
    inbound.push(0);
    inbound.extend_from_slice(&learned_destination);
    inbound.push(0);
    inbound.extend_from_slice(b"body");

    let handler =
        MeshtasticPacketHandler::new_outgoing(&inbound, 1, 200).expect("split inbound payload");
    let mut tunnel = MeshtasticTunnel::new(MeshtasticInterfaceConfig::default());
    assert_eq!(
        tunnel
            .process_received(MeshtasticReceivedFrame::new(77, handler.payload_at(1).unwrap()))
            .unwrap(),
        Some(inbound)
    );

    let mut outbound = Vec::new();
    outbound.push(0);
    outbound.push(0);
    outbound.extend_from_slice(&learned_destination);
    outbound.push(0);
    outbound.extend_from_slice(b"reply");
    tunnel.queue_outgoing_packet(&outbound).expect("queue outbound");

    let transmit = tunnel.next_transmit().expect("transmit frame");
    assert_eq!(transmit.destination, MeshtasticDestination::Node(77));
}

#[test]
fn config_preserves_reference_preset_delays_and_index_wrap() {
    assert_eq!(calc_meshtastic_index(255), 0);
    assert_eq!(
        MeshtasticInterfaceConfig::from_modem_preset(8).send_delay,
        Duration::from_millis(400)
    );
    assert_eq!(MeshtasticInterfaceConfig::from_modem_preset(7).send_delay, Duration::from_secs(12));
    assert_eq!(MeshtasticInterfaceConfig::from_modem_preset(99).send_delay, Duration::from_secs(7));
}

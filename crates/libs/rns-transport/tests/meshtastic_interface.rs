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

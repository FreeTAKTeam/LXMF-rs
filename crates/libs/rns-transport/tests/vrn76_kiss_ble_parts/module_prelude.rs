use std::collections::VecDeque;

use std::time::Duration;

use rns_transport::iface::kiss::{KissConfig, KissIdBeaconConfig};

use rns_transport::iface::vrn76_kiss_ble::{
    encode_benshi_data_rxd_event, encode_benshi_ht_send_data, BleWrite, Vrn76FrameMode,
    Vrn76KissBleBackend, Vrn76KissBleConfig, Vrn76KissBleError, Vrn76KissBleRuntime,
    Vrn76KissBleSession, VRN76_INDICATE_CHARACTERISTIC_UUID, VRN76_SERVICE_UUID,
    VRN76_WRITE_CHARACTERISTIC_UUID,
};

use rns_transport::kiss::{
    encode_command_frame, encode_data_frame, CMD_FULLDUPLEX, CMD_P, CMD_READY, CMD_SLOTTIME,
    CMD_TXDELAY, CMD_TXTAIL,
};

#[test]
fn vrn76_defaults_match_benshi_kiss_ble_profile() {
    let config = Vrn76KissBleConfig::default();

    assert_eq!(VRN76_SERVICE_UUID, "00001100-d102-11e1-9b23-00025b00a5a5");
    assert_eq!(VRN76_WRITE_CHARACTERISTIC_UUID, "00001101-d102-11e1-9b23-00025b00a5a5");
    assert_eq!(VRN76_INDICATE_CHARACTERISTIC_UUID, "00001102-d102-11e1-9b23-00025b00a5a5");
    assert_eq!(config.mtu, 564);
    assert_eq!(config.max_write_len, 512);
    assert_eq!(config.scan_timeout, Duration::from_millis(10_000));
    assert_eq!(config.command_timeout, Duration::from_millis(3_000));
    assert_eq!(config.read_frame_timeout, Duration::from_millis(1_250));
    assert_eq!(config.frame_mode, Vrn76FrameMode::BenshiTncData);
    assert_eq!(config.kiss.preamble_ms, 350);
    assert_eq!(config.kiss.tx_tail_ms, 20);
    assert_eq!(config.kiss.persistence, 64);
    assert_eq!(config.kiss.slot_time_ms, 20);
    assert!(!config.kiss.flow_control);
}

#[test]
fn vrn76_startup_subscribes_before_configuring_kiss_modem() {
    let mut session = Vrn76KissBleSession::new(Vrn76KissBleConfig::default());

    let writes = session.startup_frames();

    assert!(session.is_subscribed());
    assert!(writes
        .iter()
        .all(|write| write.characteristic_uuid == VRN76_WRITE_CHARACTERISTIC_UUID));
    assert!(writes.iter().all(|write| write.with_response));
    assert!(writes
        .iter()
        .any(|write| write.payload
            == encode_benshi_ht_send_data(&encode_command_frame(CMD_P, &[64]))));
}

#[test]
fn vrn76_session_status_reports_subscription_flow_control_and_queues() {
    let config = Vrn76KissBleConfig {
        kiss: rns_transport::iface::kiss::KissConfig { flow_control: true, ..Default::default() },
        ..Default::default()
    };
    let mut session = Vrn76KissBleSession::new(config);

    let initial = session.status();
    assert!(!initial.connected);
    assert!(!initial.subscribed);
    assert!(!initial.interface_ready);
    assert_eq!(initial.startup_write_failures, 0);
    assert_eq!(initial.pending_payloads, 0);
    assert_eq!(initial.pending_writes, 0);

    let _ = session.startup_frames();
    let _ = session.enqueue_packet(&[0x10, 0x20]);
    let queued = session.status();
    assert!(queued.subscribed);
    assert!(!queued.interface_ready);
    assert_eq!(queued.pending_payloads, 1);
    assert_eq!(queued.pending_writes, 0);

    session
        .accept_indication(&encode_benshi_data_rxd_event(&encode_command_frame(CMD_READY, &[1])))
        .expect("ready frame");
    let ready = session.status();
    assert!(!ready.interface_ready);
    assert_eq!(ready.pending_payloads, 0);
    assert_eq!(ready.pending_writes, 1);
}

#[test]
fn vrn76_status_handle_exports_json() {
    let handle = rns_transport::iface::vrn76_kiss_ble::Vrn76KissBleStatusHandle::new();
    handle.update(rns_transport::iface::vrn76_kiss_ble::Vrn76KissBleStatus {
        connected: true,
        subscribed: true,
        interface_ready: true,
        startup_write_failures: 1,
        pending_payloads: 2,
        pending_writes: 3,
        pending_packets: 4,
    });

    let status = handle.to_json();

    assert_eq!(status["connected"].as_bool(), Some(true));
    assert_eq!(status["subscribed"].as_bool(), Some(true));
    assert_eq!(status["interface_ready"].as_bool(), Some(true));
    assert_eq!(status["startup_write_failures"].as_u64(), Some(1));
    assert_eq!(status["pending_payloads"].as_u64(), Some(2));
    assert_eq!(status["pending_writes"].as_u64(), Some(3));
    assert_eq!(status["pending_packets"].as_u64(), Some(4));
}

#[test]
fn vrn76_benshi_mode_wraps_outgoing_kiss_frames_as_ht_send_data() {
    let mut session = Vrn76KissBleSession::new(Vrn76KissBleConfig::default());
    let payload = [0x01, 0xC0, 0xDB, 0x02];

    let writes = session.enqueue_packet(&payload);

    assert_eq!(
        writes,
        vec![BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_benshi_ht_send_data(&encode_data_frame(&payload)),
        }]
    );
}

#[test]
fn vrn76_benshi_mode_splits_outbound_kiss_frames_by_ble_write_limit() {
    let config = Vrn76KissBleConfig { max_write_len: 8, ..Default::default() };
    let mut session = Vrn76KissBleSession::new(config);

    let writes = session.enqueue_packet(&[1, 2, 3, 4, 5]);

    assert_eq!(
        writes,
        vec![
            BleWrite {
                characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
                with_response: true,
                payload: vec![0x00, 0x02, 0x00, 0x1F, 0x00, 0xC0, 0x00, 1],
            },
            BleWrite {
                characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
                with_response: true,
                payload: vec![0x00, 0x02, 0x00, 0x1F, 0x01, 2, 3, 4],
            },
            BleWrite {
                characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
                with_response: true,
                payload: vec![0x00, 0x02, 0x00, 0x1F, 0x82, 5, 0xC0],
            },
        ]
    );
    assert!(writes.iter().all(|write| write.payload.len() <= 8));
}

#[test]
fn vrn76_raw_mode_writes_kiss_frames_directly() {
    let config = Vrn76KissBleConfig { frame_mode: Vrn76FrameMode::RawKiss, ..Default::default() };
    let mut session = Vrn76KissBleSession::new(config);
    let payload = [0x01, 0xC0, 0xDB, 0x02];

    let writes = session.enqueue_packet(&payload);

    assert_eq!(
        writes,
        vec![BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_data_frame(&payload),
        }]
    );
}

#[test]
fn vrn76_session_writes_python_kiss_id_beacon_payload() {
    let config = Vrn76KissBleConfig {
        kiss: KissConfig {
            id_beacon: Some(KissIdBeaconConfig {
                callsign: b"MYCALL-0".to_vec(),
                interval: Duration::from_secs(600),
                min_payload_len: 15,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let session = Vrn76KissBleSession::new(config);
    let mut expected_payload = b"MYCALL-0".to_vec();
    expected_payload.resize(15, 0);

    assert_eq!(
        session.id_beacon_write(),
        Some(BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_benshi_ht_send_data(&encode_data_frame(&expected_payload)),
        })
    );
}

#[test]
fn vrn76_session_suppresses_own_kiss_id_beacon_indication() {
    let config = Vrn76KissBleConfig {
        kiss: KissConfig {
            id_beacon: Some(KissIdBeaconConfig {
                callsign: b"MYCALL-0".to_vec(),
                interval: Duration::from_secs(600),
                min_payload_len: 15,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut session = Vrn76KissBleSession::new(config);
    let mut beacon = b"MYCALL-0".to_vec();
    beacon.resize(15, 0);

    assert!(session
        .accept_indication(&encode_benshi_data_rxd_event(&encode_data_frame(&beacon)))
        .expect("beacon indication")
        .is_empty());
}

#[test]
fn vrn76_benshi_data_rxd_events_decode_kiss_payloads() {
    let mut session = Vrn76KissBleSession::new(Vrn76KissBleConfig::default());
    let frame = encode_data_frame(&[0xAA, 0xC0, 0xDB, 0xBB]);
    let split = frame.len() / 2;

    assert!(session
        .accept_indication(&encode_benshi_data_rxd_event(&frame[..split]))
        .expect("first fragment")
        .is_empty());
    let packets = session
        .accept_indication(&encode_benshi_data_rxd_event(&frame[split..]))
        .expect("second fragment");

    assert_eq!(packets, vec![vec![0xAA, 0xC0, 0xDB, 0xBB]]);
}

#[test]
fn vrn76_benshi_data_rxd_events_decode_ordered_tnc_fragments() {
    let mut session = Vrn76KissBleSession::new(Vrn76KissBleConfig::default());
    let frame = encode_data_frame(&[0xA1, 0xA2, 0xA3, 0xA4]);

    assert!(session
        .accept_indication(&encode_benshi_data_rxd_fragment(0, false, &frame[..2]))
        .expect("first tnc fragment")
        .is_empty());
    let packets = session
        .accept_indication(&encode_benshi_data_rxd_fragment(1, true, &frame[2..]))
        .expect("final tnc fragment");

    assert_eq!(packets, vec![vec![0xA1, 0xA2, 0xA3, 0xA4]]);
}

#[test]
fn vrn76_benshi_data_rxd_events_decode_channel_id_tnc_fragments() {
    let mut session = Vrn76KissBleSession::new(Vrn76KissBleConfig::default());
    let frame = encode_data_frame(&[0xC1, 0xC2]);

    let packets = session
        .accept_indication(&encode_benshi_data_rxd_channel_fragment(0, true, 7, &frame))
        .expect("channel-id tnc fragment");

    assert_eq!(packets, vec![vec![0xC1, 0xC2]]);
}

#[test]
fn vrn76_benshi_data_rxd_events_decode_ordered_channel_id_tnc_fragments() {
    let mut session = Vrn76KissBleSession::new(Vrn76KissBleConfig::default());
    let frame = encode_data_frame(&[0xB1, 0xB2, 0xB3, 0xB4]);

    assert!(session
        .accept_indication(&encode_benshi_data_rxd_channel_fragment(0, false, 4, &frame[..2]))
        .expect("first channel-id tnc fragment")
        .is_empty());
    let packets = session
        .accept_indication(&encode_benshi_data_rxd_channel_fragment(1, true, 4, &frame[2..]))
        .expect("final channel-id tnc fragment");

    assert_eq!(packets, vec![vec![0xB1, 0xB2, 0xB3, 0xB4]]);
}

#[test]
fn vrn76_benshi_data_rxd_rejects_channel_id_mismatch() {
    let mut session = Vrn76KissBleSession::new(Vrn76KissBleConfig::default());
    let frame = encode_data_frame(&[0xD1, 0xD2, 0xD3, 0xD4]);

    assert!(session
        .accept_indication(&encode_benshi_data_rxd_channel_fragment(0, false, 4, &frame[..2]))
        .expect("first channel-id tnc fragment")
        .is_empty());
    let err = session
        .accept_indication(&encode_benshi_data_rxd_channel_fragment(1, true, 5, &frame[2..]))
        .expect_err("channel id must remain stable across fragments");

    assert_eq!(
        err,
        rns_transport::iface::vrn76_kiss_ble::Vrn76KissBleError::UnexpectedTncChannel {
            expected_channel_id: Some(4),
            actual_channel_id: Some(5),
        }
    );
}

#[test]
fn vrn76_benshi_data_rxd_rejects_out_of_order_tnc_fragments() {
    let mut session = Vrn76KissBleSession::new(Vrn76KissBleConfig::default());

    let err = session
        .accept_indication(&encode_benshi_data_rxd_fragment(1, false, &[0xC0]))
        .expect_err("first fragment id must be zero");

    assert_eq!(
        err,
        rns_transport::iface::vrn76_kiss_ble::Vrn76KissBleError::UnexpectedTncFragment {
            expected_fragment_id: 0,
            actual_fragment_id: 1,
        }
    );
}

#[test]
fn vrn76_benshi_malformed_indication_resets_partial_tnc_fragment() {
    let mut session = Vrn76KissBleSession::new(Vrn76KissBleConfig::default());
    let first_frame = encode_data_frame(&[0xA1, 0xA2]);

    assert!(session
        .accept_indication(&encode_benshi_data_rxd_fragment(0, false, &first_frame[..2]))
        .expect("first fragment")
        .is_empty());

    assert_eq!(
        session.accept_indication(&[0x00]).expect_err("malformed indication"),
        rns_transport::iface::vrn76_kiss_ble::Vrn76KissBleError::BenshiFrameTooShort { actual: 1 }
    );

    let recovered_frame = encode_data_frame(&[0xB1, 0xB2]);
    let packets = session
        .accept_indication(&encode_benshi_data_rxd_fragment(0, true, &recovered_frame))
        .expect("fresh first fragment should be accepted after malformed input");

    assert_eq!(packets, vec![vec![0xB1, 0xB2]]);
}

#[test]
fn vrn76_raw_mode_indications_decode_split_kiss_frames_into_packet_payloads() {
    let config = Vrn76KissBleConfig { frame_mode: Vrn76FrameMode::RawKiss, ..Default::default() };
    let mut session = Vrn76KissBleSession::new(config);
    let frame = encode_data_frame(&[0xAA, 0xC0, 0xDB, 0xBB]);
    let split = frame.len() / 2;

    assert!(session.accept_indication(&frame[..split]).expect("first chunk").is_empty());
    let packets = session.accept_indication(&frame[split..]).expect("second chunk");

    assert_eq!(packets, vec![vec![0xAA, 0xC0, 0xDB, 0xBB]]);
}

#[test]
fn vrn76_raw_mode_drops_stale_partial_kiss_frame_before_ready() {
    let config = Vrn76KissBleConfig {
        frame_mode: Vrn76FrameMode::RawKiss,
        read_frame_timeout: Duration::from_millis(10),
        kiss: KissConfig { flow_control: true, ..Default::default() },
        ..Default::default()
    };
    let mut session = Vrn76KissBleSession::new(config);

    assert!(session.enqueue_packet(&[0x10, 0x20]).is_empty());
    assert!(session.accept_indication(&[0xC0, 0x00, b'x']).expect("partial frame").is_empty());
    std::thread::sleep(Duration::from_millis(30));

    let packets = session
        .accept_indication(&encode_command_frame(CMD_READY, &[0x01]))
        .expect("ready after stale partial frame");

    assert!(packets.is_empty(), "stale partial data frame should be dropped");
    assert_eq!(
        session.take_pending_writes(),
        vec![BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_data_frame(&[0x10, 0x20]),
        }]
    );
}

#[test]
fn vrn76_benshi_mode_drops_stale_partial_kiss_frame_before_ready() {
    let config = Vrn76KissBleConfig {
        read_frame_timeout: Duration::from_millis(10),
        kiss: KissConfig { flow_control: true, ..Default::default() },
        ..Default::default()
    };
    let mut session = Vrn76KissBleSession::new(config);

    assert!(session.enqueue_packet(&[0x10, 0x20]).is_empty());
    assert!(session
        .accept_indication(&encode_benshi_data_rxd_event(&[0xC0, 0x00, b'x']))
        .expect("partial frame")
        .is_empty());
    std::thread::sleep(Duration::from_millis(30));

    let packets = session
        .accept_indication(&encode_benshi_data_rxd_event(&encode_command_frame(CMD_READY, &[0x01])))
        .expect("ready after stale partial frame");

    assert!(packets.is_empty(), "stale partial data frame should be dropped");
    assert_eq!(
        session.take_pending_writes(),
        vec![BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_benshi_ht_send_data(&encode_data_frame(&[0x10, 0x20])),
        }]
    );
}

fn encode_benshi_data_rxd_fragment(fragment_id: u8, is_final: bool, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x00, 0x02, 0x00, 0x09, 0x02];
    frame.push((u8::from(is_final) << 7) | (fragment_id & 0x3f));
    frame.extend_from_slice(payload);
    frame
}

fn encode_benshi_data_rxd_channel_fragment(
    fragment_id: u8,
    is_final: bool,
    channel_id: u8,
    payload: &[u8],
) -> Vec<u8> {
    let mut frame = vec![0x00, 0x02, 0x00, 0x09, 0x02];
    frame.push(0x40 | (u8::from(is_final) << 7) | (fragment_id & 0x3f));
    frame.extend_from_slice(payload);
    frame.push(channel_id);
    frame
}

#[test]
fn vrn76_flow_control_queues_packets_until_ready_indication() {
    let config = Vrn76KissBleConfig {
        kiss: rns_transport::iface::kiss::KissConfig { flow_control: true, ..Default::default() },
        ..Default::default()
    };
    let mut session = Vrn76KissBleSession::new(config);

    assert!(session.enqueue_packet(&[0x10, 0x20]).is_empty());

    let packets = session
        .accept_indication(&encode_benshi_data_rxd_event(&encode_command_frame(CMD_READY, &[0x01])))
        .expect("ready frame");
    assert!(packets.is_empty());

    assert_eq!(
        session.take_pending_writes(),
        vec![BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_benshi_ht_send_data(&encode_data_frame(&[0x10, 0x20])),
        }]
    );
}

#[test]
fn vrn76_benshi_wrappers_match_reference_wire_shape() {
    assert_eq!(
        encode_benshi_ht_send_data(&[0xC0, 0x00, 0xC0]),
        vec![0x00, 0x02, 0x00, 0x1F, 0x80, 0xC0, 0x00, 0xC0]
    );
    assert_eq!(
        encode_benshi_data_rxd_event(&[0xC0, 0x00, 0xC0]),
        vec![0x00, 0x02, 0x00, 0x09, 0x02, 0x80, 0xC0, 0x00, 0xC0]
    );
}

use super::*;

#[test]
fn notification_pairs_signal_stats_with_the_packet_they_precede() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::from_secs(60));
    let mut session = RnodeBleKissSession::new(RnodeBleKissConfig::default());
    let mut bytes = encode_command_frame(CMD_SF, &[9]);
    bytes.extend(encode_command_frame(crate::iface::lora::CMD_STAT_RSSI, &[97]));
    bytes.extend(encode_command_frame(crate::iface::lora::CMD_STAT_SNR, &[0xF8]));
    bytes.extend(encode_data_frame(b"first"));
    bytes.extend(encode_data_frame(b"second"));
    bytes.extend(encode_command_frame(crate::iface::lora::CMD_STAT_RSSI, &[107]));
    let notification = session.accept_notification_events(&bytes).expect("decode");
    assert_eq!(notification.packets.len(), 2);
    assert_eq!(notification.order.len(), 6);

    let signals = monitor.accept_notification(&notification).expect("accept");

    assert_eq!(
        signals,
        vec![Some(PacketSignal { rssi: Some(-60.0), snr: Some(-2.0), q: Some(57.9) }), None],
        "the stats pair with the packet they precede; a packet with none ahead carries none"
    );

    let trailing =
        session.accept_notification_events(&encode_data_frame(b"third")).expect("decode");
    let signals = monitor.accept_notification(&trailing).expect("accept");
    assert_eq!(
        signals,
        vec![Some(PacketSignal { rssi: Some(-50.0), snr: None, q: Some(57.9) })],
        "stats at the end of one notification wait for the next frame; quality is the last known"
    );
}

#[test]
fn a_hand_built_notification_reads_as_commands_then_packets() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::from_secs(60));
    let notification = RnodeBleNotification {
        packets: vec![b"data".to_vec()],
        commands: vec![(CMD_SF, vec![9]), (crate::iface::lora::CMD_STAT_RSSI, vec![97])],
        order: Vec::new(),
    };

    let signals = monitor.accept_notification(&notification).expect("accept");

    assert_eq!(signals, vec![Some(PacketSignal { rssi: Some(-60.0), snr: None, q: None })]);
}

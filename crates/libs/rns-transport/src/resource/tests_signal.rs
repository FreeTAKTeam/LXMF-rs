#[test]
fn a_completed_inbound_resource_carries_the_links_last_signal() {
    let (mut sender_link, mut receiver_link) = resource_link_pair();
    let mut sender = ResourceManager::new_with_config(Duration::from_secs(30), 8);
    let mut receiver = ResourceManager::new_with_config(Duration::from_secs(30), 8);
    let payload = vec![0x5a; 700];
    let (original_hash, advertisement) =
        sender.start_send(&sender_link, payload.clone(), None).expect("start resource");
    sender.confirm_outbound_dispatch(original_hash, true);
    let heard = crate::transport::PacketSignal { rssi: Some(-101.0), snr: Some(-3.5), q: Some(28.9) };
    receiver_link.note_signal(Some(heard));

    let mut to_receiver = vec![advertisement];
    let mut delivered = None;
    for _ in 0..1_000 {
        if to_receiver.is_empty() {
            break;
        }
        let mut to_sender = Vec::new();
        for packet in std::mem::take(&mut to_receiver) {
            let plain = decrypt_link_packet(&receiver_link, &packet);
            to_sender.extend(receiver.handle_packet(&plain, &mut receiver_link));
        }
        for event in receiver.drain_events() {
            if let ResourceEventKind::Complete(complete) = event.kind {
                delivered = Some(complete);
            }
        }
        for packet in to_sender {
            let plain = decrypt_link_packet(&sender_link, &packet);
            to_receiver.extend(sender.handle_packet(&plain, &mut sender_link));
        }
        sender.drain_events();
    }

    let complete = delivered.expect("the transfer completes");
    assert_eq!(complete.data, payload);
    assert_eq!(complete.signal, Some(heard), "the link's last signal rides with the completed transfer");
}

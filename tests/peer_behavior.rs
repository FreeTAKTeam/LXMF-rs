use lxmf::constants::TICKET_LENGTH;
use lxmf::peer::Peer;

#[test]
fn peer_serializes_and_restores_state() {
    let mut peer = Peer::new([0x44; 16]);
    peer.mark_seen(1_700_000_000.0);
    peer.set_name("node-a");
    peer.queue_unhandled_message(b"unhandled");
    peer.queue_handled_message(b"handled");
    peer.process_queues();

    let packed = peer.to_bytes().unwrap();
    let unpacked = Peer::from_bytes(&packed).unwrap();

    assert_eq!(unpacked.dest(), [0x44; 16]);
    assert_eq!(unpacked.last_seen(), Some(1_700_000_000.0));
    assert_eq!(unpacked.name(), Some("node-a"));
    assert_eq!(unpacked.handled_message_count(), 1);
    assert_eq!(unpacked.unhandled_message_count(), 1);
}

#[test]
fn peer_acceptance_rate_tracks_handled_vs_unhandled() {
    let mut peer = Peer::new([0x55; 16]);
    peer.add_unhandled_message(b"one");
    peer.add_unhandled_message(b"two");
    assert_eq!(peer.acceptance_rate(), 0.0);

    peer.add_handled_message(b"one");
    assert_eq!(peer.handled_message_count(), 1);
    assert_eq!(peer.unhandled_message_count(), 1);
    assert_eq!(peer.acceptance_rate(), 0.5);
}

#[test]
fn peer_can_generate_peering_key() {
    let mut peer = Peer::new([0x66; 16]);
    assert!(!peer.peering_key_ready());

    peer.generate_peering_key();
    assert!(peer.peering_key_ready());
    let key = peer.peering_key_value().expect("peering key");
    assert_eq!(key.len(), TICKET_LENGTH);
}

use lxmf::peer::Peer;

#[test]
fn peer_tracks_last_seen() {
    let mut peer = Peer::new([1u8; 16]);
    peer.mark_seen(123.0);
    assert_eq!(peer.last_seen(), Some(123.0));
}

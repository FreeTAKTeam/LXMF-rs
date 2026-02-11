use lxmf::peer::Peer;

#[test]
fn peer_exposes_destination() {
    let dest = [2u8; 16];
    let peer = Peer::new(dest);
    assert_eq!(peer.dest(), dest);
}

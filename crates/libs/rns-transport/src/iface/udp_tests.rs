#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_detects_multicast_bind_v4() {
        let iface =
            UdpInterface::new("224.0.0.224:4242".to_string(), Some("224.0.0.224:4242".to_string()));
        assert!(iface.is_multicast());
    }

    #[test]
    fn new_detects_multicast_forward_even_if_bind_is_unicast() {
        let iface =
            UdpInterface::new("0.0.0.0:0".to_string(), Some("224.0.0.224:4242".to_string()));
        assert!(iface.is_multicast());
    }

    #[test]
    fn new_reports_unicast_for_plain_udp() {
        let iface =
            UdpInterface::new("127.0.0.1:5001".to_string(), Some("127.0.0.1:5002".to_string()));
        assert!(!iface.is_multicast());
    }

    #[test]
    fn new_reports_unicast_when_no_forward_addr() {
        let iface = UdpInterface::new("0.0.0.0:0".to_string(), None::<String>);
        assert!(!iface.is_multicast());
    }

    #[test]
    fn new_multicast_reports_multicast_even_with_unicast_bind() {
        let routing = Arc::new(TokioMutex::new(PeerRouting::new()));
        let iface = UdpInterface::new_multicast(
            "127.0.0.1:5001".to_string(),
            Some("127.0.0.1:5002".to_string()),
            routing,
        );
        assert!(iface.is_multicast(), "new_multicast must always report multicast=true");
    }

    #[test]
    fn is_multicast_addr_detects_v4_link_local() {
        assert!(is_multicast_addr("224.0.0.224:4242"));
    }

    #[test]
    fn is_multicast_addr_detects_v6_link_local() {
        assert!(is_multicast_addr("[ff02::1]:4242"));
    }

    #[test]
    fn is_multicast_addr_rejects_unicast_v4() {
        assert!(!is_multicast_addr("192.168.1.112:4242"));
    }

    #[test]
    fn is_multicast_addr_rejects_wildcard() {
        assert!(!is_multicast_addr("0.0.0.0:0"));
    }

    #[test]
    fn is_multicast_addr_rejects_garbage() {
        assert!(!is_multicast_addr("not a socket addr"));
    }

    #[tokio::test]
    async fn bind_udp_enables_broadcast_for_ipv4_forward_targets() {
        let socket = bind_udp("0.0.0.0:0", Some("255.255.255.255:4242"))
            .expect("bind broadcast-capable udp socket");

        assert!(socket.broadcast().expect("read broadcast flag"));
    }

    fn fake_hash(byte: u8) -> AddressHash {
        AddressHash::new_from_hash(&crate::hash::Hash::new_from_slice(&[byte; 32]))
    }

    fn fake_peer(port: u16) -> SocketAddr {
        use std::net::{IpAddr, Ipv4Addr};
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 112)), port)
    }

    #[test]
    fn peer_routing_insert_is_bidirectional() {
        let mut r = PeerRouting::new();
        let hash = fake_hash(1);
        let peer = fake_peer(4242);
        r.insert(peer, hash);
        assert_eq!(r.hash_for_addr(&peer), Some(hash));
        assert_eq!(r.addr_for_hash(&hash), Some(peer));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn peer_routing_insert_replacing_hash_clears_old_reverse_entry() {
        let mut r = PeerRouting::new();
        let peer = fake_peer(4242);
        let h1 = fake_hash(1);
        let h2 = fake_hash(2);
        r.insert(peer, h1);
        r.insert(peer, h2);
        assert_eq!(r.hash_for_addr(&peer), Some(h2));
        assert_eq!(r.addr_for_hash(&h2), Some(peer));
        assert_eq!(r.addr_for_hash(&h1), None, "old hash should no longer map");
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn peer_routing_insert_replacing_addr_clears_old_forward_entry() {
        let mut r = PeerRouting::new();
        let hash = fake_hash(1);
        let p1 = fake_peer(4242);
        let p2 = fake_peer(5252);
        r.insert(p1, hash);
        r.insert(p2, hash);
        assert_eq!(r.addr_for_hash(&hash), Some(p2));
        assert_eq!(r.hash_for_addr(&p2), Some(hash));
        assert_eq!(r.hash_for_addr(&p1), None, "old addr should no longer map");
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn peer_routing_remove_by_hash_clears_both_directions() {
        let mut r = PeerRouting::new();
        let hash = fake_hash(1);
        let peer = fake_peer(4242);
        r.insert(peer, hash);
        assert_eq!(r.remove_by_hash(&hash), Some(peer));
        assert!(r.is_empty());
        assert_eq!(r.hash_for_addr(&peer), None);
        assert_eq!(r.addr_for_hash(&hash), None);
    }
}

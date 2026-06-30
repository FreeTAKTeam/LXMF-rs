#[cfg(test)]
mod tests {
    use super::*;

    fn random_blob(emitted: u64) -> RandomBlob {
        let mut blob = RandomBlob::default();
        blob[..5].copy_from_slice(b"tnnl!");
        blob[5..].copy_from_slice(&emitted.to_be_bytes()[3..]);
        blob
    }

    #[test]
    fn tunnel_synthesize_packet_validates_to_python_tunnel_id() {
        let identity = PrivateIdentity::new_from_name("tunnel-test");
        let interface_hash = Hash::new_from_slice(b"iface");

        let packet = synthesize_tunnel_packet(&identity, interface_hash);
        let tunnel_id = validate_tunnel_synthesize(packet.data.as_slice()).expect("valid tunnel");

        let mut expected = Vec::new();
        expected.extend_from_slice(identity.as_identity().public_key_bytes());
        expected.extend_from_slice(identity.as_identity().verifying_key_bytes());
        expected.extend_from_slice(interface_hash.as_slice());
        assert_eq!(tunnel_id, Hash::new_from_slice(&expected));
    }

    #[test]
    fn tunnel_synthesize_rejects_bad_signature() {
        let identity = PrivateIdentity::new_from_name("tunnel-test");
        let interface_hash = Hash::new_from_slice(b"iface");
        let mut packet = synthesize_tunnel_packet(&identity, interface_hash);
        let last = packet.data.len() - 1;
        packet.data.as_mut_slice()[last] ^= 0x01;

        assert!(validate_tunnel_synthesize(packet.data.as_slice()).is_err());
    }

    #[test]
    fn python_tunnel_entries_roundtrip_msgpack_shape() {
        let tunnel_id = Hash::new_from_slice(b"tunnel-id");
        let interface_hash = Hash::new_from_slice(b"iface");
        let destination = AddressHash::new_from_slice(&[1u8; 16]);
        let received_from = AddressHash::new_from_slice(&[2u8; 16]);
        let packet_hash = Hash::new_from_slice(b"packet");
        let entries = vec![PythonTunnelEntry {
            tunnel_id,
            interface_hash: Some(interface_hash),
            paths: vec![PythonTunnelPathEntry {
                destination,
                timestamp_secs: 10.0,
                    received_from,
                    hops: 2,
                    expires_secs: 20.0,
                    random_blobs: vec![random_blob(10), random_blob(11)],
                    interface_hash: Some(interface_hash),
                    packet_hash,
                }],
            expires_secs: 30.0,
        }];

        let encoded = TunnelTable::encode_python_entries(&entries).expect("encode");
        let decoded = TunnelTable::decode_python_entries(&encoded).expect("decode");

        assert_eq!(decoded, entries);
    }

    #[test]
    fn restored_python_tunnel_paths_are_returned_when_tunnel_reappears() {
        let now = Instant::now();
        let tunnel_id = Hash::new_from_slice(b"tunnel-id");
        let destination = AddressHash::new_from_slice(&[1u8; 16]);
        let received_from = AddressHash::new_from_slice(&[2u8; 16]);
        let iface = AddressHash::new_from_slice(&[3u8; 16]);
        let packet_hash = Hash::new_from_slice(b"packet");
        let mut table = TunnelTable::new();

        let restored = table.restore_python_entries(
            vec![PythonTunnelEntry {
                tunnel_id,
                interface_hash: None,
                paths: vec![PythonTunnelPathEntry {
                    destination,
                    timestamp_secs: 99.0,
                    received_from,
                    hops: 3,
                    expires_secs: 199.0,
                    random_blobs: vec![random_blob(42)],
                    interface_hash: None,
                    packet_hash,
                }],
                expires_secs: 199.0,
            }],
            now,
            100.0,
        );
        assert_eq!(restored, 1);

        let paths = table.handle_tunnel(tunnel_id, iface, now);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].destination, destination);
        assert_eq!(paths[0].received_from, received_from);
        assert_eq!(paths[0].hops, 3);
        assert_eq!(paths[0].iface, iface);
        assert_eq!(paths[0].random_blobs, vec![random_blob(42)]);
        assert_eq!(paths[0].packet_hash, packet_hash);
    }
}

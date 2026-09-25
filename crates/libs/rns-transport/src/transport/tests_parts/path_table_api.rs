#[tokio::test]
async fn path_table_filters_by_hops_and_preserves_interface_display_name() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("path-table", &identity, true));
    let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
    let next_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"next-hop"));
    let iface = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let iface = *manager.new_channel(16).address();
        assert!(manager.set_display_name(iface, Some("uplink".to_owned())));
        iface
    };
    let handler = transport.get_handler();
    assert!(handler.lock().await.path_table.restore_tunnel_path(
        destination,
        next_hop,
        2,
        iface,
        Hash::new_from_slice(b"packet"),
        std::time::Instant::now(),
    ));

    assert!(transport.path_table(Some(1)).await.expect("filtered table").is_empty());
    let entries = transport.path_table(Some(2)).await.expect("path table");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].destination, destination);
    assert_eq!(entries[0].next_hop, next_hop);
    assert_eq!(entries[0].hops, 2);
    assert_eq!(entries[0].interface_name.as_deref(), Some("uplink"));
}

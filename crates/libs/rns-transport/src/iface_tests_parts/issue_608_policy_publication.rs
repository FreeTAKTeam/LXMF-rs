#[test]
fn shared_policy_publication_failure_keeps_parent_and_children_on_the_old_policy() {
    for (poison_parent, local_child) in [(true, false), (false, false), (false, true)] {
        let mut manager = InterfaceManager::new(8);
        let parent = manager.new_channel(8);
        let parent_address = *parent.address();
        let child = manager.new_channel(8);
        let child_address = *child.address();
        let old = InterfaceSharedConfig {
            network_name: Some("old-network".to_string()),
            ..Default::default()
        };
        assert!(manager.set_shared_config(parent_address, old.clone()));
        assert!(manager.inherit_runtime_config_with_ifac(parent_address, child_address, !local_child));
        let virtual_child = manager
            .register_virtual_iface(parent_address, IfaceRole::Unicast)
            .expect("virtual child shares parent's IFAC lock");
        let second_virtual_child = manager
            .register_virtual_iface(parent_address, IfaceRole::Unicast)
            .expect("second alias of the parent's IFAC lock");
        poison_ifac_state(if poison_parent { &parent.ifac_state } else { &child.ifac_state });
        let replacement = InterfaceSharedConfig {
            network_name: Some("new-network".to_string()),
            ..Default::default()
        };
        assert!(matches!(
            manager.try_set_shared_config(parent_address, replacement.clone()),
            Err(crate::error::RnsError::ConnectionError)
        ));
        assert!(!manager.set_shared_config(parent_address, replacement));
        for address in [parent_address, virtual_child, second_virtual_child] {
            assert_eq!(manager.shared_config(&address), Some(&old));
        }
        let old_child = if local_child { shared_config_without_ifac(&old) } else { old.clone() };
        assert_eq!(manager.shared_config(&child_address), Some(&old_child));
        let healthy = if poison_parent { &child.ifac_state } else { &parent.ifac_state };
        let expected = Arc::new(std::sync::RwLock::new(old.ifac_context().expect("old IFAC")));
        let packet = Packet {
            data: crate::packet::PacketDataBuffer::new_from_slice(b"unchanged authentication"),
            ..Packet::default()
        };
        let frame = encode_packet_ifac(&expected, &packet).expect("old-key frame");
        decode_packet_ifac(healthy, &frame).expect("healthy workers retain old authentication");
    }
}

#[test]
fn shared_policy_rotation_handles_virtual_aliases_and_plain_local_children() {
    let mut manager = InterfaceManager::new(8);
    let parent = manager.new_channel(8);
    let parent_address = *parent.address();
    let child = manager.new_channel(8);
    let child_address = *child.address();
    assert!(manager.inherit_runtime_config_with_ifac(parent_address, child_address, false));
    let virtual_child = manager
        .register_virtual_iface(parent_address, IfaceRole::Unicast)
        .expect("virtual child");
    let config = InterfaceSharedConfig {
        network_name: Some("new-network".to_string()),
        announce_rate_target: Some(12),
        ..Default::default()
    };
    assert!(manager.try_set_shared_config(parent_address, config.clone()).expect("publication"));
    assert_eq!(manager.shared_config(&parent_address), Some(&config));
    assert_eq!(manager.shared_config(&virtual_child), Some(&config));
    let local = manager.shared_config(&child_address).expect("shared-local child");
    assert!(local.network_name.is_none());
    assert_eq!(local.announce_rate_target, Some(12));
    assert!(child.ifac_state.read().expect("local IFAC state").is_none());
    let expected = Arc::new(std::sync::RwLock::new(config.ifac_context().expect("new IFAC")));
    let packet = Packet {
        data: crate::packet::PacketDataBuffer::new_from_slice(b"rotated authentication"),
        ..Packet::default()
    };
    let frame = encode_packet_ifac(&expected, &packet).expect("new-key frame");
    decode_packet_ifac(&parent.ifac_state, &frame).expect("parent changed key");
}

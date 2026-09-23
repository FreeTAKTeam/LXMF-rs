#[tokio::test]
async fn rns_1_5_accepted_child_uses_parent_ifac_rotation_for_wire_admission() {
    use crate::iface::{decode_packet_ifac, encode_packet_ifac, InterfaceSharedConfig};

    let mut interfaces = crate::iface::InterfaceManager::new(8);
    let parent_channel = interfaces.new_channel(8);
    let parent = *parent_channel.address();
    let original_config = InterfaceSharedConfig {
        ifac_size: Some(128),
        network_name: Some("accepted-child-ifac".to_string()),
        passphrase: Some("accepted-child-original".to_string()),
        ..Default::default()
    };
    assert!(interfaces.set_shared_config(parent, original_config.clone()));

    // Accepted TCP/I2P streams have an independent channel whose runtime
    // configuration is inherited when the stream is admitted.
    let child_channel = interfaces.new_channel(8);
    let child = *child_channel.address();
    assert!(interfaces.inherit_runtime_config(parent, child));

    let rotated_config = InterfaceSharedConfig {
        passphrase: Some("accepted-child-rotated".to_string()),
        ..original_config
    };
    assert!(interfaces.set_shared_config(parent, rotated_config.clone()));

    let old_context = InterfaceSharedConfig {
        passphrase: Some("accepted-child-original".to_string()),
        ..rotated_config.clone()
    }
    .ifac_context_with_default_size(crate::iface::DEFAULT_IFAC_SIZE_BYTES)
    .expect("derive old IFAC context")
    .expect("old IFAC remains configured");
    let rotated_context = rotated_config
        .ifac_context_with_default_size(crate::iface::DEFAULT_IFAC_SIZE_BYTES)
        .expect("derive rotated IFAC context")
        .expect("rotated IFAC remains configured");
    let old_state = std::sync::Arc::new(std::sync::RwLock::new(Some(old_context)));
    let rotated_state = std::sync::Arc::new(std::sync::RwLock::new(Some(rotated_context)));
    let packet = Packet {
        destination: AddressHash::new_from_slice(&[0xA3; crate::hash::ADDRESS_HASH_SIZE]),
        data: PacketDataBuffer::new_from_slice(b"accepted child IFAC rotation"),
        ..Default::default()
    };
    let old_frame = encode_packet_ifac(&old_state, &packet).expect("encode old-key frame");
    let rotated_frame =
        encode_packet_ifac(&rotated_state, &packet).expect("encode rotated-key frame");

    assert!(
        decode_packet_ifac(&child_channel.ifac_state, &old_frame).is_err(),
        "an accepted child must reject frames from the parent's previous credential"
    );
    assert_eq!(
        decode_packet_ifac(&child_channel.ifac_state, &rotated_frame)
            .expect("accepted child admits frame under rotated parent credential")
            .destination,
        packet.destination
    );
}

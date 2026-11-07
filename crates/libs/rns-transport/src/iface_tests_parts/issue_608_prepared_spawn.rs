struct PreparedTestInterface;

impl Interface for PreparedTestInterface {
    fn mtu() -> usize {
        64
    }

    fn ifac_default_size_bytes() -> usize {
        8
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prepared_root_installs_carrier_policy_before_worker_constructor_and_first_poll() {
    let mut manager = InterfaceManager::new(8);
    let config = InterfaceSharedConfig {
        network_name: Some("first-frame-network".to_string()),
        ..Default::default()
    };
    let (polled, first_poll) = tokio::sync::oneshot::channel();
    let address = manager
        .try_spawn_as_with_mode(
            PreparedTestInterface,
            |context| {
                let state = context.channel.ifac_state.read().expect("prepared constructor");
                assert!(state.is_some(), "even the worker constructor observes IFAC policy");
                assert_eq!(state.as_ref().expect("IFAC context").ifac_size(), 8);
                drop(state);
                async move {
                    let packet = Packet {
                        data: crate::packet::PacketDataBuffer::new_from_slice(b"first poll"),
                        ..Packet::default()
                    };
                    let frame = encode_packet_ifac(&context.channel.ifac_state, &packet)
                        .expect("first poll authenticated frame");
                    polled.send(frame).expect("first-poll observer");
                    context.cancel.cancelled().await;
                }
            },
            IfaceRole::Unicast,
            InterfaceMode::Boundary,
            |manager, address| {
                assert!(manager.try_set_shared_config(address, config)?);
                manager.set_gravity(address, 42);
                Ok::<(), crate::error::RnsError>(())
            },
        )
        .expect("prepared spawn");
    assert_eq!(manager.mode(&address), Some(InterfaceMode::Boundary));
    assert_eq!(manager.gravity(&address), Some(42));
    assert_eq!(manager.mtu(&address), Some(64));
    let frame = tokio::time::timeout(Duration::from_secs(2), first_poll)
        .await
        .expect("first worker poll")
        .expect("first frame");
    assert_ne!(frame[0] & 0x80, 0);
    let workers = manager.take_worker_tree(address);
    assert_eq!(workers.len(), 1, "prepared worker belongs to existing lifecycle tree");
    assert!(manager.stop_interface(address));
    for worker in workers {
        tokio::time::timeout(Duration::from_secs(2), worker).await.expect("stop").expect("join");
    }
}

#[tokio::test]
async fn failed_root_preparation_never_constructs_worker_or_returns_a_live_handle() {
    let mut manager = InterfaceManager::new(8);
    let unrelated = manager.new_channel(8);
    let initial = manager.interface_hashes();
    let result = manager.try_spawn_as_with_mode_and_handle(
        PreparedTestInterface,
        |_context| -> std::future::Ready<()> {
            panic!("failed preparation must not invoke the worker constructor");
        },
        IfaceRole::Unicast,
        InterfaceMode::Boundary,
        |manager, address| {
            manager.try_set_shared_config(
                address,
                InterfaceSharedConfig { ifac_size: Some(128), ..Default::default() },
            )?;
            Ok::<(), crate::error::RnsError>(())
        },
    );
    assert!(matches!(result, Err(crate::error::RnsError::InvalidArgument)));
    assert_eq!(manager.interface_hashes(), initial);
    assert!(manager.worker_tasks.is_empty());
    assert_eq!(manager.role(unrelated.address()), Some(IfaceRole::Unicast));
}

#[test]
fn cancelled_parent_rejects_late_policy_inheritance_without_mutating_target() {
    let mut manager = InterfaceManager::new(8);
    let parent = manager.new_channel(8);
    let target = manager.new_channel(8);
    parent.stop.cancel();
    assert!(!manager.inherit_runtime_config(*parent.address(), *target.address()));
    let target_entry = manager
        .ifaces
        .iter()
        .find(|iface| iface.address == *target.address())
        .expect("unchanged target");
    assert!(target_entry.parent.is_none());
}

#[test]
fn cancelled_parent_rejects_late_virtual_registration_without_publication() {
    let mut manager = InterfaceManager::new(8);
    let parent = manager.new_channel(8);
    let before = manager.interface_hashes();
    parent.stop.cancel();
    assert!(manager.register_virtual_iface(*parent.address(), IfaceRole::VirtualUnicast).is_none());
    assert_eq!(manager.interface_hashes(), before);
}

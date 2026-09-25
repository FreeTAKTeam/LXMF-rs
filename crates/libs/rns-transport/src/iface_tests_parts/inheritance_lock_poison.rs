fn poison_ifac_state(state: &IfacState) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = state.write().expect("IFAC state lock before poisoning");
        panic!("poison IFAC state lock for regression test");
    }));
    assert!(result.is_err());
}

#[tokio::test]
async fn inherited_spawn_fails_closed_when_parent_ifac_lock_is_poisoned() {
    struct TestInterface;

    impl Interface for TestInterface {
        fn mtu() -> usize {
            64
        }
    }

    let mut manager = InterfaceManager::new(16);
    let parent = manager.new_channel(16);
    let parent_address = *parent.address();
    assert!(manager.set_shared_config(
        parent_address,
        InterfaceSharedConfig {
            network_name: Some("tcp-child-network".to_string()),
            passphrase: Some("tcp-child-secret".to_string()),
            ..Default::default()
        }
    ));
    poison_ifac_state(&parent.ifac_state);

    let worker_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_started_by_task = std::sync::Arc::clone(&worker_started);
    let child = manager.spawn_inheriting(
        parent_address,
        TestInterface,
        move |_context: InterfaceContext<TestInterface>| async move {
            worker_started_by_task.store(true, std::sync::atomic::Ordering::Release);
        },
    );

    assert!(child.is_none());
    assert!(!worker_started.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(manager.ifaces.len(), 1, "failed child must be removed");
}

#[test]
fn runtime_inheritance_leaves_target_unchanged_when_target_ifac_lock_is_poisoned() {
    let mut manager = InterfaceManager::new(16);
    let source = manager.new_channel(16);
    let target = manager.new_channel(16);
    let source_address = *source.address();
    let target_address = *target.address();
    assert!(manager.set_gravity(source_address, 42));
    poison_ifac_state(&target.ifac_state);

    assert!(!manager.inherit_runtime_config(source_address, target_address));
    assert_eq!(manager.gravity(&target_address), Some(0));
    assert_eq!(manager.shared_config(&target_address), Some(&InterfaceSharedConfig::default()));
    assert_eq!(manager.ifaces[1].parent, None);
}

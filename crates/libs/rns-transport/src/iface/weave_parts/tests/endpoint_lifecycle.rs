use super::*;
use crate::iface::{InterfaceMode, InterfaceSharedConfig};
use std::future::Future;
use std::task::Waker;

fn poll_pending<F: Future>(future: Pin<&mut F>) {
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.poll(&mut context).is_pending(), "actual endpoint handoff is blocked");
}

async fn unrelated_tree(
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
) -> (AddressHash, AddressHash) {
    let mut manager = manager.lock().await;
    let parent = manager.new_channel_with_role(8, IfaceRole::Multicast).address;
    let child = manager
        .register_virtual_iface(parent, IfaceRole::VirtualUnicast)
        .expect("unrelated native child");
    (parent, child)
}

async fn assert_cleanup(
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
    orphan: AddressHash,
    unrelated: (AddressHash, AddressHash),
) {
    cleanup_weave_endpoints(options, state.clone()).await;
    {
        let mut manager = options.iface_manager.lock().await;
        let hashes = manager.interface_hashes();
        assert!(!hashes.contains(&orphan), "stream cleanup must retire unindexed native children");
        assert!(
            hashes.contains(&options.parent_iface),
            "parent retirement is not the cleanup proof"
        );
        assert!(hashes.contains(&unrelated.0));
        assert!(hashes.contains(&unrelated.1), "another parent's child must survive");
        assert!(manager.stop_interface(options.parent_iface));
        assert!(manager.stop_interface(unrelated.0));
        assert!(manager.interface_hashes().is_empty());
    }
    assert!(state.lock().await.endpoints.is_empty());
    assert!(state.lock().await.addresses.is_empty());
}

#[tokio::test]
async fn weave_existing_and_new_endpoints_inherit_disabled_parent_policy() {
    let (options, manager, parent) = test_options().await;
    assert_eq!(WeaveInterface::ifac_default_size_bytes(), 16);
    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let existing = ensure_weave_endpoint([0xa1; ENDPOINT_ID_LEN], &options, state.clone())
        .await
        .expect("initial endpoint");
    {
        let mut manager = manager.lock().await;
        let config = InterfaceSharedConfig {
            network_name: Some("weave-policy-network".into()),
            passphrase: Some("weave-policy-credential".into()),
            ..Default::default()
        };
        assert_eq!(config.ifac_size, None);
        assert!(manager.try_set_shared_config(parent, config).expect("prepared IFAC policy"));
        assert!(manager.set_mode(parent, InterfaceMode::Gateway));
        assert!(manager.set_outgoing(parent, false));
        assert!(manager.set_gravity(parent, 7));
        assert!(manager.set_announce_pacing(parent, 9600, 23));
    }
    let created = ensure_weave_endpoint([0xa2; ENDPOINT_ID_LEN], &options, state.clone())
        .await
        .expect("new endpoint under disabled parent");
    {
        let manager = manager.lock().await;
        for address in [parent, existing, created] {
            assert_eq!(manager.mode(&address), Some(InterfaceMode::Gateway));
            assert_eq!(manager.outgoing(&address), Some(false));
            assert_eq!(manager.gravity(&address), Some(7));
            assert_eq!(manager.announce_pacing(&address), Some((9600, 23)));
            assert_eq!(
                manager
                    .shared_config(&address)
                    .expect("native inherited IFAC")
                    .passphrase
                    .as_deref(),
                Some("weave-policy-credential")
            );
        }
    }
    cleanup_weave_endpoints(&options, state).await;
    let mut manager = manager.lock().await;
    assert_eq!(manager.interface_hashes(), std::collections::HashSet::from([parent]));
    assert!(manager.stop_interface(parent));
}

#[tokio::test]
async fn weave_cleanup_retires_unindexed_child_after_registration_cancellation() {
    let (options, manager, parent) = test_options().await;
    let unrelated = unrelated_tree(&manager).await;
    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let held_manager = manager.lock().await;
    let mut registration =
        Box::pin(ensure_weave_endpoint([0xb1; ENDPOINT_ID_LEN], &options, state.clone()));
    poll_pending(registration.as_mut());
    let held_state = state.try_lock().expect("initial endpoint lookup released state");
    drop(held_manager);
    poll_pending(registration.as_mut());
    let orphan = manager
        .try_lock()
        .expect("native registration released manager before indexing")
        .traffic_snapshots()
        .into_iter()
        .find(|iface| iface.parent == Some(parent))
        .expect("native child exists before private index commit")
        .address;
    assert!(held_state.endpoints.is_empty());
    drop(registration);
    drop(held_state);
    assert_cleanup(&options, state, orphan, unrelated).await;
}

#[tokio::test]
async fn weave_cleanup_retires_unindexed_child_after_gc_cancellation() {
    let (options, manager, _parent) = test_options().await;
    let unrelated = unrelated_tree(&manager).await;
    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let orphan = ensure_weave_endpoint([0xc1; ENDPOINT_ID_LEN], &options, state.clone())
        .await
        .expect("registered endpoint to expire");
    let held_manager = manager.lock().await;
    let mut gc = Box::pin(gc_weave_endpoints(
        &options,
        state.clone(),
        Instant::now() + WEAVE_ENDPOINT_IDLE_TIMEOUT + Duration::from_secs(1),
    ));
    poll_pending(gc.as_mut());
    assert!(held_manager.interface_hashes().contains(&orphan));
    assert!(state.try_lock().expect("GC released state before retirement").endpoints.is_empty());
    drop(gc);
    drop(held_manager);
    assert_cleanup(&options, state, orphan, unrelated).await;
}

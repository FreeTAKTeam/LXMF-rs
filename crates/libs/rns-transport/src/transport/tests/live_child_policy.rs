use super::*;
use crate::iface::IfaceSource;

#[tokio::test]
async fn live_child_gravity_controls_equal_hop_equal_emitted_native_announce_ties() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("live-child-gravity", &identity, true));
    let manager = transport.iface_manager();
    let mut manager_guard = manager.lock().await;
    let parent = manager_guard.new_channel(16);
    let root = *parent.address();
    let child = manager_guard.new_channel(16);
    let child_id = *child.address();
    assert!(manager_guard.inherit_runtime_config(root, child_id));
    let unrelated = manager_guard.new_channel(16);
    let other_id = *unrelated.address();
    assert!(manager_guard.set_gravity(other_id, 1));
    drop(manager_guard);
    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "child-gravity"),
    );
    let mut packet = destination.announce(OsRng, None).expect("signed native announce");
    packet.header.hops = 2;
    // Reuse the exact signed packet: hops, random blob and emitted time cannot
    // make the replacement pass independently of the current child gravity.
    handle_announce(&packet, transport.handler.lock().await, other_id, IfaceSource::None).await;
    handle_announce(&packet, transport.handler.lock().await, child_id, IfaceSource::None).await;
    assert_eq!(
        transport
            .handler
            .lock()
            .await
            .path_table
            .get(&packet.destination)
            .expect("initial path")
            .iface,
        other_id
    );
    assert!(manager.lock().await.set_gravity(root, 2));
    assert_eq!(
        transport
            .handler
            .lock()
            .await
            .path_table
            .get(&packet.destination)
            .expect("existing path is not rewritten")
            .iface,
        other_id
    );
    handle_announce(&packet, transport.handler.lock().await, child_id, IfaceSource::None).await;
    assert_eq!(
        transport
            .handler
            .lock()
            .await
            .path_table
            .get(&packet.destination)
            .expect("higher child gravity wins")
            .iface,
        child_id
    );
    assert!(manager.lock().await.set_gravity(child_id, 0));
    assert_eq!(
        transport
            .handler
            .lock()
            .await
            .path_table
            .get(&packet.destination)
            .expect("existing child path remains")
            .iface,
        child_id
    );
    handle_announce(&packet, transport.handler.lock().await, other_id, IfaceSource::None).await;
    assert_eq!(
        transport
            .handler
            .lock()
            .await
            .path_table
            .get(&packet.destination)
            .expect("existing-path policy is read live")
            .iface,
        other_id
    );
}

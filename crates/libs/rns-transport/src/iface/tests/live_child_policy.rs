use super::*;
use crate::packet::{Header, PacketDataBuffer, PacketType};

fn assert_policy(
    manager: &InterfaceManager,
    address: AddressHash,
    mode: InterfaceMode,
    outgoing: bool,
    gravity: i64,
    pacing: (u64, u64),
) {
    assert_eq!(manager.policy(&address), Some(InterfacePolicy { mode, gravity }));
    assert_eq!(manager.outgoing(&address), Some(outgoing));
    assert_eq!(manager.announce_pacing(&address), Some(pacing));
}

#[test]
fn live_policy_setters_follow_the_registered_subtree_and_preserve_other_trees() {
    let mut manager = InterfaceManager::new(16);
    let parent = manager.new_channel(16);
    let root = *parent.address();
    assert!(manager
        .try_set_shared_config(
            root,
            InterfaceSharedConfig {
                network_name: Some("child-policy-test".into()),
                passphrase: Some("fixture-credential".into()),
                ..Default::default()
            }
        )
        .expect("valid test IFAC"));
    let virtual_child =
        manager.register_virtual_iface(root, IfaceRole::VirtualUnicast).expect("virtual child");
    let accepted = manager.new_channel(16);
    assert!(manager.inherit_runtime_config(root, *accepted.address()));
    let nested = manager
        .register_virtual_iface(*accepted.address(), IfaceRole::VirtualUnicast)
        .expect("nested virtual child");
    let plaintext = manager.new_channel(16);
    assert!(manager.inherit_runtime_config_with_ifac(root, *plaintext.address(), false));
    let other = manager.new_channel(16);
    let other_child = manager
        .register_virtual_iface(*other.address(), IfaceRole::VirtualUnicast)
        .expect("unrelated child");
    let unknown = AddressHash::new_from_hash(&Hash::new_from_slice(b"unknown policy parent"));
    let orphan = manager.new_channel(16);
    manager
        .ifaces
        .iter_mut()
        .find(|iface| iface.address == *orphan.address())
        .expect("orphan record")
        .parent = Some(unknown);
    assert!(!manager.set_mode(unknown, InterfaceMode::Gateway));
    assert!(!manager.set_gravity(unknown, 9));
    assert!(!manager.set_outgoing(unknown, false));
    assert!(!manager.set_announce_pacing(unknown, 1_200, 5));
    assert_policy(&manager, *orphan.address(), InterfaceMode::Full, true, 0, (62_500, 2));

    assert!(manager.set_mode(root, InterfaceMode::Gateway));
    assert!(manager.set_gravity(root, 9));
    assert!(manager.set_outgoing(root, false));
    assert!(manager.set_announce_pacing(root, 1_200, 5));
    for address in [root, virtual_child, *accepted.address(), nested, *plaintext.address()] {
        assert_policy(&manager, address, InterfaceMode::Gateway, false, 9, (1_200, 5));
        let iface =
            manager.ifaces.iter().find(|iface| iface.address == address).expect("subtree record");
        assert_eq!(
            iface.ifac_state.read().expect("IFAC state").is_some(),
            address != *plaintext.address()
        );
    }
    for address in [*other.address(), other_child] {
        assert_policy(&manager, address, InterfaceMode::Full, true, 0, (62_500, 2));
    }
    let later = manager.new_channel(16);
    assert!(manager.inherit_runtime_config(root, *later.address()));
    assert_policy(&manager, *later.address(), InterfaceMode::Gateway, false, 9, (1_200, 5));

    // A child-address operation owns that subtree, not its parent or siblings.
    let child = *accepted.address();
    assert!(manager.set_mode(child, InterfaceMode::Boundary));
    assert!(manager.set_gravity(child, -2));
    assert!(manager.set_outgoing(child, true));
    assert!(manager.set_announce_pacing(child, 300, 7));
    for address in [child, nested] {
        assert_policy(&manager, address, InterfaceMode::Boundary, true, -2, (300, 7));
    }
    for address in [root, virtual_child, *plaintext.address(), *later.address()] {
        assert_policy(&manager, address, InterfaceMode::Gateway, false, 9, (1_200, 5));
    }
}

#[tokio::test]
async fn live_parent_outgoing_and_mode_control_existing_child_dispatch() {
    let mut manager = InterfaceManager::new(16);
    let mut parent = manager.new_channel(16);
    let root = *parent.address();
    let mut child = manager.new_channel(16);
    assert!(manager.inherit_runtime_config(root, *child.address()));
    let virtual_child =
        manager.register_virtual_iface(root, IfaceRole::VirtualUnicast).expect("virtual child");
    let mut other = manager.new_channel(16);
    assert!(manager.set_outgoing(root, false));
    for address in [root, *child.address(), virtual_child] {
        let trace = manager
            .send(TxMessage { tx_type: TxMessageType::Direct(address), packet: Packet::default() })
            .await;
        assert_eq!(trace.sent_ifaces, 0);
    }
    assert!(parent.tx_channel.try_recv().is_err());
    assert!(child.tx_channel.try_recv().is_err());
    let trace = manager
        .send(TxMessage {
            tx_type: TxMessageType::Direct(*other.address()),
            packet: Packet::default(),
        })
        .await;
    assert_eq!(trace.sent_ifaces, 1);
    other.tx_channel.try_recv().expect("unrelated direct traffic");

    assert!(manager.set_outgoing(root, true));
    let trace = manager
        .send(TxMessage {
            tx_type: TxMessageType::Direct(virtual_child),
            packet: Packet::default(),
        })
        .await;
    assert_eq!(trace.sent_ifaces, 1);
    assert_eq!(
        parent.tx_channel.try_recv().expect("virtual direct handoff").tx_type,
        TxMessageType::Direct(virtual_child)
    );
    assert!(manager.set_mode(root, InterfaceMode::Roaming));
    let trace = manager
        .send_recursive_path_request_with_modes(
            TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() },
            Some(&[InterfaceMode::Full]),
        )
        .await;
    assert_eq!(trace.sent_ifaces, 1, "only unrelated Full interface is eligible");
    assert!(parent.tx_channel.try_recv().is_err());
    assert!(child.tx_channel.try_recv().is_err());
    other.tx_channel.try_recv().expect("unrelated recursive request");
}

#[tokio::test]
async fn live_parent_pacing_applies_on_release_without_resetting_existing_queue_deadlines() {
    let mut manager = InterfaceManager::new(16);
    let parent = manager.new_channel(16);
    let root = *parent.address();
    let mut child = manager.new_channel(16);
    let address = *child.address();
    assert!(manager.inherit_runtime_config(root, address));
    let packet = Packet {
        header: Header { packet_type: PacketType::Announce, hops: 1, ..Default::default() },
        data: PacketDataBuffer::new_from_slice(b"queued announce"),
        ..Default::default()
    };
    let blocked_until = Instant::now() + Duration::from_secs(60);
    let iface =
        manager.ifaces.iter_mut().find(|iface| iface.address == address).expect("child state");
    iface.announce_allowed_at = blocked_until;
    assert_eq!(
        manager
            .send(TxMessage {
                tx_type: TxMessageType::Broadcast(Some(root)),
                packet: packet.clone()
            })
            .await
            .queued_ifaces,
        1
    );
    let queued_at = manager
        .ifaces
        .iter()
        .find(|iface| iface.address == address)
        .expect("child state")
        .announce_queue[0]
        .queued_at;
    assert!(manager.set_announce_pacing(root, 8_000, 10));
    assert!(manager.set_mode(root, InterfaceMode::AccessPoint));
    assert!(manager.set_outgoing(root, false));
    let iface =
        manager.ifaces.iter_mut().find(|iface| iface.address == address).expect("child state");
    assert_eq!(
        iface.announce_allowed_at, blocked_until,
        "setting does not rewrite an existing scheduling commitment"
    );
    assert_eq!(iface.announce_queue[0].queued_at, queued_at);
    iface.announce_allowed_at = Instant::now();
    assert_eq!(manager.release_queued_announces().await.sent_ifaces, 0);
    let iface = manager.ifaces.iter().find(|iface| iface.address == address).expect("child state");
    assert_eq!(iface.announce_queue[0].queued_at, queued_at);
    assert!(manager.set_outgoing(root, true));
    let before = Instant::now();
    assert_eq!(
        manager.release_queued_announces().await.sent_ifaces,
        1,
        "queued mode admission stays committed; outgoing and pacing are read live"
    );
    let after = Instant::now();
    let wait = announce_wait(&packet, 8_000, 10);
    let next = manager
        .ifaces
        .iter()
        .find(|iface| iface.address == address)
        .expect("child state")
        .announce_allowed_at;
    assert!(
        next >= before + wait && next <= after + wait,
        "release reads the updated child pacing policy"
    );
    assert_eq!(child.tx_channel.try_recv().expect("released announce").packet, packet);
}

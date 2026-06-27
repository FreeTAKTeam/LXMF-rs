#[tokio::test]
async fn closed_tx_queue_stops_and_cleans_up_iface() {
    let mut mgr = InterfaceManager::new(16);
    let channel = mgr.new_channel(1);
    let iface = *channel.address();
    drop(channel);

    let trace = mgr
        .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet: Packet::default() })
        .await;

    assert_eq!(trace.matched_ifaces, 1);
    assert_eq!(trace.sent_ifaces, 0);
    assert_eq!(trace.failed_ifaces, 1);
    assert_eq!(mgr.iface_count(), 0);
}

#[tokio::test]
async fn closed_shared_tx_queue_cleans_up_virtual_ifaces() {
    let mut mgr = InterfaceManager::new(16);
    let channel = mgr.new_channel_with_role(1, IfaceRole::Multicast);
    let host = *channel.address();
    let virtual_iface =
        mgr.register_virtual_iface(host, IfaceRole::VirtualUnicast).expect("virtual iface");
    drop(channel);

    let trace = mgr
        .send(TxMessage {
            tx_type: TxMessageType::Direct(virtual_iface),
            packet: Packet::default(),
        })
        .await;

    assert_eq!(trace.matched_ifaces, 1);
    assert_eq!(trace.sent_ifaces, 0);
    assert_eq!(trace.failed_ifaces, 1);
    assert_eq!(mgr.iface_count(), 0);
}

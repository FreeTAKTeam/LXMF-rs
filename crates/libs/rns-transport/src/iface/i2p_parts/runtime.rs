use super::*;

pub(super) async fn run_interface(context: InterfaceContext<I2pInterface>) {
    let iface_stop = context.channel.stop.clone();
    let parent_iface = context.channel.address;
    let ifac_state = context.channel.ifac_state.clone();
    let ifac_violations = context.channel.ifac_violations.clone();
    let (
        name,
        sam_addr,
        peers,
        connectable,
        state_path,
        transport_identity_hash,
        mtu,
        reconnect_wait,
        runtime_status,
        iface_manager,
    ) = {
        let guard = context.inner.lock().expect("i2p interface mutex poisoned");
        (
            guard.name.clone(),
            guard.sam_addr.clone(),
            guard.peers.clone(),
            guard.connectable,
            guard.state_path.clone(),
            guard.transport_identity_hash,
            guard.mtu,
            guard.reconnect_wait,
            guard.runtime_status.clone(),
            guard.iface_manager.clone(),
        )
    };
    let (rx_channel, tx_channel) = context.channel.split();

    let peer_routes = Arc::new(tokio::sync::Mutex::new(BTreeMap::<
        AddressHash,
        tokio::sync::mpsc::Sender<TxMessage>,
    >::new()));
    for peer in peers {
        let Some(child_iface) = iface_manager
            .lock()
            .await
            .register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
        else {
            log::warn!("failed to register I2P virtual peer iface for {}", peer);
            continue;
        };
        let (peer_tx, peer_rx) = tokio::sync::mpsc::channel(128);
        peer_routes.lock().await.insert(child_iface, peer_tx);

        tokio::spawn(run_i2p_peer_loop_with_ifac(
            peer,
            child_iface,
            sam_addr.clone(),
            transport_identity_hash,
            mtu,
            reconnect_wait,
            runtime_status.clone(),
            context.cancel.clone(),
            iface_stop.clone(),
            rx_channel.clone(),
            peer_rx,
            ifac_state.clone(),
            ifac_violations.clone(),
        ));
    }

    if connectable {
        tokio::spawn(run_i2p_accept_loop_with_ifac(
            parent_iface,
            name,
            sam_addr.clone(),
            state_path.clone(),
            transport_identity_hash,
            mtu,
            reconnect_wait,
            runtime_status.clone(),
            context.cancel.clone(),
            iface_stop.clone(),
            rx_channel.clone(),
            iface_manager.clone(),
            peer_routes.clone(),
            ifac_state.clone(),
            ifac_violations.clone(),
        ));
    }

    let mut tx_channel = tx_channel;
    loop {
        tokio::select! {
            _ = context.cancel.cancelled() => break,
            _ = iface_stop.cancelled() => break,
            Some(message) = tx_channel.recv() => {
                match message.tx_type {
                    TxMessageType::Broadcast(_) => {
                        let senders = peer_routes.lock().await.values().cloned().collect::<Vec<_>>();
                        for sender in senders {
                            if let Err(err) = sender.try_send(message.clone()) {
                                log::warn!("failed to enqueue I2P broadcast packet: {err}");
                            }
                        }
                    }
                    TxMessageType::Direct(address) => {
                        let sender = peer_routes.lock().await.get(&address).cloned();
                        if let Some(sender) = sender {
                            if let Err(err) = sender.send(message).await {
                                log::warn!(
                                    "failed to enqueue I2P direct packet iface={address}: {err}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    iface_stop.cancel();
    cleanup_i2p_peer_routes(&iface_manager, &peer_routes, &runtime_status).await;
}

impl AutoDaemonStartupPlan {

    #[allow(dead_code)]
    fn spawn_peer_data_receive_loop(
        &self,
        socket: AutoBoundDataSocket,
        state: Arc<tokio::sync::Mutex<AutoDiscoveryState>>,
        dedupe: Arc<tokio::sync::Mutex<AutoInboundPacketDeduplicator>>,
        transport: Option<AutoInterfaceTransportBridge>,
        events: tokio::sync::mpsc::Sender<AutoPeerDataLoopEvent>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let plan = self.clone();
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            let started_at = Instant::now();
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    received = socket.recv_peer_data_datagram() => {
                        let datagram = match received {
                            Ok(datagram) => datagram,
                            Err(error) => {
                                let _ = events
                                    .send(AutoPeerDataLoopEvent::ReceiveFailed {
                                        ifname: socket.ifname.clone(),
                                        bind_addr: socket.bind_addr,
                                        error,
                                    })
                                    .await;
                                break;
                            }
                        };
                        let processed = {
                            let mut state = state.lock().await;
                            let mut dedupe = dedupe.lock().await;
                            plan.process_peer_data_datagram(
                                &mut state,
                                &mut dedupe,
                                datagram,
                                started_at.elapsed(),
                            )
                        };
                        if let Some(transport) = &transport {
                            transport
                                .forward_peer_data(&processed, Arc::clone(&socket.socket))
                                .await;
                        }
                        if events.send(AutoPeerDataLoopEvent::Processed(processed)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
    }

    fn spawn_peer_data_transport_tx_loop(
        &self,
        transport: AutoInterfaceTransportBridge,
        mut tx_channel: InterfaceTxReceiver,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            if *shutdown.borrow() {
                return;
            }
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    message = tx_channel.recv() => {
                        let Some(message) = message else {
                            break;
                        };
                        transport.send_outbound(message).await;
                    }
                }
            }
        })
    }
}

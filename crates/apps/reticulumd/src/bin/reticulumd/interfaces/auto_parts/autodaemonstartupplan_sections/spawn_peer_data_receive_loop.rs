impl AutoDaemonStartupPlan {

    #[allow(dead_code)]
    fn spawn_peer_data_receive_loop(
        &self,
        socket: AutoBoundDataSocket,
        runtime: AutoPeerDataReceiveLoopRuntime,
    ) -> tokio::task::JoinHandle<()> {
        let plan = self.clone();
        tokio::spawn(async move {
            let AutoPeerDataReceiveLoopRuntime {
                state,
                dedupe,
                transport,
                runtime_status,
                events,
                mut shutdown,
                started_at,
            } = runtime;
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
                        let Some(processed) = ({
                            let mut state = state.lock().await;
                            let mut dedupe = dedupe.lock().await;
                            plan.process_peer_data_datagram(
                                &mut state,
                                &mut dedupe,
                                datagram,
                                started_at.elapsed(),
                            )
                        }) else {
                            continue;
                        };
                        let forwarding = if let Some(transport) = &transport {
                            Some(transport
                                .forward_peer_data(&processed, Arc::clone(&socket.socket))
                                .await)
                        } else {
                            None
                        };
                        if let Some(runtime_status) = &runtime_status {
                            runtime_status.record_peer_data(&processed, forwarding);
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

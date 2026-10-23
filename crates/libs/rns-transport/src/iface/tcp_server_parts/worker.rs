impl TcpServer {
    pub async fn spawn(context: InterfaceContext<Self>) {
        let parent_iface = context.channel.address;
        let (
            addr,
            client_mtu,
            client_socket_tuning,
            client_hdlc_watchdog,
            client_forced_bitrate_bps,
            prefer_ipv6,
            plaintext_shared_clients,
            fast_flap_policy,
            fast_flap_tracker,
            runtime_status,
        ) = {
            let guard = context.inner.lock().unwrap();
            (
                guard.addr.clone(),
                guard.client_mtu,
                guard.client_socket_tuning,
                guard.client_hdlc_watchdog.clone(),
                guard.client_forced_bitrate_bps,
                guard.prefer_ipv6,
                guard.plaintext_shared_clients,
                guard.fast_flap_policy,
                guard.fast_flap_tracker.clone(),
                guard.runtime_status.clone(),
            )
        };

        let iface_manager = { context.inner.lock().unwrap().iface_manager.clone() };
        let mut startup_result = context.inner.lock().unwrap().startup_result.take();

        let (_, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            runtime_status.lock().expect("tcp server runtime status mutex poisoned").mark_binding();
            let listener = bind_tcp_listener(addr.clone(), prefer_ipv6).await;
            if let Some(sender) = startup_result.take() {
                let result = listener.as_ref().map(|_| ()).map_err(ToString::to_string);
                if sender.send(result).is_err() {
                    log::debug!("tcp server startup observer closed for {addr}");
                }
            }
            let listener = listener.map_err(|err| {
                runtime_status
                    .lock()
                    .expect("tcp server runtime status mutex poisoned")
                    .mark_bind_error(err.to_string());
                RnsError::ConnectionError
            });

            if listener.is_err() {
                log::warn!("couldn't bind to <{}>", addr);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            log::info!("listen on <{}>", addr);

            let listener = listener.unwrap();
            runtime_status
                .lock()
                .expect("tcp server runtime status mutex poisoned")
                .mark_listening();

            let tx_task = {
                let cancel = context.cancel.clone();
                let tx_channel = tx_channel.clone();

                tokio::spawn(async move {
                    loop {
                        if cancel.is_cancelled() {
                            break;
                        }

                        let mut tx_channel = tx_channel.lock().await;

                        tokio::select! {
                            _ = cancel.cancelled() => {
                                break;
                            }
                            // Skip all tx messages
                            _ = tx_channel.recv() => {}
                        }
                    }
                })
            };

            let cancel = context.cancel.clone();

            loop {
                if cancel.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    }

                    client = listener.accept() => {
                        match client {
                            Ok(client) => {
                                log::info!(
                                    "new client <{}> connected to <{}>",
                                    client.1,
                                    addr
                                );

                                let endpoint = client.1.to_string();
                                let peer_ip = client.1.ip().to_string();
                                if fast_flap_tracker.is_blocked(&peer_ip, fast_flap_policy) {
                                    log::warn!(
                                        "ignoring incoming connection from fast-flapping IP {}",
                                        peer_ip
                                    );
                                    continue;
                                }

                                let mut iface_manager = iface_manager.lock().await;
                                let accepted_client = TcpServer::accepted_client(
                                    endpoint.clone(),
                                    client.0,
                                    AcceptedClientOptions {
                                        client_mtu,
                                        client_socket_tuning,
                                        client_hdlc_watchdog: client_hdlc_watchdog.clone(),
                                        client_forced_bitrate_bps,
                                        fast_flap_tracker: Some(fast_flap_tracker.clone()),
                                        fast_flap_policy,
                                        peer_ip: Some(peer_ip),
                                    },
                                );
                                let child_status = accepted_client.runtime_status_handle();
                                let child = if plaintext_shared_clients {
                                    iface_manager.spawn_local_shared_client(
                                        parent_iface, accepted_client, TcpClient::spawn,
                                    )
                                } else {
                                    iface_manager.spawn_inheriting(
                                        parent_iface, accepted_client, TcpClient::spawn,
                                    )
                                };
                                let Some(child_iface) = child else {
                                    runtime_status
                                        .lock()
                                        .expect("tcp server runtime status mutex poisoned")
                                        .mark_accept_error(
                                            "accepted client runtime policy inheritance failed".to_string(),
                                        );
                                    continue;
                                };
                                runtime_status
                                    .lock()
                                    .expect("tcp server runtime status mutex poisoned")
                                    .mark_accepted(endpoint, child_iface, child_status);
                            }
                            Err(err) => {
                                runtime_status
                                    .lock()
                                    .expect("tcp server runtime status mutex poisoned")
                                    .mark_accept_error(err.to_string());
                            }
                        };
                    }
                }
            }

            if let Err(error) = tx_task.await {
                log::error!("tcp server transmit task failed listener={addr}: {error}");
            }
        }
        runtime_status.lock().expect("tcp server runtime status mutex poisoned").mark_closed();
    }
}

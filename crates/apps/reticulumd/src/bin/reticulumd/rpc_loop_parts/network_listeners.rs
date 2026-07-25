fn reserve_remote_connection(
    permits: &Arc<Semaphore>,
) -> Result<OwnedSemaphorePermit, tokio::sync::TryAcquireError> {
    Arc::clone(permits).try_acquire_owned()
}

async fn run_plain_rpc_loop(
    addr: SocketAddr,
    daemon: Arc<RpcDaemon>,
    mut shutdown: ShutdownReceiver,
) {
    let listener = TcpListener::bind(addr).await.expect("bind rpc listener");
    let connection_permits = Arc::new(Semaphore::new(RPC_MAX_REMOTE_CONNECTIONS));
    println!("{}", rpc_ready_line("http", addr));

    loop {
        tokio::select! {
            shutdown_result = shutdown.changed() => {
                if shutdown_result.is_err() || *shutdown.borrow() {
                    log::info!("[daemon] rpc tcp listener shutting down");
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, peer_addr) = accepted.expect("accept rpc socket");
                let Ok(connection_permit) =
                    reserve_remote_connection(&connection_permits)
                else {
                    log::debug!(
                        "[daemon-rpc] rejecting rpc connection at capacity peer={peer_addr}"
                    );
                    continue;
                };
                let daemon = daemon.clone();
                tokio::spawn(async move {
                    let _connection_permit = connection_permit;
                    handle_connection(stream, peer_addr, daemon.as_ref(), None).await;
                });
            }
        }
    }
}

async fn run_tls_rpc_loop(
    addr: SocketAddr,
    daemon: Arc<RpcDaemon>,
    config: RpcTlsConfig,
    mut shutdown: ShutdownReceiver,
) {
    let tls_server = build_tls_server_config(&config).expect("build rpc tls server config");
    let acceptor = TlsAcceptor::from(tls_server);
    let listener = TcpListener::bind(addr).await.expect("bind tls rpc listener");
    let connection_permits = Arc::new(Semaphore::new(RPC_MAX_REMOTE_CONNECTIONS));
    println!("{}", rpc_ready_line("https", addr));

    loop {
        tokio::select! {
            shutdown_result = shutdown.changed() => {
                if shutdown_result.is_err() || *shutdown.borrow() {
                    log::info!("[daemon] rpc tls listener shutting down");
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, peer_addr) = accepted.expect("accept tls rpc socket");
                let Ok(connection_permit) =
                    reserve_remote_connection(&connection_permits)
                else {
                    log::debug!(
                        "[daemon-rpc] rejecting tls rpc connection at capacity peer={peer_addr}"
                    );
                    continue;
                };
                let daemon = daemon.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let _connection_permit = connection_permit;
                    match timeout(
                        RPC_TLS_HANDSHAKE_TIMEOUT,
                        acceptor.accept(stream),
                    )
                    .await
                    {
                        Ok(Ok(tls_stream)) => {
                            let transport_auth = extract_transport_auth(&tls_stream);
                            handle_connection(
                                tls_stream,
                                peer_addr,
                                daemon.as_ref(),
                                Some(transport_auth),
                            )
                            .await;
                        }
                        Ok(Err(err)) => {
                            log::error!(
                                "[daemon] rpc tls handshake failed peer={} err={}",
                                peer_addr, err
                            );
                        }
                        Err(_) => {
                            log::warn!(
                                "[daemon] rpc tls handshake timed out peer={peer_addr}"
                            );
                        }
                    }
                });
            }
        }
    }
}

pub struct LocalUnixServer {
    endpoint: LocalUnixEndpoint,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    client_mtu: usize,
    client_forced_bitrate_bps: Option<u64>,
    plaintext_shared_clients: bool,
}

impl LocalUnixServer {
    pub const DEFAULT_CLIENT_MTU: usize = TcpClient::DEFAULT_MTU;

    pub fn new<P: Into<PathBuf>>(
        path: P,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        Self {
            endpoint: LocalUnixEndpoint::filesystem(path),
            iface_manager,
            client_mtu: Self::DEFAULT_CLIENT_MTU,
            client_forced_bitrate_bps: None,
            plaintext_shared_clients: false,
        }
    }

    pub fn new_endpoint(
        endpoint: LocalUnixEndpoint,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        Self {
            endpoint,
            iface_manager,
            client_mtu: Self::DEFAULT_CLIENT_MTU,
            client_forced_bitrate_bps: None,
            plaintext_shared_clients: false,
        }
    }

    #[must_use]
    pub fn with_client_mtu(mut self, client_mtu: usize) -> Self {
        self.client_mtu = client_mtu.max(256);
        self
    }

    #[must_use]
    pub fn with_client_forced_bitrate(mut self, bitrate_bps: u64) -> Self {
        self.client_forced_bitrate_bps = (bitrate_bps > 0).then_some(bitrate_bps);
        self
    }

    /// Opt into the native automatic shared-instance plaintext contract.
    /// Explicit Local interfaces inherit their configured IFAC by default.
    #[must_use]
    pub fn with_plaintext_shared_clients(mut self, enabled: bool) -> Self {
        self.plaintext_shared_clients = enabled;
        self
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let parent_iface = context.channel.address;
        let (
            endpoint,
            client_mtu,
            client_forced_bitrate_bps,
            plaintext_shared_clients,
            iface_manager,
        ) = {
            let guard = context.inner.lock().unwrap();
            (
                guard.endpoint.clone(),
                guard.client_mtu,
                guard.client_forced_bitrate_bps,
                guard.plaintext_shared_clients,
                guard.iface_manager.clone(),
            )
        };

        let (_, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));
        let endpoint_label = endpoint.label();

        let tx_task = {
            let cancel = context.cancel.clone();
            let iface_stop = iface_stop.clone();
            let tx_channel = tx_channel.clone();

            tokio::spawn(async move {
                loop {
                    if cancel.is_cancelled() || iface_stop.is_cancelled() {
                        break;
                    }

                    let mut tx_channel = tx_channel.lock().await;

                    tokio::select! {
                        _ = cancel.cancelled() => {
                            break;
                        }
                        _ = iface_stop.cancelled() => {
                            break;
                        }
                        // Listener interfaces do not transmit packets directly.
                        message = tx_channel.recv() => {
                            if message.is_none() {
                                break;
                            }
                        }
                    }
                }
            })
        };

        loop {
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            if let Some(path) = endpoint.filesystem_path() {
                if let Err(err) = prepare_socket_path(path) {
                    log::warn!("couldn't prepare local unix socket <{}>: {}", endpoint_label, err);
                    tokio::select! {
                        _ = context.cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                    }
                    continue;
                }
            }

            let listener = match UnixListener::bind(endpoint.tokio_path()) {
                Ok(listener) => listener,
                Err(err) => {
                    log::warn!("couldn't bind local unix socket <{}>: {}", endpoint_label, err);
                    tokio::select! {
                        _ = context.cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                    }
                    continue;
                }
            };

            log::info!("listen on local unix socket <{}>", endpoint_label);
            let mut counter = 0usize;

            loop {
                if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = context.cancel.cancelled() => {
                        break;
                    }
                    _ = iface_stop.cancelled() => {
                        break;
                    }
                    client = listener.accept() => {
                        match client {
                            Ok((stream, _addr)) => {
                                counter = counter.saturating_add(1);
                                let peer_label = format!("unix:{}#{}", endpoint_label, counter);
                                log::info!("new local unix client <{}> connected", peer_label);

                                let mut iface_manager = iface_manager.lock().await;
                                let mut client =
                                    LocalUnixClient::new_from_stream(peer_label, stream)
                                        .with_mtu(client_mtu);
                                if let Some(bitrate_bps) = client_forced_bitrate_bps {
                                    client = client.with_forced_bitrate(bitrate_bps);
                                }
                                let child = if plaintext_shared_clients {
                                    iface_manager.spawn_local_shared_client(
                                        parent_iface, client, LocalUnixClient::spawn,
                                    )
                                } else {
                                    iface_manager.spawn_inheriting(
                                        parent_iface, client, LocalUnixClient::spawn,
                                    )
                                };
                                if child.is_none() {
                                    log::warn!(
                                        "local unix client runtime policy inheritance failed parent={parent_iface}"
                                    );
                                    continue;
                                }
                            }
                            Err(err) => {
                                log::warn!(
                                    "local unix socket accept error <{}>: {}",
                                    endpoint_label,
                                    err
                                );
                                break;
                            }
                        }
                    }
                }
            }

            if let Some(path) = endpoint.filesystem_path() {
                if let Err(error) = remove_socket_file(path) {
                    log::warn!(
                        "local unix socket cleanup failed endpoint={} path={} error={error}",
                        endpoint_label,
                        path.display()
                    );
                }
            }
        }

        if let Some(path) = endpoint.filesystem_path() {
            if let Err(error) = remove_socket_file(path) {
                log::warn!(
                    "local unix socket cleanup failed endpoint={} path={} error={error}",
                    endpoint.label(),
                    path.display()
                );
            }
        }
        iface_stop.cancel();
        if let Err(error) = tx_task.await {
            log::error!("local unix transmit task failed endpoint={}: {error}", endpoint.label());
        }
    }

    pub async fn preflight_bind_available(endpoint: &LocalUnixEndpoint) -> std::io::Result<()> {
        if let Some(path) = endpoint.filesystem_path() {
            prepare_socket_path(path)?;
        }
        let listener = UnixListener::bind(endpoint.tokio_path())?;
        drop(listener);
        if let Some(path) = endpoint.filesystem_path() {
            remove_socket_file(path)?;
        }
        Ok(())
    }
}

impl Interface for LocalUnixServer {
    fn mtu() -> usize {
        2048
    }
}

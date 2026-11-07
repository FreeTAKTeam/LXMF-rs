pub struct LocalUnixClient {
    addr: String,
    stream: Option<UnixStream>,
    connect_endpoint: Option<LocalUnixEndpoint>,
    mtu: usize,
    forced_bitrate_bps: Option<u64>,
    reconnect_wait: Duration,
    reconnect_events: Option<tokio::sync::mpsc::Sender<crate::hash::AddressHash>>,
}

impl LocalUnixClient {
    pub const DEFAULT_RECONNECT_WAIT: Duration = Duration::from_secs(5);

    pub fn new_from_stream<T: Into<String>>(addr: T, stream: UnixStream) -> Self {
        Self {
            addr: addr.into(),
            stream: Some(stream),
            connect_endpoint: None,
            mtu: TcpClient::DEFAULT_MTU,
            forced_bitrate_bps: None,
            reconnect_wait: Self::DEFAULT_RECONNECT_WAIT,
            reconnect_events: None,
        }
    }

    pub fn new_connect(endpoint: LocalUnixEndpoint) -> Self {
        let addr = endpoint.label();
        Self {
            addr,
            stream: None,
            connect_endpoint: Some(endpoint),
            mtu: TcpClient::DEFAULT_MTU,
            forced_bitrate_bps: None,
            reconnect_wait: Self::DEFAULT_RECONNECT_WAIT,
            reconnect_events: None,
        }
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(256);
        self
    }

    #[must_use]
    pub fn with_forced_bitrate(mut self, bitrate_bps: u64) -> Self {
        self.forced_bitrate_bps = (bitrate_bps > 0).then_some(bitrate_bps);
        self
    }

    #[must_use]
    pub fn with_reconnect_wait(mut self, reconnect_wait: Duration) -> Self {
        self.reconnect_wait = reconnect_wait;
        self
    }

    #[must_use]
    pub fn with_reconnect_events(
        mut self,
        events: tokio::sync::mpsc::Sender<crate::hash::AddressHash>,
    ) -> Self {
        self.reconnect_events = Some(events);
        self
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let ifac_state: IfacState = context.channel.ifac_state.clone();
        let ifac_violations = context.channel.ifac_violations.clone();
        let (
            addr,
            mtu,
            forced_bitrate_bps,
            mut stream,
            connect_endpoint,
            reconnect_wait,
            reconnect_events,
        ) = {
            let mut guard = context.inner.lock().unwrap();
            (
                guard.addr.clone(),
                guard.mtu,
                guard.forced_bitrate_bps,
                guard.stream.take(),
                guard.connect_endpoint.clone(),
                guard.reconnect_wait,
                guard.reconnect_events.clone(),
            )
        };

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        let mut running = true;
        let mut has_connected = false;
        while running && !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
            let stream = match stream.take() {
                Some(stream) => {
                    running = false;
                    Ok(stream)
                }
                None => {
                    let Some(endpoint) = connect_endpoint.as_ref() else {
                        break;
                    };
                    UnixStream::connect(endpoint.tokio_path()).await
                }
            };

            let stream = match stream {
                Ok(stream) => stream,
                Err(err) => {
                    log::warn!("couldn't connect local unix client <{}>: {}", addr, err);
                    tokio::select! {
                        _ = context.cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(reconnect_wait) => {}
                    }
                    continue;
                }
            };

            let (read_stream, write_stream) = stream.into_split();

            log::info!("connected to local unix client <{}>", addr);
            if has_connected {
                if let Some(events) = reconnect_events.as_ref() {
                    if let Err(err) = events.try_send(iface_address) {
                        log::debug!(
                            "dropped local unix reconnect event iface={} endpoint={} err={}",
                            iface_address,
                            addr,
                            err
                        );
                    }
                }
            } else {
                has_connected = true;
            }

            if let Some(bitrate_bps) = forced_bitrate_bps {
                run_hdlc_stream_with_runtime_and_ifac(
                    "local_unix".to_string(),
                    iface_address,
                    mtu,
                    context.cancel.clone(),
                    iface_stop.clone(),
                    rx_channel.clone(),
                    tx_channel.clone(),
                    ifac_state.clone(),
                    ifac_violations.clone(),
                    read_stream,
                    write_stream,
                    HdlcStreamRuntime::new().with_forced_bitrate(bitrate_bps),
                )
                .await;
            } else {
                run_hdlc_stream_with_runtime_and_ifac(
                    "local_unix".to_string(),
                    iface_address,
                    mtu,
                    context.cancel.clone(),
                    iface_stop.clone(),
                    rx_channel.clone(),
                    tx_channel.clone(),
                    ifac_state.clone(),
                    ifac_violations.clone(),
                    read_stream,
                    write_stream,
                    HdlcStreamRuntime::default(),
                )
                .await;
            }

            log::info!("disconnected from local unix client <{}>", addr);
        }

        iface_stop.cancel();
    }
}

pub async fn preflight_unix_connect(endpoint: &LocalUnixEndpoint) -> Result<(), String> {
    UnixStream::connect(endpoint.tokio_path()).await.map(|_| ()).map_err(|err| {
        format!("local unix preflight connect failed endpoint={} err={}", endpoint.label(), err)
    })
}

impl Interface for LocalUnixClient {
    fn mtu() -> usize {
        TcpClient::DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

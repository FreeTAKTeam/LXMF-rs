impl KissTcpClientInterface {
    #[must_use]
    pub fn new<T: Into<String>>(addr: T) -> Self {
        Self {
            addr: addr.into(),
            mtu: 564,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            kiss: KissConfig::default(),
        }
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(64);
        self
    }

    #[must_use]
    pub fn with_kiss_config(mut self, kiss: KissConfig) -> Self {
        self.kiss = kiss;
        self
    }

    #[must_use]
    pub fn with_reconnect_backoff(mut self, reconnect_backoff: Duration) -> Self {
        self.reconnect_backoff = reconnect_backoff;
        if self.max_reconnect_backoff < self.reconnect_backoff {
            self.max_reconnect_backoff = self.reconnect_backoff;
        }
        self
    }

    #[must_use]
    pub fn with_max_reconnect_backoff(mut self, max_reconnect_backoff: Duration) -> Self {
        self.max_reconnect_backoff = max_reconnect_backoff.max(self.reconnect_backoff);
        self
    }

    #[must_use]
    pub fn addr(&self) -> &str {
        &self.addr
    }

    #[must_use]
    pub fn mtu(&self) -> usize {
        self.mtu
    }

    #[must_use]
    pub fn kiss_config(&self) -> KissConfig {
        self.kiss.clone()
    }

    #[must_use]
    pub fn reconnect_backoff(&self) -> Duration {
        self.reconnect_backoff
    }

    #[must_use]
    pub fn max_reconnect_backoff(&self) -> Duration {
        self.max_reconnect_backoff
    }

    pub async fn spawn(context: InterfaceContext<KissTcpClientInterface>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (addr, mtu, reconnect_backoff, max_reconnect_backoff, kiss) = {
            let guard = context.inner.lock().expect("kiss tcp client interface mutex poisoned");
            (
                guard.addr.clone(),
                guard.mtu,
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
                guard.kiss.clone(),
            )
        };

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));
        let mut active_backoff = reconnect_backoff;

        loop {
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            let stream = match TcpStream::connect(addr.clone()).await {
                Ok(stream) => stream,
                Err(err) => {
                    log::warn!("failed to connect KISS TCP endpoint={} err={}", addr, err);
                    tokio::select! {
                        _ = context.cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(active_backoff) => {}
                    }
                    active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
                    continue;
                }
            };

            log::info!("connected KISS TCP endpoint={} iface={}", addr, iface_address);
            active_backoff = reconnect_backoff;

            let stream_cancel = context.cancel.child_token();
            let stop_cancel = stream_cancel.clone();
            let iface_stop_rx = iface_stop.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = iface_stop_rx.cancelled() => stop_cancel.cancel(),
                    _ = stop_cancel.cancelled() => {}
                }
            });

            run_kiss_stream(
                stream,
                KissStreamOptions {
                    iface_address,
                    device: addr.clone(),
                    mtu,
                    flow_control: kiss.flow_control,
                    flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
                    read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
                    initial_frames: kiss.command_frames(),
                    shutdown_frames: Vec::new(),
                    id_beacon: kiss.id_beacon.clone(),
                    activity_probe: None,
                    strip_command_port_nibble: true,
                    command_tx: None,
                    data_rx_tx: None,
                },
                stream_cancel.clone(),
                rx_channel.clone(),
                tx_channel.clone(),
            )
            .await;
            stream_cancel.cancel();

            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }
            tokio::time::sleep(active_backoff).await;
            active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
        }

        iface_stop.cancel();
    }
}

impl Interface for KissTcpClientInterface {
    fn mtu() -> usize {
        564
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

#[derive(Debug, Clone)]
pub struct KissStreamOptions {
    pub iface_address: AddressHash,
    pub device: String,
    pub mtu: usize,
    pub flow_control: bool,
    pub flow_control_timeout: Duration,
    pub read_frame_timeout: Duration,
    pub initial_frames: Vec<Vec<u8>>,
    pub shutdown_frames: Vec<Vec<u8>>,
    pub id_beacon: Option<KissIdBeaconConfig>,
    pub activity_probe: Option<KissActivityProbeConfig>,
    pub strip_command_port_nibble: bool,
    pub command_tx: Option<tokio::sync::mpsc::Sender<KissCommandFrame>>,
    pub data_rx_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KissActivityProbeConfig {
    pub interval: Duration,
    pub frames: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KissCommandFrame {
    pub command: u8,
    pub payload: Vec<u8>,
}

pub async fn run_kiss_stream<IO>(
    mut stream: IO,
    options: KissStreamOptions,
    cancel: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut decoder = KissStreamDecoder::new(options.mtu)
        .with_command_port_nibble_stripping(options.strip_command_port_nibble);
    let mut read_buffer = vec![0_u8; options.mtu.max(256)];
    let mut tx_buffer = vec![0_u8; options.mtu];
    let mut pending = VecDeque::<Vec<u8>>::new();
    let mut interface_ready = true;
    let mut flow_control_locked_at: Option<Instant> = None;
    let mut first_tx_at: Option<Instant> = None;
    let mut id_tick = tokio::time::interval(Duration::from_millis(80));
    id_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut flow_control_tick = tokio::time::interval(Duration::from_millis(80));
    flow_control_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut activity_tick = tokio::time::interval(Duration::from_millis(80));
    activity_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_read_at = Instant::now();

    for frame in &options.initial_frames {
        if let Err(err) = stream.write_all(frame).await {
            log::warn!(
                "KISS init write error iface={} device={} err={}",
                options.iface_address,
                options.device,
                err
            );
            return;
        }
    }
    if let Err(err) = stream.flush().await {
        log::warn!(
            "KISS init flush error iface={} device={} err={}",
            options.iface_address,
            options.device,
            err
        );
        return;
    }
    let mut last_write_at = Instant::now();

    loop {
        let mut tx_channel = tx_channel.lock().await;
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = id_tick.tick(), if options.id_beacon.is_some() && first_tx_at.is_some() => {
                let Some(beacon) = options.id_beacon.as_ref() else {
                    continue;
                };
                let Some(first_tx) = first_tx_at else {
                    continue;
                };
                if first_tx.elapsed() >= beacon.interval {
                    let payload = beacon.payload();
                    if options.flow_control && !interface_ready {
                        pending.push_back(payload);
                    } else {
                        if write_kiss_payload(
                            &mut stream,
                            &options,
                            &mut interface_ready,
                            &mut flow_control_locked_at,
                            payload,
                        )
                        .await
                        {
                            last_write_at = Instant::now();
                        }
                        first_tx_at = None;
                    }
                }
            }
            _ = flow_control_tick.tick(), if options.flow_control && !interface_ready => {
                if flow_control_locked_at
                    .is_some_and(|locked_at| locked_at.elapsed() >= options.flow_control_timeout)
                {
                    log::warn!(
                        "KISS flow control timeout iface={} device={} timeout_ms={} unlocking missed READY",
                        options.iface_address,
                        options.device,
                        options.flow_control_timeout.as_millis()
                    );
                    interface_ready = true;
                    flow_control_locked_at = None;
                    flush_pending_kiss(
                        &mut stream,
                        &options,
                        &mut interface_ready,
                        &mut flow_control_locked_at,
                        &mut pending,
                        &mut first_tx_at,
                        &mut last_write_at,
                    )
                    .await;
                }
            }
            _ = activity_tick.tick(), if options.activity_probe.is_some() => {
                let Some(probe) = options.activity_probe.as_ref() else {
                    continue;
                };
                if last_write_at.elapsed() >= probe.interval
                    && write_raw_kiss_frames(&mut stream, &options, &probe.frames, "activity probe").await
                {
                    last_write_at = Instant::now();
                }
            }
            result = stream.read(&mut read_buffer[..]) => {
                match result {
                    Ok(0) => break,
                    Ok(n) => {
                        if decoder.has_partial_frame()
                            && last_read_at.elapsed() >= options.read_frame_timeout
                        {
                            decoder.clear_partial_frame();
                        }
                        last_read_at = Instant::now();
                        match decoder.push_bytes(&read_buffer[..n]) {
                            Ok(frames) => {
                                for frame in frames {
                                    match frame {
                                        KissFrame::Data(payload) => {
                                            if let Some(data_rx_tx) = &options.data_rx_tx {
                                                let _ = data_rx_tx.try_send(());
                                            }
                                            if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(&payload)) {
                                                let _ = rx_channel
                                                    .send(RxMessage {
                                                        address: options.iface_address,
                                                        packet,
                                                        source: IfaceSource::None,
                                                    })
                                                    .await;
                                            }
                                        }
                                        KissFrame::Command(KissCommand::Ready) => {
                                            interface_ready = true;
                                            flow_control_locked_at = None;
                                            flush_pending_kiss(
                                                &mut stream,
                                                &options,
                                                &mut interface_ready,
                                                &mut flow_control_locked_at,
                                                &mut pending,
                                                &mut first_tx_at,
                                                &mut last_write_at,
                                            )
                                            .await;
                                        }
                                        KissFrame::Command(KissCommand::Unknown(command, payload)) => {
                                            if let Some(command_tx) = &options.command_tx {
                                                let _ = command_tx.try_send(KissCommandFrame { command, payload });
                                            }
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                log::warn!(
                                    "KISS decode error iface={} device={} err={:?}",
                                    options.iface_address,
                                    options.device,
                                    err
                                );
                            }
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "KISS read error iface={} device={} err={}",
                            options.iface_address,
                            options.device,
                            err
                        );
                        break;
                    }
                }
            }
            Some(message) = tx_channel.recv() => {
                let mut output = OutputBuffer::new(&mut tx_buffer[..]);
                if message.packet.serialize(&mut output).is_ok() {
                    let payload = output.as_slice().to_vec();
                    if options.flow_control && !interface_ready {
                        pending.push_back(payload);
                    } else {
                        if write_kiss_payload(
                            &mut stream,
                            &options,
                            &mut interface_ready,
                            &mut flow_control_locked_at,
                            payload,
                        )
                        .await
                        {
                            last_write_at = Instant::now();
                        }
                        if first_tx_at.is_none() {
                            first_tx_at = Some(Instant::now());
                        }
                    }
                } else {
                    log::warn!(
                        "KISS packet serialize failed iface={} device={} mtu={}",
                        options.iface_address,
                        options.device,
                        options.mtu
                    );
                }
            }
        }
    }

    write_shutdown_frames(&mut stream, &options).await;
}

async fn write_shutdown_frames<IO>(stream: &mut IO, options: &KissStreamOptions)

where
    IO: AsyncWrite + Unpin,
{
    if options.shutdown_frames.is_empty() {
        return;
    }
    for frame in &options.shutdown_frames {
        if let Err(err) = stream.write_all(frame).await {
            log::warn!(
                "KISS shutdown write error iface={} device={} err={}",
                options.iface_address,
                options.device,
                err
            );
            return;
        }
    }
    if let Err(err) = stream.flush().await {
        log::warn!(
            "KISS shutdown flush error iface={} device={} err={}",
            options.iface_address,
            options.device,
            err
        );
    }
}

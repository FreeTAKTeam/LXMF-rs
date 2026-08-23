impl KissTcpClientInterface {
    #[must_use]
    pub fn new<T: Into<String>>(addr: T) -> Self {
        let addr = addr.into();
        let mtu = 564;
        let kiss = KissConfig::default();
        let runtime_status =
            KissRuntimeStatusHandle::new(KissRuntimeStatus::new_tcp(addr.clone(), mtu, &kiss));
        Self {
            addr,
            mtu,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            kiss,
            runtime_status,
        }
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(64);
        self.runtime_status.update(|status| {
            status.mtu = self.mtu;
        });
        self
    }

    #[must_use]
    pub fn with_kiss_config(mut self, kiss: KissConfig) -> Self {
        self.kiss = kiss;
        self.runtime_status.update(|status| {
            status.preamble_ms = self.kiss.preamble_ms;
            status.tx_tail_ms = self.kiss.tx_tail_ms;
            status.persistence = self.kiss.persistence;
            status.slot_time_ms = self.kiss.slot_time_ms;
            status.kiss_flow_control = self.kiss.flow_control;
        });
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

    #[must_use]
    pub fn runtime_status_handle(&self) -> KissRuntimeStatusHandle {
        self.runtime_status.clone()
    }

    pub async fn spawn(context: InterfaceContext<KissTcpClientInterface>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (addr, mtu, reconnect_backoff, max_reconnect_backoff, kiss, runtime_status) = {
            let guard = context.inner.lock().expect("kiss tcp client interface mutex poisoned");
            (
                guard.addr.clone(),
                guard.mtu,
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
                guard.kiss.clone(),
                guard.runtime_status.clone(),
            )
        };
        runtime_status.update(|status| {
            status.iface = Some(iface_address.to_string());
        });

        let online = context.channel.online.clone();
        online.store(false, std::sync::atomic::Ordering::Release);
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
                    runtime_status.update(|status| {
                        status.link_state = "connect_failed".to_string();
                        status.connect_errors = status.connect_errors.saturating_add(1);
                        status.reconnect_attempts = status.reconnect_attempts.saturating_add(1);
                        status.last_error = Some(err.to_string());
                    });
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
            runtime_status.update(|status| {
                status.link_state = "connected".to_string();
                status.last_error = None;
            });
            online.store(true, std::sync::atomic::Ordering::Release);

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
                    payload_adapter: KissPayloadAdapter::Raw,
                    strip_command_port_nibble: true,
                    command_tx: None,
                    data_rx_tx: None,
                    management_frame_rx: None,
                    runtime_status: Some(runtime_status.clone()),
                },
                stream_cancel.clone(),
                rx_channel.clone(),
                tx_channel.clone(),
            )
            .await;
            stream_cancel.cancel();
            online.store(false, std::sync::atomic::Ordering::Release);

            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }
            tokio::time::sleep(active_backoff).await;
            active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
        }

        online.store(false, std::sync::atomic::Ordering::Release);
        iface_stop.cancel();
        runtime_status.update(|status| {
            status.link_state = "stopped".to_string();
        });
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

#[derive(Debug)]
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
    pub payload_adapter: KissPayloadAdapter,
    pub strip_command_port_nibble: bool,
    pub command_tx: Option<tokio::sync::mpsc::Sender<KissCommandFrame>>,
    pub data_rx_tx: Option<tokio::sync::mpsc::Sender<()>>,
    pub management_frame_rx: Option<Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>>,
    pub runtime_status: Option<KissRuntimeStatusHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum KissPayloadAdapter {
    #[default]
    Raw,
    Ax25(Ax25KissPayloadConfig),
}

impl KissPayloadAdapter {
    fn inbound(&self, payload: &[u8]) -> Option<Vec<u8>> {
        match self {
            Self::Raw => Some(payload.to_vec()),
            Self::Ax25(_) => decode_ax25_ui_payload(payload).map(Vec::from),
        }
    }

    fn outbound(&self, payload: &[u8]) -> Vec<u8> {
        match self {
            Self::Raw => payload.to_vec(),
            Self::Ax25(config) => encode_ax25_ui_payload(payload, config),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ax25KissPayloadConfig {
    pub src_call: Vec<u8>,
    pub src_ssid: u8,
    pub dst_call: Vec<u8>,
    pub dst_ssid: u8,
}

impl Ax25KissPayloadConfig {
    pub fn new(src_call: impl AsRef<str>, src_ssid: u8) -> Result<Self, String> {
        Self::with_destination(src_call, src_ssid, "APZRNS", 0)
    }

    pub fn with_destination(
        src_call: impl AsRef<str>,
        src_ssid: u8,
        dst_call: impl AsRef<str>,
        dst_ssid: u8,
    ) -> Result<Self, String> {
        let src_call = normalize_ax25_call(src_call.as_ref(), "ax25_kiss.callsign")?;
        let dst_call = normalize_ax25_call(dst_call.as_ref(), "ax25_kiss.destination_callsign")?;
        if src_ssid > 15 {
            return Err("ax25_kiss.ssid must be between 0 and 15".to_string());
        }
        if dst_ssid > 15 {
            return Err("ax25_kiss.destination_ssid must be between 0 and 15".to_string());
        }
        Ok(Self { src_call, src_ssid, dst_call, dst_ssid })
    }
}

const AX25_HEADER_SIZE: usize = 16;
const AX25_CTRL_UI: u8 = 0x03;
const AX25_PID_NOLAYER3: u8 = 0xF0;

fn normalize_ax25_call(value: &str, field: &str) -> Result<Vec<u8>, String> {
    let call = value.trim().to_ascii_uppercase();
    if !(3..=6).contains(&call.len()) || !call.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return Err(format!("{field} must be 3 to 6 ASCII alphanumeric characters"));
    }
    Ok(call.into_bytes())
}

fn encode_ax25_ui_payload(payload: &[u8], config: &Ax25KissPayloadConfig) -> Vec<u8> {
    let mut output = Vec::with_capacity(AX25_HEADER_SIZE + payload.len());
    push_ax25_call(&mut output, &config.dst_call);
    output.push(0x60 | (config.dst_ssid << 1));
    push_ax25_call(&mut output, &config.src_call);
    output.push(0x60 | (config.src_ssid << 1) | 0x01);
    output.push(AX25_CTRL_UI);
    output.push(AX25_PID_NOLAYER3);
    output.extend_from_slice(payload);
    output
}

fn push_ax25_call(output: &mut Vec<u8>, call: &[u8]) {
    for index in 0..6 {
        let byte = call.get(index).copied().unwrap_or(b' ');
        output.push(byte << 1);
    }
}

fn decode_ax25_ui_payload(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() <= AX25_HEADER_SIZE {
        return None;
    }
    Some(&payload[AX25_HEADER_SIZE..])
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KissDataFrameKind {
    Packet,
    IdBeacon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KissRawFrameKind {
    Init,
    Shutdown,
    Management,
    Activity,
}

#[derive(Debug)]
struct PendingKissPayload {
    payload: Vec<u8>,
    kind: KissDataFrameKind,
}

pub async fn run_kiss_stream<IO>(
    mut stream: IO,
    mut options: KissStreamOptions,
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
    let mut pending = VecDeque::<PendingKissPayload>::new();
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
    let mut management_frame_rx = options.management_frame_rx.take();

    update_kiss_status(&options, |status| {
        status.link_state = "running".to_string();
        status.interface_ready = true;
        status.pending_depth = 0;
        status.last_error = None;
    });
    if !options.initial_frames.is_empty()
        && !write_raw_kiss_frames(
            &mut stream,
            &options,
            &options.initial_frames,
            "init",
            KissRawFrameKind::Init,
        )
        .await
    {
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
                        pending.push_back(PendingKissPayload {
                            payload,
                            kind: KissDataFrameKind::IdBeacon,
                        });
                        update_kiss_pending_depth(&options, pending.len());
                    } else {
                        if write_kiss_payload(
                            &mut stream,
                            &options,
                            &mut interface_ready,
                            &mut flow_control_locked_at,
                            payload,
                            KissDataFrameKind::IdBeacon,
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
                    update_kiss_status(&options, |status| {
                        status.interface_ready = true;
                        status.flow_control_timeouts = status.flow_control_timeouts.saturating_add(1);
                    });
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
                    && write_raw_kiss_frames(
                        &mut stream,
                        &options,
                        &probe.frames,
                        "activity probe",
                        KissRawFrameKind::Activity,
                    )
                    .await
                {
                    last_write_at = Instant::now();
                }
            }
            frame = recv_optional_management_frame(&mut management_frame_rx), if management_frame_rx.is_some() => {
                match frame {
                    Some(frame) => {
                        if write_raw_kiss_frames(
                            &mut stream,
                            &options,
                            &[frame],
                            "management command",
                            KissRawFrameKind::Management,
                        )
                        .await
                        {
                            last_write_at = Instant::now();
                        }
                    }
                    None => {
                        management_frame_rx = None;
                    }
                }
            }
            result = stream.read(&mut read_buffer[..]) => {
                match result {
                    Ok(0) => {
                        update_kiss_status(&options, |status| {
                            status.link_state = "eof".to_string();
                            status.eof_count = status.eof_count.saturating_add(1);
                        });
                        break;
                    }
                    Ok(n) => {
                        update_kiss_status(&options, |status| {
                            status.bytes_rx = status.bytes_rx.saturating_add(n as u64);
                        });
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
                                            update_kiss_status(&options, |status| {
                                                status.data_frames_rx =
                                                    status.data_frames_rx.saturating_add(1);
                                            });
                                            if let Some(data_rx_tx) = &options.data_rx_tx {
                                                if let Err(err) = data_rx_tx.try_send(()) {
                                                    log::warn!("KISS data notification dropped: {err}");
                                                    update_kiss_status(&options, |status| {
                                                        status.data_notifications_dropped = status
                                                            .data_notifications_dropped
                                                            .saturating_add(1);
                                                        status.last_error = Some(err.to_string());
                                                    });
                                                }
                                            }
                                            let Some(payload) = options.payload_adapter.inbound(&payload) else {
                                                update_kiss_status(&options, |status| {
                                                    status.ax25_drops =
                                                        status.ax25_drops.saturating_add(1);
                                                    status.last_error =
                                                        Some("AX.25 UI payload too short".to_string());
                                                });
                                                continue;
                                            };
                                            if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(&payload)) {
                                                match rx_channel
                                                    .send(RxMessage {
                                                        address: options.iface_address,
                                                        packet,
                                                        source: IfaceSource::None,
                                                    })
                                                    .await
                                                {
                                                    Ok(()) => {
                                                        update_kiss_status(&options, |status| {
                                                            status.packets_rx =
                                                                status.packets_rx.saturating_add(1);
                                                            status.last_error = None;
                                                        });
                                                    }
                                                    Err(err) => {
                                                        update_kiss_status(&options, |status| {
                                                            status.rx_queue_errors = status
                                                                .rx_queue_errors
                                                                .saturating_add(1);
                                                            status.last_error = Some(err.to_string());
                                                        });
                                                    }
                                                }
                                            } else {
                                                update_kiss_status(&options, |status| {
                                                    status.deserialize_errors =
                                                        status.deserialize_errors.saturating_add(1);
                                                    status.last_error =
                                                        Some("packet deserialize failed".to_string());
                                                });
                                            }
                                        }
                                        KissFrame::Command(KissCommand::Ready) => {
                                            interface_ready = true;
                                            flow_control_locked_at = None;
                                            update_kiss_status(&options, |status| {
                                                status.command_frames_rx =
                                                    status.command_frames_rx.saturating_add(1);
                                                status.ready_frames_rx =
                                                    status.ready_frames_rx.saturating_add(1);
                                                status.interface_ready = true;
                                            });
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
                                            update_kiss_status(&options, |status| {
                                                status.command_frames_rx =
                                                    status.command_frames_rx.saturating_add(1);
                                            });
                                            if let Some(command_tx) = &options.command_tx {
                                                if let Err(err) =
                                                    command_tx.try_send(KissCommandFrame { command, payload })
                                                {
                                                    log::warn!(
                                                        "KISS command notification dropped: {err}"
                                                    );
                                                    update_kiss_status(&options, |status| {
                                                        status.command_notifications_dropped = status
                                                            .command_notifications_dropped
                                                            .saturating_add(1);
                                                        status.last_error = Some(err.to_string());
                                                    });
                                                }
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
                                update_kiss_status(&options, |status| {
                                    status.decode_errors = status.decode_errors.saturating_add(1);
                                    status.last_error = Some(format!("{err:?}"));
                                });
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
                        update_kiss_status(&options, |status| {
                            status.link_state = "read_error".to_string();
                            status.read_errors = status.read_errors.saturating_add(1);
                            status.last_error = Some(err.to_string());
                        });
                        break;
                    }
                }
            }
            Some(message) = tx_channel.recv() => {
                let mut output = OutputBuffer::new(&mut tx_buffer[..]);
                if message.packet.serialize(&mut output).is_ok() {
                    let payload = options.payload_adapter.outbound(output.as_slice());
                    if options.flow_control && !interface_ready {
                        pending.push_back(PendingKissPayload {
                            payload,
                            kind: KissDataFrameKind::Packet,
                        });
                        update_kiss_pending_depth(&options, pending.len());
                    } else {
                        if write_kiss_payload(
                            &mut stream,
                            &options,
                            &mut interface_ready,
                            &mut flow_control_locked_at,
                            payload,
                            KissDataFrameKind::Packet,
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
                    update_kiss_status(&options, |status| {
                        status.serialize_errors = status.serialize_errors.saturating_add(1);
                        status.last_error = Some("packet serialize failed".to_string());
                    });
                }
            }
        }
    }

    write_shutdown_frames(&mut stream, &options).await;
    update_kiss_status(&options, |status| {
        status.link_state = "closed".to_string();
        status.interface_ready = interface_ready;
        status.pending_depth = pending.len();
    });
}

async fn recv_optional_management_frame(
    management_frame_rx: &mut Option<Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>>,
) -> Option<Vec<u8>> {
    match management_frame_rx {
        Some(rx) => rx.lock().await.recv().await,
        None => None,
    }
}

async fn write_shutdown_frames<IO>(stream: &mut IO, options: &KissStreamOptions)

where
    IO: AsyncWrite + Unpin,
{
    let _ = write_raw_kiss_frames(
        stream,
        options,
        &options.shutdown_frames,
        "shutdown",
        KissRawFrameKind::Shutdown,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ax25_payload_adapter_wraps_reticulum_payload_in_python_ui_header() {
        let config = Ax25KissPayloadConfig::new("n0call", 1).expect("ax25 config");
        let adapter = KissPayloadAdapter::Ax25(config);
        let payload = b"reticulum-packet";

        let wrapped = adapter.outbound(payload);

        assert_eq!(wrapped.len(), AX25_HEADER_SIZE + payload.len());
        assert_eq!(
            &wrapped[..AX25_HEADER_SIZE],
            &[
                b'A' << 1,
                b'P' << 1,
                b'Z' << 1,
                b'R' << 1,
                b'N' << 1,
                b'S' << 1,
                0x60,
                b'N' << 1,
                b'0' << 1,
                b'C' << 1,
                b'A' << 1,
                b'L' << 1,
                b'L' << 1,
                0x60 | (1 << 1) | 0x01,
                AX25_CTRL_UI,
                AX25_PID_NOLAYER3,
            ]
        );
        assert_eq!(adapter.inbound(&wrapped).as_deref(), Some(payload.as_slice()));
    }

    #[test]
    fn ax25_payload_adapter_strips_any_long_python_ax25_header() {
        let config = Ax25KissPayloadConfig::new("N0CALL", 0).expect("ax25 config");
        let adapter = KissPayloadAdapter::Ax25(config);
        let mut frame = vec![0xAA; AX25_HEADER_SIZE];
        frame.extend_from_slice(b"packet");

        assert_eq!(adapter.inbound(&frame).as_deref(), Some(&b"packet"[..]));
        assert_eq!(adapter.inbound(&frame[..AX25_HEADER_SIZE]), None);
    }

    #[test]
    fn ax25_payload_config_validates_python_callsign_and_ssid_bounds() {
        assert!(Ax25KissPayloadConfig::new("ab1", 15).is_ok());
        assert!(Ax25KissPayloadConfig::new("ab", 0).is_err());
        assert!(Ax25KissPayloadConfig::new("abcdefg", 0).is_err());
        assert!(Ax25KissPayloadConfig::new("ab-1", 0).is_err());
        assert!(Ax25KissPayloadConfig::new("ab1", 16).is_err());
    }
}

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::hash::AddressHash;
use crate::iface::{IfaceSource, RxMessage, TxMessage};
use crate::kiss::{
    encode_command_frame, encode_data_frame, KissCommand, KissFrame, KissStreamDecoder, CMD_P,
    CMD_READY, CMD_SLOTTIME, CMD_TXDELAY, CMD_TXTAIL,
};
use crate::packet::Packet;
use crate::serde::Serialize;

use super::{Interface, InterfaceContext};

pub const KISS_FLOW_CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
pub const KISS_READ_FRAME_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KissIdBeaconConfig {
    pub callsign: Vec<u8>,
    pub interval: Duration,
    pub min_payload_len: usize,
}

impl KissIdBeaconConfig {
    #[must_use]
    pub fn payload(&self) -> Vec<u8> {
        let mut payload = self.callsign.clone();
        payload.resize(payload.len().max(self.min_payload_len), 0);
        payload
    }

    #[must_use]
    pub fn matches_payload(&self, payload: &[u8]) -> bool {
        self.payload().as_slice() == payload
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KissConfig {
    pub preamble_ms: u16,
    pub tx_tail_ms: u16,
    pub persistence: u8,
    pub slot_time_ms: u16,
    pub flow_control: bool,
    pub id_beacon: Option<KissIdBeaconConfig>,
}

impl Default for KissConfig {
    fn default() -> Self {
        Self {
            preamble_ms: 350,
            tx_tail_ms: 20,
            persistence: 64,
            slot_time_ms: 20,
            flow_control: false,
            id_beacon: None,
        }
    }
}

impl KissConfig {
    #[must_use]
    pub fn command_frames(&self) -> Vec<Vec<u8>> {
        let mut frames = vec![
            encode_command_frame(CMD_TXDELAY, &[ms_to_tens(self.preamble_ms)]),
            encode_command_frame(CMD_TXTAIL, &[ms_to_tens(self.tx_tail_ms)]),
            encode_command_frame(CMD_P, &[self.persistence]),
            encode_command_frame(CMD_SLOTTIME, &[ms_to_tens(self.slot_time_ms)]),
        ];
        frames.push(encode_command_frame(CMD_READY, &[1]));
        frames
    }
}

fn ms_to_tens(value: u16) -> u8 {
    (value / 10).min(u16::from(u8::MAX)) as u8
}

#[derive(Debug, Clone)]
pub struct KissInterface {
    device: String,
    baud_rate: u32,
    data_bits: DataBits,
    parity: Parity,
    stop_bits: StopBits,
    mtu: usize,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
    kiss: KissConfig,
}

impl KissInterface {
    #[must_use]
    pub fn new<T: Into<String>>(device: T, baud_rate: u32) -> Self {
        Self {
            device: device.into(),
            baud_rate,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            mtu: 564,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            kiss: KissConfig::default(),
        }
    }

    #[must_use]
    pub fn device(&self) -> &str {
        &self.device
    }

    #[must_use]
    pub fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    #[must_use]
    pub fn data_bits_value(&self) -> u8 {
        match self.data_bits {
            DataBits::Five => 5,
            DataBits::Six => 6,
            DataBits::Seven => 7,
            DataBits::Eight => 8,
        }
    }

    #[must_use]
    pub fn parity_name(&self) -> &'static str {
        match self.parity {
            Parity::None => "none",
            Parity::Odd => "odd",
            Parity::Even => "even",
        }
    }

    #[must_use]
    pub fn stop_bits_value(&self) -> u8 {
        match self.stop_bits {
            StopBits::One => 1,
            StopBits::Two => 2,
        }
    }

    #[must_use]
    pub fn with_data_bits(mut self, data_bits: DataBits) -> Self {
        self.data_bits = data_bits;
        self
    }

    pub fn with_data_bits_raw(self, data_bits: u8) -> Result<Self, String> {
        let data_bits = match data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            8 => DataBits::Eight,
            _ => {
                return Err(format!("kiss.data_bits must be one of: 5, 6, 7, 8 (got {data_bits})"))
            }
        };
        Ok(self.with_data_bits(data_bits))
    }

    #[must_use]
    pub fn with_parity(mut self, parity: Parity) -> Self {
        self.parity = parity;
        self
    }

    pub fn with_parity_name(self, parity: &str) -> Result<Self, String> {
        let parity = match parity.trim().to_ascii_lowercase().as_str() {
            "n" | "none" => Parity::None,
            "e" | "even" => Parity::Even,
            "o" | "odd" => Parity::Odd,
            _ => {
                return Err(format!(
                    "kiss.parity must be one of: n, none, e, even, o, odd (got {parity})"
                ))
            }
        };
        Ok(self.with_parity(parity))
    }

    #[must_use]
    pub fn with_stop_bits(mut self, stop_bits: StopBits) -> Self {
        self.stop_bits = stop_bits;
        self
    }

    pub fn with_stop_bits_raw(self, stop_bits: u8) -> Result<Self, String> {
        let stop_bits = match stop_bits {
            1 => StopBits::One,
            2 => StopBits::Two,
            _ => return Err(format!("kiss.stop_bits must be one of: 1, 2 (got {stop_bits})")),
        };
        Ok(self.with_stop_bits(stop_bits))
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

    pub fn preflight_open(&self) -> Result<(), String> {
        tokio_serial::new(self.device.clone(), self.baud_rate)
            .data_bits(self.data_bits)
            .parity(self.parity)
            .stop_bits(self.stop_bits)
            .flow_control(FlowControl::None)
            .open_native_async()
            .map(|_| ())
            .map_err(|err| {
                format!(
                    "kiss preflight open failed device={} baud_rate={} err={}",
                    self.device, self.baud_rate, err
                )
            })
    }

    pub async fn spawn(context: InterfaceContext<KissInterface>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (
            device,
            baud_rate,
            data_bits,
            parity,
            stop_bits,
            mtu,
            reconnect_backoff,
            max_reconnect_backoff,
            kiss,
        ) = {
            let guard = context.inner.lock().expect("kiss interface mutex poisoned");
            (
                guard.device.clone(),
                guard.baud_rate,
                guard.data_bits,
                guard.parity,
                guard.stop_bits,
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
            if context.cancel.is_cancelled() {
                break;
            }

            let port = match tokio_serial::new(device.clone(), baud_rate)
                .data_bits(data_bits)
                .parity(parity)
                .stop_bits(stop_bits)
                .flow_control(FlowControl::None)
                .open_native_async()
            {
                Ok(port) => port,
                Err(err) => {
                    log::warn!(
                        "failed to open KISS device={} baud_rate={} err={}",
                        device,
                        baud_rate,
                        err
                    );
                    tokio::time::sleep(active_backoff).await;
                    active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
                    continue;
                }
            };

            log::info!(
                "opened KISS device={} baud_rate={} iface={}",
                device,
                baud_rate,
                iface_address
            );
            active_backoff = reconnect_backoff;

            run_kiss_stream(
                port,
                KissStreamOptions {
                    iface_address,
                    device: device.clone(),
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
                context.cancel.clone(),
                rx_channel.clone(),
                tx_channel.clone(),
            )
            .await;

            if context.cancel.is_cancelled() {
                break;
            }
            tokio::time::sleep(active_backoff).await;
            active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
        }

        iface_stop.cancel();
    }
}

impl Interface for KissInterface {
    fn mtu() -> usize {
        564
    }
}

#[derive(Debug, Clone)]
pub struct KissTcpClientInterface {
    addr: String,
    mtu: usize,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
    kiss: KissConfig,
}

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

async fn flush_pending_kiss<IO>(
    stream: &mut IO,
    options: &KissStreamOptions,
    interface_ready: &mut bool,
    flow_control_locked_at: &mut Option<Instant>,
    pending: &mut VecDeque<Vec<u8>>,
    first_tx_at: &mut Option<Instant>,
    last_write_at: &mut Instant,
) where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    while *interface_ready {
        let Some(payload) = pending.pop_front() else {
            break;
        };
        let is_id_beacon =
            options.id_beacon.as_ref().is_some_and(|beacon| beacon.matches_payload(&payload));
        if write_kiss_payload(stream, options, interface_ready, flow_control_locked_at, payload)
            .await
        {
            *last_write_at = Instant::now();
        }
        if is_id_beacon {
            *first_tx_at = None;
        } else if first_tx_at.is_none() {
            *first_tx_at = Some(Instant::now());
        }
    }
}

async fn write_kiss_payload<IO>(
    stream: &mut IO,
    options: &KissStreamOptions,
    interface_ready: &mut bool,
    flow_control_locked_at: &mut Option<Instant>,
    payload: Vec<u8>,
) -> bool
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let frame = encode_data_frame(&payload);
    if let Err(err) = stream.write_all(&frame).await {
        log::warn!(
            "KISS write error iface={} device={} err={}",
            options.iface_address,
            options.device,
            err
        );
        return false;
    }
    if let Err(err) = stream.flush().await {
        log::warn!(
            "KISS flush error iface={} device={} err={}",
            options.iface_address,
            options.device,
            err
        );
        return false;
    }
    if options.flow_control {
        *interface_ready = false;
        *flow_control_locked_at = Some(Instant::now());
    }
    true
}

async fn write_raw_kiss_frames<IO>(
    stream: &mut IO,
    options: &KissStreamOptions,
    frames: &[Vec<u8>],
    reason: &str,
) -> bool
where
    IO: AsyncWrite + Unpin,
{
    if frames.is_empty() {
        return false;
    }
    for frame in frames {
        if let Err(err) = stream.write_all(frame).await {
            log::warn!(
                "KISS {} write error iface={} device={} err={}",
                reason,
                options.iface_address,
                options.device,
                err
            );
            return false;
        }
    }
    if let Err(err) = stream.flush().await {
        log::warn!(
            "KISS {} flush error iface={} device={} err={}",
            reason,
            options.iface_address,
            options.device,
            err
        );
        return false;
    }
    true
}

fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

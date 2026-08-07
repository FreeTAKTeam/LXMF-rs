use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{lookup_host, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::error::RnsError;
use crate::iface::{IfaceSource, RxMessage};
use crate::packet::Packet;
use crate::serde::Serialize;

use alloc::string::String;

use super::hdlc::Hdlc;
use super::tcp_server::{FastFlapPolicy, FastFlapTracker};
use super::{Interface, InterfaceContext, InterfaceRxSender, InterfaceTxReceiver};

// TCP packet tracing is kept off by default and gated by diagnostics env flags.
const PACKET_TRACE: bool = false;
const HDLC_KEEPALIVE_FRAME: &[u8] = &[0x7e, 0x7e];

// A peer that accepts the TCP handshake and then resets/closes the
// connection almost immediately used to bypass the reconnect backoff
// entirely — `failed_connect_attempts` only ever incremented on an
// outright `connect()` failure, so a connection that succeeds and then
// dies moments later reset straight back to redialing with zero delay,
// forever. Any stream that doesn't survive at least this long after
// connecting is treated the same as a failed connect attempt for
// backoff/counting purposes. Long enough that a real, working
// connection's normal keepalive/traffic cadence never trips it; short
// enough to catch "connected then instantly reset" within one or two
// iterations rather than many.
const MIN_STABLE_CONNECTION: Duration = Duration::from_secs(2);
pub(crate) const HDLC_STREAM_EVENT_CHANNEL_CAPACITY: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TcpSocketTuning {
    pub nodelay: Option<bool>,
    pub keepalive: Option<bool>,
    pub tcp_keepalive_idle: Option<Duration>,
    pub tcp_keepalive_interval: Option<Duration>,
    pub tcp_keepalive_retries: Option<u32>,
    pub tcp_user_timeout: Option<Duration>,
}

impl TcpSocketTuning {
    #[must_use]
    pub fn backbone() -> Self {
        Self {
            nodelay: Some(true),
            keepalive: Some(true),
            tcp_keepalive_idle: Some(Duration::from_secs(5)),
            tcp_keepalive_interval: Some(Duration::from_secs(2)),
            tcp_keepalive_retries: Some(12),
            tcp_user_timeout: Some(Duration::from_secs(24)),
        }
    }

    #[must_use]
    pub fn i2p_tunneled() -> Self {
        Self {
            nodelay: Some(true),
            keepalive: Some(true),
            tcp_keepalive_idle: Some(Duration::from_secs(10)),
            tcp_keepalive_interval: Some(Duration::from_secs(9)),
            tcp_keepalive_retries: Some(5),
            tcp_user_timeout: Some(Duration::from_secs(45)),
        }
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.nodelay.is_none()
            && self.keepalive.is_none()
            && self.tcp_keepalive_idle.is_none()
            && self.tcp_keepalive_interval.is_none()
            && self.tcp_keepalive_retries.is_none()
            && self.tcp_user_timeout.is_none()
    }

    pub fn apply_to_stream(self, stream: &TcpStream) -> io::Result<()> {
        if let Some(nodelay) = self.nodelay {
            stream.set_nodelay(nodelay)?;
        }
        self.apply_platform_tuning(stream)?;
        Ok(())
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn apply_platform_tuning(self, stream: &TcpStream) -> io::Result<()> {
        let socket = socket2::SockRef::from(stream);
        let keepalive_params_configured = self.tcp_keepalive_idle.is_some()
            || self.tcp_keepalive_interval.is_some()
            || self.tcp_keepalive_retries.is_some();

        if keepalive_params_configured {
            let mut keepalive = socket2::TcpKeepalive::new();
            if let Some(idle) = self.tcp_keepalive_idle {
                keepalive = keepalive.with_time(idle);
            }
            if let Some(interval) = self.tcp_keepalive_interval {
                keepalive = keepalive.with_interval(interval);
            }
            if let Some(retries) = self.tcp_keepalive_retries {
                keepalive = keepalive.with_retries(retries);
            }
            socket.set_tcp_keepalive(&keepalive)?;
        } else if let Some(keepalive) = self.keepalive {
            socket.set_keepalive(keepalive)?;
        }

        if let Some(timeout) = self.tcp_user_timeout {
            socket.set_tcp_user_timeout(Some(timeout))?;
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn apply_platform_tuning(self, _stream: &TcpStream) -> io::Result<()> {
        let _ = self;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HdlcStreamEvent {
    Read { bytes: usize },
    Write { bytes: usize },
    Keepalive,
    Active,
    Stale,
    ReadTimeout,
    Closed,
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HdlcStreamWatchdog {
    pub keepalive_after: Duration,
    pub stale_after: Duration,
    pub read_timeout: Duration,
}

#[derive(Clone)]
pub(crate) struct HdlcStreamRuntime {
    pub watchdog: Option<HdlcStreamWatchdog>,
    pub events: Option<tokio::sync::mpsc::Sender<HdlcStreamEvent>>,
    pub forced_bitrate_bps: Option<u64>,
}

impl HdlcStreamRuntime {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self { watchdog: None, events: None, forced_bitrate_bps: None }
    }

    #[must_use]
    pub(crate) fn with_watchdog(mut self, watchdog: HdlcStreamWatchdog) -> Self {
        self.watchdog = Some(watchdog);
        self
    }

    #[must_use]
    pub(crate) fn with_events(
        mut self,
        events: tokio::sync::mpsc::Sender<HdlcStreamEvent>,
    ) -> Self {
        self.events = Some(events);
        self
    }

    #[must_use]
    pub(crate) fn with_forced_bitrate(mut self, bitrate_bps: u64) -> Self {
        self.forced_bitrate_bps = (bitrate_bps > 0).then_some(bitrate_bps);
        self
    }
}

impl Default for HdlcStreamRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpRuntimeStatus {
    pub endpoint: String,
    pub mtu: usize,
    pub stream_state: String,
    pub reconnect_attempts: u64,
    pub liveness_enabled: bool,
    pub forced_bitrate_bps: Option<u64>,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub keepalives_sent: u64,
    pub stale_events: u64,
    pub read_timeouts: u64,
    pub closed_events: u64,
    pub error_events: u64,
    pub last_error: Option<String>,
}

impl TcpRuntimeStatus {
    #[must_use]
    pub fn new(endpoint: String, mtu: usize) -> Self {
        Self {
            endpoint,
            mtu,
            stream_state: "configured".to_string(),
            reconnect_attempts: 0,
            liveness_enabled: false,
            forced_bitrate_bps: None,
            bytes_rx: 0,
            bytes_tx: 0,
            keepalives_sent: 0,
            stale_events: 0,
            read_timeouts: 0,
            closed_events: 0,
            error_events: 0,
            last_error: None,
        }
    }

    fn mark_connecting(&mut self) {
        self.stream_state = "connecting".to_string();
        self.last_error = None;
    }

    fn mark_connected(&mut self) {
        self.stream_state = "connected".to_string();
        self.last_error = None;
    }

    fn mark_reconnecting(&mut self, error: String) {
        self.stream_state = "reconnecting".to_string();
        self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
        self.last_error = Some(error);
    }

    fn mark_closed(&mut self) {
        self.stream_state = "closed".to_string();
    }

    fn apply_event(&mut self, event: HdlcStreamEvent) {
        match event {
            HdlcStreamEvent::Read { bytes } => {
                self.bytes_rx = self.bytes_rx.saturating_add(bytes as u64);
                if self.stream_state == "stale" {
                    self.stream_state = "connected".to_string();
                }
            }
            HdlcStreamEvent::Write { bytes } => {
                self.bytes_tx = self.bytes_tx.saturating_add(bytes as u64);
            }
            HdlcStreamEvent::Keepalive => {
                self.keepalives_sent = self.keepalives_sent.saturating_add(1);
            }
            HdlcStreamEvent::Active => {
                self.stream_state = "connected".to_string();
            }
            HdlcStreamEvent::Stale => {
                self.stream_state = "stale".to_string();
                self.stale_events = self.stale_events.saturating_add(1);
            }
            HdlcStreamEvent::ReadTimeout => {
                self.stream_state = "reconnecting".to_string();
                self.read_timeouts = self.read_timeouts.saturating_add(1);
                self.last_error = Some("tcp stream read timeout".to_string());
            }
            HdlcStreamEvent::Closed => {
                self.stream_state = "closed".to_string();
                self.closed_events = self.closed_events.saturating_add(1);
            }
            HdlcStreamEvent::Error { message } => {
                self.stream_state = "reconnecting".to_string();
                self.error_events = self.error_events.saturating_add(1);
                self.last_error = Some(message);
            }
        }
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        root.insert("endpoint".to_string(), serde_json::Value::String(self.endpoint.clone()));
        root.insert("mtu".to_string(), serde_json::Value::Number((self.mtu as u64).into()));
        root.insert(
            "stream_state".to_string(),
            serde_json::Value::String(self.stream_state.clone()),
        );
        root.insert(
            "reconnect_attempts".to_string(),
            serde_json::Value::Number(self.reconnect_attempts.into()),
        );
        root.insert("liveness_enabled".to_string(), serde_json::Value::Bool(self.liveness_enabled));
        root.insert(
            "forced_bitrate_bps".to_string(),
            self.forced_bitrate_bps
                .map(|value| serde_json::Value::Number(value.into()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert("bytes_rx".to_string(), serde_json::Value::Number(self.bytes_rx.into()));
        root.insert("bytes_tx".to_string(), serde_json::Value::Number(self.bytes_tx.into()));
        root.insert(
            "keepalives_sent".to_string(),
            serde_json::Value::Number(self.keepalives_sent.into()),
        );
        root.insert(
            "stale_events".to_string(),
            serde_json::Value::Number(self.stale_events.into()),
        );
        root.insert(
            "read_timeouts".to_string(),
            serde_json::Value::Number(self.read_timeouts.into()),
        );
        root.insert(
            "closed_events".to_string(),
            serde_json::Value::Number(self.closed_events.into()),
        );
        root.insert(
            "error_events".to_string(),
            serde_json::Value::Number(self.error_events.into()),
        );
        root.insert(
            "last_error".to_string(),
            self.last_error
                .as_ref()
                .map(|err| serde_json::Value::String(err.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        serde_json::Value::Object(root)
    }
}

#[derive(Clone)]
pub struct TcpRuntimeStatusHandle {
    inner: Arc<std::sync::Mutex<TcpRuntimeStatus>>,
}

impl TcpRuntimeStatusHandle {
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("tcp runtime status mutex poisoned").to_json()
    }
}

async fn track_tcp_stream_events(
    runtime_status: Arc<std::sync::Mutex<TcpRuntimeStatus>>,
    mut event_rx: tokio::sync::mpsc::Receiver<HdlcStreamEvent>,
) {
    while let Some(event) = event_rx.recv().await {
        runtime_status.lock().expect("tcp runtime status mutex poisoned").apply_event(event);
    }
}

fn tcp_wire_buffer_capacity(mtu: usize) -> usize {
    // Worst-case HDLC expansion doubles bytes (all escaped) plus frame delimiters.
    mtu.saturating_mul(2).saturating_add(16)
}

/// Maximum number of bytes the HDLC frame reassembly buffer may hold
/// before it is reset. A valid frame's wire length never exceeds the
/// worst-case HDLC escape expansion of a single MTU payload
/// (`tcp_wire_buffer_capacity`), so allowing twice that preserves valid
/// oversized-frame handling — including a frame split across multiple
/// reads — while bounding memory against a peer that streams garbage
/// that never closes a frame. The previous `mtu * 64` guard let a
/// malformed stream sustain ~16 MiB at the default MTU before tripping;
/// this caps it at ~1 MiB instead.
fn max_hdlc_frame_buffer_len(mtu: usize) -> usize {
    tcp_wire_buffer_capacity(mtu).saturating_mul(2)
}

/// Drops malformed backlog from the HDLC frame reassembly buffer once it
/// exceeds `limit`, returning whether anything was dropped. A trailing
/// partial frame is preserved when it can still decode: with no complete
/// frame found, the buffer holds at most one flag byte, so everything
/// before the last flag is guaranteed garbage, whereas the flag itself
/// may open a valid frame whose body arrives in later reads. A partial
/// frame already larger than the worst-case valid wire length can never
/// decode into the MTU-sized receive buffer and is dropped entirely, so
/// a flag followed by endless garbage still can't grow the buffer without
/// bound.
fn trim_malformed_hdlc_backlog(frame_buffer: &mut Vec<u8>, limit: usize, mtu: usize) -> bool {
    if frame_buffer.len() <= limit {
        return false;
    }
    let worst_case_frame = tcp_wire_buffer_capacity(mtu);
    match frame_buffer.iter().rposition(|&byte| byte == super::hdlc::HDLC_FRAME_FLAG) {
        Some(flag) if frame_buffer.len() - flag <= worst_case_frame => {
            frame_buffer.drain(..flag);
        }
        _ => frame_buffer.clear(),
    }
    true
}

fn forced_bitrate_delay(raw_len: usize, bitrate_bps: u64) -> Option<Duration> {
    if raw_len == 0 || bitrate_bps == 0 {
        return None;
    }
    let nanos =
        (raw_len as u128).saturating_mul(8).saturating_mul(1_000_000_000) / u128::from(bitrate_bps);
    let nanos = nanos.max(1).min(u128::from(u64::MAX)) as u64;
    Some(Duration::from_nanos(nanos))
}

pub(crate) fn backbone_hdlc_watchdog() -> HdlcStreamWatchdog {
    HdlcStreamWatchdog {
        keepalive_after: Duration::from_secs(10),
        stale_after: Duration::from_secs(20),
        read_timeout: Duration::from_secs(110),
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) async fn run_hdlc_stream<R, W>(
    label: String,
    iface_address: crate::hash::AddressHash,
    mtu: usize,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: InterfaceRxSender,
    tx_channel: Arc<tokio::sync::Mutex<InterfaceTxReceiver>>,
    read_stream: R,
    write_stream: W,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    run_hdlc_stream_with_runtime(
        label,
        iface_address,
        mtu,
        cancel,
        iface_stop,
        rx_channel,
        tx_channel,
        read_stream,
        write_stream,
        HdlcStreamRuntime::default(),
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_hdlc_stream_with_runtime<R, W>(
    label: String,
    iface_address: crate::hash::AddressHash,
    mtu: usize,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: InterfaceRxSender,
    tx_channel: Arc<tokio::sync::Mutex<InterfaceTxReceiver>>,
    read_stream: R,
    write_stream: W,
    runtime: HdlcStreamRuntime,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let stop = CancellationToken::new();
    let iface_stop_rx = iface_stop.clone();
    let iface_stop_tx = iface_stop.clone();
    let last_read_at = Arc::new(std::sync::Mutex::new(Instant::now()));
    let events = runtime.events.clone();

    let rx_task = {
        let cancel = cancel.clone();
        let stop = stop.clone();
        let mut stream = read_stream;
        let rx_channel = rx_channel.clone();
        let label = label.clone();
        let last_read_at = last_read_at.clone();
        let events = events.clone();

        tokio::spawn(async move {
            let mut hdlc_rx_buffer = vec![0u8; mtu];
            let mut frame_buffer: Vec<u8> = Vec::with_capacity(mtu.saturating_mul(4));
            let mut tcp_buffer = vec![0u8; mtu.saturating_mul(16)];

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                            break;
                    }
                    _ = iface_stop_rx.cancelled() => {
                            stop.cancel();
                            break;
                    }
                    _ = stop.cancelled() => {
                            break;
                    }
                    result = stream.read(&mut tcp_buffer[..]) => {
                            match result {
                                Ok(0) => {
                                    log::warn!("connection closed");
                                    send_hdlc_stream_event(&events, HdlcStreamEvent::Closed);
                                    stop.cancel();
                                    break;
                                }
                                Ok(n) => {
                                    *last_read_at
                                        .lock()
                                        .expect("tcp client last-read mutex poisoned") =
                                        Instant::now();
                                    send_hdlc_stream_event(&events, HdlcStreamEvent::Read { bytes: n });
                                    // TCP and Unix streams can deliver partial or multiple HDLC frames.
                                    frame_buffer.extend_from_slice(&tcp_buffer[..n]);

                                    while let Some((start, end)) = Hdlc::find(&frame_buffer) {
                                        let frame = &frame_buffer[start..=end];
                                        let mut output = OutputBuffer::new(&mut hdlc_rx_buffer[..]);
                                        if Hdlc::decode(frame, &mut output).is_ok() {
                                            if let Ok(packet) =
                                                Packet::deserialize(&mut InputBuffer::new(output.as_slice()))
                                            {
                                                if PACKET_TRACE {
                                                    log::trace!("rx << ({}) {}", iface_address, packet);
                                                }
                                                log::debug!(
                                                    "[tp-diag] {} rx_packet iface={} type={:?} dst={} ctx={:02x} hops={}",
                                                    label,
                                                    iface_address,
                                                    packet.header.packet_type,
                                                    packet.destination,
                                                    packet.context as u8,
                                                    packet.header.hops
                                                );
                                                if rx_channel
                                                    .send(RxMessage {
                                                        address: iface_address,
                                                        packet,
                                                        source: IfaceSource::None,
                                                    })
                                                    .await
                                                    .is_err()
                                                {
                                                    log::warn!(
                                                        "[tp-diag] transport receive queue closed iface={} label={}",
                                                        iface_address,
                                                        label
                                                    );
                                                    stop.cancel();
                                                    return;
                                                }
                                            } else {
                                                log::warn!("couldn't decode packet");
                                            }
                                        } else {
                                            log::warn!("couldn't decode hdlc frame");
                                        }

                                        // Drop all bytes up to and including the closing
                                        // flag of the frame we just handled.
                                        frame_buffer.drain(..=end);
                                    }

                                    let frame_buffer_limit = max_hdlc_frame_buffer_len(mtu);
                                    let buffered = frame_buffer.len();
                                    if trim_malformed_hdlc_backlog(
                                        &mut frame_buffer,
                                        frame_buffer_limit,
                                        mtu,
                                    ) {
                                        // Guard against unbounded growth on malformed
                                        // streams where no valid frame closes. Any
                                        // trailing partial frame that can still
                                        // decode is preserved — see
                                        // trim_malformed_hdlc_backlog.
                                        log::warn!(
                                            "[tp-diag] {} dropped malformed HDLC backlog iface={} buffered={} kept={} limit={}",
                                            label,
                                            iface_address,
                                            buffered,
                                            frame_buffer.len(),
                                            frame_buffer_limit
                                        );
                                    }
                                }
                                Err(e) => {
                                    log::warn!("connection error {}", e);
                                    send_hdlc_stream_event(
                                        &events,
                                        HdlcStreamEvent::Error { message: e.to_string() },
                                    );
                                    // A TCP reset (rather than a graceful FIN) lands
                                    // here, not in the `Ok(0)` arm above — which
                                    // does call `stop.cancel()`. Without it, the tx
                                    // task's `select!` has no branch left that will
                                    // ever fire when there's no watchdog configured
                                    // and nothing queued to send, so it parks on
                                    // `tx_channel.recv()` forever and this function's
                                    // `tx_task.await` (below) never returns — meaning
                                    // the MIN_STABLE_CONNECTION backoff check in
                                    // tcp_client's caller never gets reached for
                                    // exactly the reset case it was added for.
                                    stop.cancel();
                                    break;
                                }
                            }
                        },
                };
            }
        })
    };

    let tx_task = {
        let cancel = cancel.clone();
        let tx_channel = tx_channel.clone();
        let mut stream = write_stream;
        let label = label.clone();
        let last_read_at = last_read_at.clone();
        let events = events.clone();
        let watchdog = runtime.watchdog.clone();
        let forced_bitrate_bps = runtime.forced_bitrate_bps;

        tokio::spawn(async move {
            let mut last_write_at = Instant::now();
            let mut stale = false;
            let mut watchdog_tick = tokio::time::interval(Duration::from_secs(1));
            watchdog_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                if stop.is_cancelled() {
                    break;
                }

                let mut hdlc_tx_buffer = vec![0u8; tcp_wire_buffer_capacity(mtu)];
                let mut tx_buffer = vec![0u8; mtu];

                let mut tx_channel = tx_channel.lock().await;

                tokio::select! {
                    _ = cancel.cancelled() => {
                            break;
                    }
                    _ = iface_stop_tx.cancelled() => {
                            stop.cancel();
                            break;
                    }
                    _ = stop.cancelled() => {
                            break;
                    }
                    _ = watchdog_tick.tick(), if watchdog.is_some() => {
                        let watchdog = watchdog.as_ref().expect("guarded by select condition");
                        let now = Instant::now();
                        let last_read = *last_read_at
                            .lock()
                            .expect("tcp client last-read mutex poisoned");
                        let read_idle = now.saturating_duration_since(last_read);

                        if read_idle > watchdog.read_timeout {
                            log::warn!(
                                "[tp-diag] {} read watchdog timed out iface={} idle_ms={}",
                                label,
                                iface_address,
                                read_idle.as_millis()
                            );
                            send_hdlc_stream_event(&events, HdlcStreamEvent::ReadTimeout);
                            stop.cancel();
                            break;
                        }

                        if read_idle > watchdog.stale_after {
                            if !stale {
                                send_hdlc_stream_event(&events, HdlcStreamEvent::Stale);
                            }
                            stale = true;
                        } else if stale {
                            stale = false;
                            send_hdlc_stream_event(&events, HdlcStreamEvent::Active);
                        }

                        if now.saturating_duration_since(last_write_at) > watchdog.keepalive_after {
                            match stream.write_all(HDLC_KEEPALIVE_FRAME).await {
                                Ok(()) => {
                                    if let Err(err) = stream.flush().await {
                                        log::warn!("[tp-diag] keepalive flush failed iface={} err={}", iface_address, err);
                                        send_hdlc_stream_event(
                                            &events,
                                            HdlcStreamEvent::Error { message: err.to_string() },
                                        );
                                        stop.cancel();
                                        break;
                                    }
                                    last_write_at = Instant::now();
                                    send_hdlc_stream_event(&events, HdlcStreamEvent::Keepalive);
                                }
                                Err(err) => {
                                    log::warn!("[tp-diag] keepalive write failed iface={} err={}", iface_address, err);
                                    send_hdlc_stream_event(
                                        &events,
                                        HdlcStreamEvent::Error { message: err.to_string() },
                                    );
                                    stop.cancel();
                                    break;
                                }
                            }
                        }
                    }
                    Some(message) = tx_channel.recv() => {
                        let packet = message.packet;
                        if PACKET_TRACE {
                            log::trace!("tx >> ({}) {}", iface_address, packet);
                        }
                        log::debug!("[tp-diag] {} tx_dequeue iface={} {}", label, iface_address, packet);
                        let mut output = OutputBuffer::new(&mut tx_buffer);
                        if packet.serialize(&mut output).is_ok() {
                            if let Some(bitrate_bps) = forced_bitrate_bps {
                                if let Some(delay) = forced_bitrate_delay(output.as_slice().len(), bitrate_bps) {
                                    tokio::select! {
                                        _ = cancel.cancelled() => break,
                                        _ = iface_stop_tx.cancelled() => {
                                            stop.cancel();
                                            break;
                                        }
                                        _ = stop.cancelled() => break,
                                        _ = tokio::time::sleep(delay) => {}
                                    }
                                }
                            }
                            let mut hdlc_output = OutputBuffer::new(&mut hdlc_tx_buffer[..]);
                            if Hdlc::encode(output.as_slice(), &mut hdlc_output).is_ok() {
                                if let Err(err) = stream.write_all(hdlc_output.as_slice()).await {
                                    log::warn!("[tp-diag] write_all failed iface={} err={}", iface_address, err);
                                    send_hdlc_stream_event(
                                        &events,
                                        HdlcStreamEvent::Error { message: err.to_string() },
                                    );
                                    stop.cancel();
                                    break;
                                }
                                if let Err(err) = stream.flush().await {
                                    log::warn!("[tp-diag] flush failed iface={} err={}", iface_address, err);
                                    send_hdlc_stream_event(
                                        &events,
                                        HdlcStreamEvent::Error { message: err.to_string() },
                                    );
                                    stop.cancel();
                                    break;
                                }
                                last_write_at = Instant::now();
                                send_hdlc_stream_event(
                                    &events,
                                    HdlcStreamEvent::Write { bytes: hdlc_output.as_slice().len() },
                                );
                                log::debug!(
                                    "[tp-diag] {} tx_write_ok iface={} wire_len={} raw_len={}",
                                    label,
                                    iface_address,
                                    hdlc_output.as_slice().len(),
                                    output.as_slice().len()
                                );
                            } else {
                                log::warn!(
                                    "[tp-diag] hdlc_encode failed iface={} raw_len={}",
                                    iface_address,
                                    output.as_slice().len()
                                );
                            }
                        } else {
                            log::warn!(
                                "[tp-diag] serialize failed iface={} buffer_cap={}",
                                iface_address,
                                tx_buffer.len()
                            );
                        }
                    }
                };
            }
        })
    };

    tx_task.await.unwrap();
    rx_task.await.unwrap();
}

fn send_hdlc_stream_event(
    events: &Option<tokio::sync::mpsc::Sender<HdlcStreamEvent>>,
    event: HdlcStreamEvent,
) {
    if let Some(events) = events {
        if let Err(err) = events.try_send(event) {
            log::debug!("dropped HDLC stream event: {err}");
        }
    }
}

pub struct TcpClient {
    addr: String,
    stream: Option<TcpStream>,
    mtu: usize,
    socket_tuning: TcpSocketTuning,
    hdlc_watchdog: Option<HdlcStreamWatchdog>,
    forced_bitrate_bps: Option<u64>,
    reconnect_events: Option<tokio::sync::mpsc::Sender<crate::hash::AddressHash>>,
    connect_timeout: Duration,
    max_reconnect_tries: Option<u64>,
    prefer_ipv6: bool,
    runtime_status: Arc<std::sync::Mutex<TcpRuntimeStatus>>,
    fast_flap_tracker: Option<FastFlapTracker>,
    fast_flap_policy: FastFlapPolicy,
    peer_ip: Option<String>,
}

impl TcpClient {
    pub const DEFAULT_MTU: usize = 262_144;
    pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

    pub fn new<T: Into<String>>(addr: T) -> Self {
        let addr = addr.into();
        Self {
            runtime_status: Arc::new(std::sync::Mutex::new(TcpRuntimeStatus::new(
                addr.clone(),
                Self::DEFAULT_MTU,
            ))),
            addr,
            stream: None,
            mtu: Self::DEFAULT_MTU,
            socket_tuning: TcpSocketTuning::default(),
            hdlc_watchdog: None,
            forced_bitrate_bps: None,
            reconnect_events: None,
            connect_timeout: Self::DEFAULT_CONNECT_TIMEOUT,
            max_reconnect_tries: None,
            prefer_ipv6: false,
            fast_flap_tracker: None,
            fast_flap_policy: FastFlapPolicy::default(),
            peer_ip: None,
        }
    }

    pub fn new_from_stream<T: Into<String>>(addr: T, stream: TcpStream) -> Self {
        let addr = addr.into();
        Self {
            runtime_status: Arc::new(std::sync::Mutex::new(TcpRuntimeStatus::new(
                addr.clone(),
                Self::DEFAULT_MTU,
            ))),
            addr,
            stream: Some(stream),
            mtu: Self::DEFAULT_MTU,
            socket_tuning: TcpSocketTuning::default(),
            hdlc_watchdog: None,
            forced_bitrate_bps: None,
            reconnect_events: None,
            connect_timeout: Self::DEFAULT_CONNECT_TIMEOUT,
            max_reconnect_tries: None,
            prefer_ipv6: false,
            fast_flap_tracker: None,
            fast_flap_policy: FastFlapPolicy::default(),
            peer_ip: None,
        }
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(256);
        self.runtime_status.lock().expect("tcp runtime status mutex poisoned").mtu = self.mtu;
        self
    }

    #[must_use]
    pub fn with_socket_tuning(mut self, socket_tuning: TcpSocketTuning) -> Self {
        self.socket_tuning = socket_tuning;
        self
    }

    #[must_use]
    pub fn with_backbone_liveness(self) -> Self {
        self.with_hdlc_watchdog(backbone_hdlc_watchdog())
    }

    #[must_use]
    pub(crate) fn with_hdlc_watchdog(mut self, watchdog: HdlcStreamWatchdog) -> Self {
        self.hdlc_watchdog = Some(watchdog);
        self.runtime_status.lock().expect("tcp runtime status mutex poisoned").liveness_enabled =
            true;
        self
    }

    #[must_use]
    pub fn with_forced_bitrate(mut self, bitrate_bps: u64) -> Self {
        self.forced_bitrate_bps = (bitrate_bps > 0).then_some(bitrate_bps);
        self.runtime_status.lock().expect("tcp runtime status mutex poisoned").forced_bitrate_bps =
            self.forced_bitrate_bps;
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

    #[must_use]
    pub fn with_connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.connect_timeout = connect_timeout.max(Duration::from_millis(1));
        self
    }

    #[must_use]
    pub fn with_max_reconnect_tries(mut self, max_reconnect_tries: Option<u64>) -> Self {
        self.max_reconnect_tries = max_reconnect_tries;
        self
    }

    #[must_use]
    pub fn with_prefer_ipv6(mut self, prefer_ipv6: bool) -> Self {
        self.prefer_ipv6 = prefer_ipv6;
        self
    }

    pub(crate) fn with_fast_flap_tracking(
        mut self,
        tracker: FastFlapTracker,
        policy: FastFlapPolicy,
        peer_ip: Option<String>,
    ) -> Self {
        self.fast_flap_tracker = Some(tracker);
        self.fast_flap_policy = policy;
        self.peer_ip = peer_ip;
        self
    }

    #[must_use]
    pub fn addr(&self) -> &str {
        &self.addr
    }

    #[must_use]
    pub fn mtu_value(&self) -> usize {
        self.mtu
    }

    #[must_use]
    pub fn socket_tuning(&self) -> TcpSocketTuning {
        self.socket_tuning
    }

    #[must_use]
    pub fn hdlc_liveness_enabled(&self) -> bool {
        self.hdlc_watchdog.is_some()
    }

    #[must_use]
    pub fn forced_bitrate_bps(&self) -> Option<u64> {
        self.forced_bitrate_bps
    }

    #[must_use]
    pub fn reconnect_events_enabled(&self) -> bool {
        self.reconnect_events.is_some()
    }

    #[must_use]
    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    #[must_use]
    pub fn max_reconnect_tries(&self) -> Option<u64> {
        self.max_reconnect_tries
    }

    #[must_use]
    pub fn prefer_ipv6(&self) -> bool {
        self.prefer_ipv6
    }

    #[must_use]
    pub fn runtime_status_json(&self) -> serde_json::Value {
        self.runtime_status.lock().expect("tcp runtime status mutex poisoned").to_json()
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> TcpRuntimeStatusHandle {
        TcpRuntimeStatusHandle { inner: self.runtime_status.clone() }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn hdlc_watchdog(&self) -> Option<HdlcStreamWatchdog> {
        self.hdlc_watchdog.clone()
    }

    #[tracing::instrument(name = "tcp_peer", skip_all, fields(addr = tracing::field::Empty))]
    pub async fn spawn(context: InterfaceContext<TcpClient>) {
        let iface_stop = context.channel.stop.clone();
        let (
            addr,
            mtu,
            socket_tuning,
            hdlc_watchdog,
            forced_bitrate_bps,
            connect_timeout,
            max_reconnect_tries,
            prefer_ipv6,
            fast_flap_tracker,
            fast_flap_policy,
            peer_ip,
            runtime_status,
        ) = {
            let guard = context.inner.lock().unwrap();
            (
                guard.addr.clone(),
                guard.mtu,
                guard.socket_tuning,
                guard.hdlc_watchdog.clone(),
                guard.forced_bitrate_bps,
                guard.connect_timeout,
                guard.max_reconnect_tries,
                guard.prefer_ipv6,
                guard.fast_flap_tracker.clone(),
                guard.fast_flap_policy,
                guard.peer_ip.clone(),
                guard.runtime_status.clone(),
            )
        };
        tracing::Span::current().record("addr", addr.as_str());
        let iface_address = context.channel.address;
        let (mut stream, reconnect_events) = {
            let mut guard = context.inner.lock().unwrap();
            (guard.stream.take(), guard.reconnect_events.clone())
        };

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        let mut running = true;
        let mut has_connected = false;
        let mut failed_connect_attempts = 0_u64;
        loop {
            if !running || context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            let stream = {
                match stream.take() {
                    Some(stream) => {
                        running = false;
                        Ok(stream)
                    }
                    None => {
                        runtime_status
                            .lock()
                            .expect("tcp runtime status mutex poisoned")
                            .mark_connecting();
                        tokio::time::timeout(
                            connect_timeout,
                            connect_tcp_stream(addr.clone(), prefer_ipv6),
                        )
                        .await
                        .map_err(|_| RnsError::ConnectionError)
                        .and_then(|result| result.map_err(|_| RnsError::ConnectionError))
                    }
                }
            };

            if stream.is_err() {
                failed_connect_attempts = failed_connect_attempts.saturating_add(1);
                runtime_status
                    .lock()
                    .expect("tcp runtime status mutex poisoned")
                    .mark_reconnecting(format!("tcp connect failed endpoint={addr}"));
                log::warn!("couldn't connect to <{}>", addr);
                if max_reconnect_tries.is_some_and(|max_reconnect_tries| {
                    failed_connect_attempts > max_reconnect_tries
                }) {
                    log::error!(
                        "max TCP reconnect attempts reached for <{}> after {} failed attempts",
                        addr,
                        failed_connect_attempts
                    );
                    break;
                }
                tokio::select! {
                    _ = context.cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                }
                continue;
            }

            let stream = stream.unwrap();
            runtime_status.lock().expect("tcp runtime status mutex poisoned").mark_connected();
            if let Err(err) = socket_tuning.apply_to_stream(&stream) {
                log::warn!("failed to apply TCP socket tuning to <{}>: {}", addr, err);
            }
            let (read_stream, write_stream) = stream.into_split();
            // `failed_connect_attempts` is NOT reset here anymore; it's
            // only reset below once the stream has actually proven itself
            // stable — see `MIN_STABLE_CONNECTION`'s doc comment.
            let connected_at = Instant::now();

            log::info!("connected to <{}>", addr);
            if has_connected {
                if let Some(events) = reconnect_events.as_ref() {
                    if let Err(err) = events.try_send(iface_address) {
                        log::debug!(
                            "dropped TCP reconnect event iface={} endpoint={} err={}",
                            iface_address,
                            addr,
                            err
                        );
                    }
                }
            } else {
                has_connected = true;
            }

            let (event_tx, event_rx) =
                tokio::sync::mpsc::channel(HDLC_STREAM_EVENT_CHANNEL_CAPACITY);
            let status_task =
                tokio::spawn(track_tcp_stream_events(runtime_status.clone(), event_rx));
            let runtime =
                hdlc_watchdog.clone().map_or_else(HdlcStreamRuntime::default, |watchdog| {
                    HdlcStreamRuntime::new().with_watchdog(watchdog)
                });
            let runtime = if let Some(bitrate_bps) = forced_bitrate_bps {
                runtime.with_forced_bitrate(bitrate_bps)
            } else {
                runtime
            }
            .with_events(event_tx);

            run_hdlc_stream_with_runtime(
                "tcp_client".to_string(),
                iface_address,
                mtu,
                context.cancel.clone(),
                iface_stop.clone(),
                rx_channel.clone(),
                tx_channel.clone(),
                read_stream,
                write_stream,
                runtime,
            )
            .await;
            if let Err(error) = status_task.await {
                log::error!(
                    "tcp client status task failed iface={} peer={} error={error}",
                    iface_address,
                    addr
                );
            }

            log::info!("disconnected from <{}>", addr);

            // A deliberate shutdown requested during those first couple of
            // seconds (context.cancel/iface_stop already cancelled) is not
            // evidence of an unstable peer — it's the reason
            // run_hdlc_stream_with_runtime just returned in the first
            // place. Falling through to the loop's own top-of-iteration
            // check breaks cleanly without polluting failed_connect_attempts
            // or runtime status with a spurious "treating as a failed
            // attempt" — a hot-reload or daemon shutdown that lands inside
            // MIN_STABLE_CONNECTION shouldn't look like a flaky connection.
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            // A stream that died moments after connecting is treated as a
            // failed attempt (backoff + counts toward `max_reconnect_tries`),
            // same as an outright `connect()` failure just above — otherwise
            // a peer that accepts the handshake and then instantly resets
            // gets hammered with an unthrottled redial loop forever.
            let stayed_connected_for = connected_at.elapsed();
            if let (Some(tracker), Some(peer_ip)) = (fast_flap_tracker.as_ref(), peer_ip.as_deref())
            {
                tracker.record_short_connection(peer_ip, stayed_connected_for, fast_flap_policy);
            }
            if stayed_connected_for < MIN_STABLE_CONNECTION {
                failed_connect_attempts = failed_connect_attempts.saturating_add(1);
                runtime_status.lock().expect("tcp runtime status mutex poisoned").mark_reconnecting(format!(
                    "connection to <{addr}> dropped after only {stayed_connected_for:?} — treating as a failed attempt"
                ));
                if max_reconnect_tries.is_some_and(|max_reconnect_tries| {
                    failed_connect_attempts > max_reconnect_tries
                }) {
                    log::error!(
                        "max TCP reconnect attempts reached for <{}> after {} failed attempts",
                        addr,
                        failed_connect_attempts
                    );
                    break;
                }
                // The first short-lived disconnect in a streak retries
                // immediately — a single clean drop-then-reconnect (a wifi
                // blip, the peer restarting once) shouldn't eat a 5s
                // penalty. Only the SECOND consecutive one onward backs
                // off — that's what distinguishes an actual storm (a
                // misbehaving peer that never stabilizes) from a normal
                // one-off reconnect.
                if failed_connect_attempts > 1 {
                    tokio::select! {
                        _ = context.cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                    }
                }
            } else {
                failed_connect_attempts = 0;
            }
        }

        runtime_status.lock().expect("tcp runtime status mutex poisoned").mark_closed();
        iface_stop.cancel();
    }
}

pub(crate) fn prefer_ipv6_socket_addrs(
    addrs: impl IntoIterator<Item = SocketAddr>,
    prefer_ipv6: bool,
) -> Vec<SocketAddr> {
    let mut addrs = addrs.into_iter().collect::<Vec<_>>();
    if prefer_ipv6 {
        addrs.sort_by_key(|addr| if addr.is_ipv6() { 0 } else { 1 });
    }
    addrs
}

async fn connect_tcp_stream(addr: String, prefer_ipv6: bool) -> io::Result<TcpStream> {
    if !prefer_ipv6 {
        return TcpStream::connect(addr).await;
    }

    let addrs = prefer_ipv6_socket_addrs(lookup_host(addr.as_str()).await?, true);
    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect(addr).await {
            Ok(stream) => return Ok(stream),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "TCP endpoint resolved to no addresses")
    }))
}

impl Interface for TcpClient {
    fn mtu() -> usize {
        TcpClient::DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::{
        forced_bitrate_delay, max_hdlc_frame_buffer_len, prefer_ipv6_socket_addrs,
        run_hdlc_stream_with_runtime, tcp_wire_buffer_capacity, trim_malformed_hdlc_backlog,
        HdlcStreamEvent, HdlcStreamRuntime, HdlcStreamWatchdog, TcpClient, TcpSocketTuning,
        HDLC_STREAM_EVENT_CHANNEL_CAPACITY,
    };
    use crate::buffer::OutputBuffer;
    use crate::hash::AddressHash;
    use crate::iface::hdlc::Hdlc;
    use crate::iface::{InterfaceManager, TxMessage, TxMessageType};
    use crate::packet::Packet;
    use crate::serde::Serialize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_util::sync::CancellationToken;

    #[test]
    fn tcp_client_default_and_configured_mtu_are_exposed() {
        assert_eq!(TcpClient::new("rmap.world:4242").mtu_value(), TcpClient::DEFAULT_MTU);
        assert_eq!(TcpClient::DEFAULT_MTU, 262_144);
        assert_eq!(TcpClient::new("rmap.world:4242").with_mtu(4096).mtu_value(), 4096);
        assert_eq!(TcpClient::new("rmap.world:4242").with_mtu(64).mtu_value(), 256);
    }

    #[test]
    fn tcp_client_default_liveness_is_disabled() {
        let client = TcpClient::new("rmap.world:4242");

        assert!(!client.hdlc_liveness_enabled());
        assert_eq!(client.hdlc_watchdog(), None);
        assert_eq!(client.forced_bitrate_bps(), None);
    }

    #[test]
    fn tcp_client_exposes_forced_bitrate() {
        let client = TcpClient::new("rmap.world:4242").with_forced_bitrate(9_600);

        assert_eq!(client.forced_bitrate_bps(), Some(9_600));
        assert_eq!(
            TcpClient::new("rmap.world:4242").with_forced_bitrate(0).forced_bitrate_bps(),
            None
        );
    }

    #[test]
    fn forced_bitrate_delay_matches_python_formula() {
        assert_eq!(forced_bitrate_delay(0, 1_000), None);
        assert_eq!(forced_bitrate_delay(128, 0), None);
        assert_eq!(forced_bitrate_delay(125, 1_000), Some(Duration::from_secs(1)));
    }

    #[test]
    fn tcp_client_exposes_reticulum_reconnect_options() {
        let client = TcpClient::new("rmap.world:4242")
            .with_connect_timeout(Duration::from_secs(7))
            .with_max_reconnect_tries(Some(3))
            .with_prefer_ipv6(true);

        assert_eq!(client.connect_timeout(), Duration::from_secs(7));
        assert_eq!(client.max_reconnect_tries(), Some(3));
        assert!(client.prefer_ipv6());
    }

    #[test]
    fn tcp_client_prefers_ipv6_socket_addrs_when_requested() {
        let v4: std::net::SocketAddr = "127.0.0.1:4242".parse().expect("v4 addr");
        let v6: std::net::SocketAddr = "[::1]:4242".parse().expect("v6 addr");

        assert_eq!(prefer_ipv6_socket_addrs([v4, v6], false), vec![v4, v6]);
        assert_eq!(prefer_ipv6_socket_addrs([v4, v6], true), vec![v6, v4]);
    }

    #[tokio::test]
    async fn tcp_client_stops_after_reconnect_budget_is_exhausted() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        drop(listener);

        let mut manager = InterfaceManager::new(8);
        let client = TcpClient::new(addr.to_string())
            .with_connect_timeout(Duration::from_millis(50))
            .with_max_reconnect_tries(Some(0));
        let runtime_status = client.runtime_status_handle();
        let context = manager.new_context(client);
        let iface_stop = context.channel.stop.clone();
        let task = tokio::spawn(TcpClient::spawn(context));

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("tcp client should stop after reconnect budget")
            .expect("tcp client task should not panic");
        assert!(iface_stop.is_cancelled());
        let status = runtime_status.to_json();
        assert_eq!(status["endpoint"].as_str(), Some(addr.to_string().as_str()));
        assert_eq!(status["stream_state"].as_str(), Some("closed"));
        assert_eq!(status["reconnect_attempts"].as_u64(), Some(1));
        assert_eq!(
            status["last_error"].as_str(),
            Some(format!("tcp connect failed endpoint={addr}").as_str())
        );
    }

    // Regression test for `MIN_STABLE_CONNECTION`'s fix: a peer that
    // completes the TCP handshake and then immediately closes the
    // connection (rather than refusing it outright) used to bypass the
    // reconnect backoff entirely, redialing with zero delay forever. This
    // accepts every connection and drops it right away — simulating
    // exactly that misbehaving-peer case — and asserts the failed-attempt
    // counter (and therefore `max_reconnect_tries`) still applies.
    #[tokio::test]
    async fn tcp_client_backs_off_when_connection_dies_immediately_after_connecting() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        });

        let mut manager = InterfaceManager::new(8);
        let client = TcpClient::new(addr.to_string())
            .with_connect_timeout(Duration::from_millis(200))
            .with_max_reconnect_tries(Some(1));
        let runtime_status = client.runtime_status_handle();
        let context = manager.new_context(client);
        let iface_stop = context.channel.stop.clone();
        let task = tokio::spawn(TcpClient::spawn(context));

        // The first short-lived disconnect retries immediately (no
        // penalty for a one-off); `max_reconnect_tries=1` trips right on
        // the second, before any backoff sleep would even apply — see
        // `MIN_STABLE_CONNECTION`'s fix.
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("tcp client should stop after reconnect budget")
            .expect("tcp client task should not panic");
        assert!(iface_stop.is_cancelled());
        let status = runtime_status.to_json();
        assert_eq!(status["stream_state"].as_str(), Some("closed"));
        // Two failed attempts before `max_reconnect_tries=1` trips (count
        // > max) — proves each connect-then-instant-close cycle
        // incremented the same counter an outright connect() failure
        // would have.
        assert_eq!(status["reconnect_attempts"].as_u64(), Some(2));
    }

    // Companion to the test above: proves the backoff *sleep* itself
    // actually engages once a storm is underway (not just that the
    // counter increments) — the real-world bug this fixes was an
    // unthrottled, effectively instant redial loop against a misbehaving
    // hub.
    #[tokio::test]
    async fn tcp_client_actually_sleeps_once_a_reconnect_storm_is_underway() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        });

        let mut manager = InterfaceManager::new(8);
        let client = TcpClient::new(addr.to_string())
            .with_connect_timeout(Duration::from_millis(200))
            .with_max_reconnect_tries(Some(2));
        let runtime_status = client.runtime_status_handle();
        let context = manager.new_context(client);
        let iface_stop = context.channel.stop.clone();
        let started_at = Instant::now();
        let task = tokio::spawn(TcpClient::spawn(context));

        // Attempt 1: immediate retry (no penalty). Attempt 2: the storm is
        // now established, so a 5s backoff sleep happens before attempt 3,
        // which trips `max_reconnect_tries=2` (count 3 > 2) and stops.
        tokio::time::timeout(Duration::from_secs(8), task)
            .await
            .expect("tcp client should stop after reconnect budget")
            .expect("tcp client task should not panic");
        assert!(iface_stop.is_cancelled());
        assert_eq!(runtime_status.to_json()["reconnect_attempts"].as_u64(), Some(3));
        assert!(
            started_at.elapsed() >= Duration::from_secs(5),
            "expected at least one 5s backoff sleep once the storm was established, only took {:?}",
            started_at.elapsed()
        );
    }

    // Regression test for review feedback on the MIN_STABLE_CONNECTION fix
    // above (PR #494): a deliberate shutdown request (iface_stop/
    // context.cancel already cancelled) that lands inside the
    // MIN_STABLE_CONNECTION window is not evidence of an unstable peer —
    // it's the reason the stream loop just returned. Without the fix, a
    // hot-reload or daemon shutdown shortly after connecting would show up
    // as a spurious "reconnecting"/failed-attempt status right before
    // going to "closed".
    #[tokio::test]
    async fn tcp_client_does_not_count_a_deliberate_shutdown_as_a_failed_attempt() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");

        // Accept and hold the connection open for the task's lifetime —
        // the disconnect in this test comes from cancelling iface_stop
        // below, never from the peer closing its end.
        let _accept_task = tokio::spawn(async move {
            let mut streams = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                streams.push(stream);
            }
        });

        let mut manager = InterfaceManager::new(8);
        let client =
            TcpClient::new(addr.to_string()).with_connect_timeout(Duration::from_millis(200));
        let runtime_status = client.runtime_status_handle();
        let context = manager.new_context(client);
        let iface_stop = context.channel.stop.clone();
        let task = tokio::spawn(TcpClient::spawn(context));

        // Well inside MIN_STABLE_CONNECTION, and long enough for the
        // connection to have actually been established.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(runtime_status.to_json()["stream_state"].as_str(), Some("connected"));

        iface_stop.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("tcp client should stop promptly once iface_stop is cancelled")
            .expect("tcp client task should not panic");

        let status = runtime_status.to_json();
        assert_eq!(status["stream_state"].as_str(), Some("closed"));
        assert_eq!(status["reconnect_attempts"].as_u64(), Some(0));
    }

    #[test]
    fn tcp_client_backbone_liveness_uses_watchdog_profile() {
        let client = TcpClient::new("rmap.world:4242").with_backbone_liveness();

        assert!(client.hdlc_liveness_enabled());
        assert_eq!(client.hdlc_watchdog(), Some(super::backbone_hdlc_watchdog()));
        assert_eq!(client.runtime_status_json()["liveness_enabled"].as_bool(), Some(true));
    }

    #[tokio::test]
    async fn tcp_socket_tuning_applies_nodelay_to_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let client = TcpStream::connect(addr).await.expect("connect client");
        let (_server, _) = listener.accept().await.expect("accept server");

        let tuning = TcpSocketTuning::backbone();
        assert_eq!(tuning.nodelay, Some(true));
        assert_eq!(tuning.keepalive, Some(true));
        assert_eq!(tuning.tcp_keepalive_idle, Some(Duration::from_secs(5)));
        assert_eq!(tuning.tcp_keepalive_interval, Some(Duration::from_secs(2)));
        assert_eq!(tuning.tcp_keepalive_retries, Some(12));
        assert_eq!(tuning.tcp_user_timeout, Some(Duration::from_secs(24)));

        assert!(!client.nodelay().expect("read default nodelay"));
        tuning.apply_to_stream(&client).expect("apply tuning");
        assert!(client.nodelay().expect("read tuned nodelay"));

        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let socket = socket2::SockRef::from(&client);
            assert!(socket.keepalive().expect("read keepalive"));
            assert_eq!(
                socket.keepalive_time().expect("read keepalive idle"),
                Duration::from_secs(5)
            );
            assert_eq!(
                socket.keepalive_interval().expect("read keepalive interval"),
                Duration::from_secs(2)
            );
            assert_eq!(socket.keepalive_retries().expect("read keepalive retries"), 12);
            assert_eq!(
                socket.tcp_user_timeout().expect("read user timeout"),
                Some(Duration::from_secs(24))
            );
        }
    }

    #[test]
    fn tcp_socket_tuning_i2p_tunneled_matches_reticulum_profile() {
        let tuning = TcpSocketTuning::i2p_tunneled();

        assert_eq!(tuning.nodelay, Some(true));
        assert_eq!(tuning.keepalive, Some(true));
        assert_eq!(tuning.tcp_keepalive_idle, Some(Duration::from_secs(10)));
        assert_eq!(tuning.tcp_keepalive_interval, Some(Duration::from_secs(9)));
        assert_eq!(tuning.tcp_keepalive_retries, Some(5));
        assert_eq!(tuning.tcp_user_timeout, Some(Duration::from_secs(45)));
    }

    #[test]
    fn tcp_wire_capacity_handles_worst_case_hdlc_escape_expansion() {
        let mtu = 512;
        let raw = vec![0x7e_u8; mtu];
        let mut wire = vec![0_u8; tcp_wire_buffer_capacity(mtu)];
        let mut output = OutputBuffer::new(&mut wire[..]);

        let encoded_len = Hdlc::encode(&raw, &mut output).expect("encode worst-case payload");
        assert!(encoded_len >= (mtu * 2) + 2, "wire len must cover escaped payload plus flags");
    }

    #[test]
    fn hdlc_frame_buffer_limit_is_tight_but_preserves_valid_oversized_frames() {
        // A valid frame's wire length never exceeds the worst-case escape
        // expansion of one MTU payload; the limit must admit a full such
        // frame (even split across reads) with headroom to spare.
        for mtu in [256, 512, 4096, TcpClient::DEFAULT_MTU] {
            let limit = max_hdlc_frame_buffer_len(mtu);
            let worst_case_valid_frame = mtu * 2 + 2;
            assert!(
                limit >= worst_case_valid_frame * 2,
                "limit {limit} must preserve oversized frames for mtu {mtu}"
            );
            // ... while staying far tighter than the old `mtu * 64` guard
            // that allowed ~16 MiB of malformed backlog at the default MTU.
            assert!(
                limit <= mtu * 8,
                "limit {limit} regressed toward the old loose guard for mtu {mtu}"
            );
        }
        // Concrete bound for the default-MTU case from issue #512.
        assert!(max_hdlc_frame_buffer_len(TcpClient::DEFAULT_MTU) <= 2 * 1024 * 1024);
    }

    // Regression for PR #528 review: clearing the malformed backlog must
    // not discard the opening flag and partial body of a valid frame
    // whose remainder arrives in later TCP reads.
    #[test]
    fn trim_malformed_backlog_preserves_decodable_partial_frame() {
        let mtu = 256;
        let limit = max_hdlc_frame_buffer_len(mtu);
        let mut buffer = vec![0x55_u8; limit + 10];
        // Garbage prefix followed by a partial frame: opening flag plus a
        // few payload bytes, no closing flag yet.
        let partial_start = buffer.len() - 4;
        buffer[partial_start] = 0x7e;

        assert!(trim_malformed_hdlc_backlog(&mut buffer, limit, mtu));
        assert_eq!(buffer.len(), 4, "only the partial frame suffix survives");
        assert_eq!(buffer[0], 0x7e, "the opening flag is preserved");
    }

    #[test]
    fn trim_malformed_backlog_drops_undecodable_partial_frame() {
        let mtu = 256;
        let limit = max_hdlc_frame_buffer_len(mtu);
        // A "partial frame" (flag at the start) already larger than the
        // worst-case valid wire length can never decode — keeping it
        // would let flag-prefixed garbage grow the buffer without bound.
        let mut buffer = vec![0x55_u8; limit + 10];
        buffer[0] = 0x7e;

        assert!(trim_malformed_hdlc_backlog(&mut buffer, limit, mtu));
        assert!(buffer.is_empty(), "oversized partial frame must be dropped entirely");
    }

    #[test]
    fn trim_malformed_backlog_clears_flagless_garbage_and_ignores_small_buffers() {
        let mtu = 256;
        let limit = max_hdlc_frame_buffer_len(mtu);

        let mut garbage = vec![0x55_u8; limit + 1];
        assert!(trim_malformed_hdlc_backlog(&mut garbage, limit, mtu));
        assert!(garbage.is_empty());

        let mut small = vec![0x55_u8; limit];
        assert!(!trim_malformed_hdlc_backlog(&mut small, limit, mtu));
        assert_eq!(small.len(), limit, "buffers at or under the limit are untouched");
    }

    // Regression test for issue #512: a peer that streams persistent
    // garbage with no HDLC frame flags must not grow the reassembly
    // buffer without bound, and the stream must recover sync once a
    // valid frame finally arrives.
    #[tokio::test]
    async fn hdlc_stream_recovers_sync_after_sustained_malformed_stream() {
        let mtu = 256;
        let (stream, mut peer) = tokio::io::duplex(128 * 1024);
        let (read_stream, write_stream) = tokio::io::split(stream);
        let cancel = CancellationToken::new();
        let iface_stop = CancellationToken::new();
        let iface_address = AddressHash::new([0x48; 16]);
        let (rx_channel, mut rx_messages) = tokio::sync::mpsc::channel(4);
        let (_tx_sender, tx_receiver) = tokio::sync::mpsc::channel(1);
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::channel(HDLC_STREAM_EVENT_CHANNEL_CAPACITY);

        let handle = tokio::spawn(run_hdlc_stream_with_runtime(
            "test".to_string(),
            iface_address,
            mtu,
            cancel.clone(),
            iface_stop,
            rx_channel,
            Arc::new(tokio::sync::Mutex::new(tx_receiver)),
            read_stream,
            write_stream,
            HdlcStreamRuntime::new().with_events(event_tx),
        ));

        // Persistent malformed stream: three times the frame-buffer limit
        // of garbage that contains no HDLC flag byte, so no frame can
        // ever close and the guard must reset the buffer repeatedly.
        let garbage = vec![0x55_u8; max_hdlc_frame_buffer_len(mtu) * 3];
        peer.write_all(&garbage).await.expect("write garbage");

        // Wait until the rx task has consumed all of the garbage (i.e.
        // the guard has tripped and cleared the buffer at least once)
        // before sending a valid frame, so the frame can't be caught in
        // a guard reset alongside the garbage.
        let mut consumed = 0_usize;
        while consumed < garbage.len() {
            let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
                .await
                .expect("read event deadline")
                .expect("read event");
            if let HdlcStreamEvent::Read { bytes } = event {
                consumed += bytes;
            }
        }

        // A valid frame arriving after the malformed flood must still be
        // decoded and delivered.
        let mut raw_buffer = vec![0_u8; mtu];
        let raw_len = {
            let mut output = OutputBuffer::new(&mut raw_buffer[..]);
            Packet::default().serialize(&mut output).expect("serialize packet");
            output.offset()
        };
        let mut wire_buffer = vec![0_u8; tcp_wire_buffer_capacity(mtu)];
        let wire_len = {
            let mut output = OutputBuffer::new(&mut wire_buffer[..]);
            Hdlc::encode(&raw_buffer[..raw_len], &mut output).expect("encode frame");
            output.offset()
        };
        peer.write_all(&wire_buffer[..wire_len]).await.expect("write valid frame");

        let message = tokio::time::timeout(Duration::from_secs(2), rx_messages.recv())
            .await
            .expect("packet deadline")
            .expect("packet after malformed stream");
        assert_eq!(message.address, iface_address);

        cancel.cancel();
        drop(peer);
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("hdlc stream task timed out")
            .expect("hdlc stream task");
    }

    #[tokio::test]
    async fn hdlc_watchdog_writes_keepalive_and_reports_event() {
        let (stream, mut peer) = tokio::io::duplex(64);
        let (read_stream, write_stream) = tokio::io::split(stream);
        let cancel = CancellationToken::new();
        let iface_stop = CancellationToken::new();
        let (rx_channel, _rx_messages) = tokio::sync::mpsc::channel(1);
        let (_tx_sender, tx_receiver) = tokio::sync::mpsc::channel(1);
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::channel(HDLC_STREAM_EVENT_CHANNEL_CAPACITY);

        let handle = tokio::spawn(run_hdlc_stream_with_runtime(
            "test".to_string(),
            AddressHash::new([0x44; 16]),
            256,
            cancel.clone(),
            iface_stop,
            rx_channel,
            Arc::new(tokio::sync::Mutex::new(tx_receiver)),
            read_stream,
            write_stream,
            HdlcStreamRuntime::new()
                .with_watchdog(HdlcStreamWatchdog {
                    keepalive_after: Duration::from_millis(10),
                    stale_after: Duration::from_secs(1),
                    read_timeout: Duration::from_secs(5),
                })
                .with_events(event_tx),
        ));

        let mut keepalive = [0_u8; 2];
        tokio::time::timeout(Duration::from_secs(3), peer.read_exact(&mut keepalive))
            .await
            .expect("keepalive deadline")
            .expect("read keepalive");
        assert_eq!(keepalive, [0x7e, 0x7e]);

        let mut saw_keepalive = false;
        for _ in 0..4 {
            let event = tokio::time::timeout(Duration::from_secs(3), event_rx.recv())
                .await
                .expect("event deadline")
                .expect("watchdog event");
            if matches!(event, HdlcStreamEvent::Keepalive) {
                saw_keepalive = true;
                break;
            }
        }
        assert!(saw_keepalive);

        cancel.cancel();
        drop(peer);
        handle.await.expect("hdlc stream task");
    }

    #[tokio::test]
    async fn hdlc_watchdog_reports_stale_active_and_read_timeout_order() {
        let (stream, mut peer) = tokio::io::duplex(256);
        let (read_stream, write_stream) = tokio::io::split(stream);
        let cancel = CancellationToken::new();
        let iface_stop = CancellationToken::new();
        let (rx_channel, _rx_messages) = tokio::sync::mpsc::channel(1);
        let (_tx_sender, tx_receiver) = tokio::sync::mpsc::channel(1);
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::channel(HDLC_STREAM_EVENT_CHANNEL_CAPACITY);

        let handle = tokio::spawn(run_hdlc_stream_with_runtime(
            "test".to_string(),
            AddressHash::new([0x46; 16]),
            256,
            cancel.clone(),
            iface_stop,
            rx_channel,
            Arc::new(tokio::sync::Mutex::new(tx_receiver)),
            read_stream,
            write_stream,
            HdlcStreamRuntime::new()
                .with_watchdog(HdlcStreamWatchdog {
                    keepalive_after: Duration::from_millis(10),
                    stale_after: Duration::from_millis(1500),
                    read_timeout: Duration::from_millis(3500),
                })
                .with_events(event_tx),
        ));

        let first_event = tokio::time::timeout(Duration::from_secs(3), event_rx.recv())
            .await
            .expect("first watchdog event deadline")
            .expect("first watchdog event");
        assert_eq!(first_event, HdlcStreamEvent::Keepalive);

        loop {
            let event = tokio::time::timeout(Duration::from_secs(4), event_rx.recv())
                .await
                .expect("stale event deadline")
                .expect("watchdog event");
            if event == HdlcStreamEvent::Stale {
                break;
            }
            assert_ne!(event, HdlcStreamEvent::ReadTimeout);
        }

        peer.write_all(&[0x7e, 0x7e]).await.expect("write empty HDLC frame");
        loop {
            let event = tokio::time::timeout(Duration::from_secs(4), event_rx.recv())
                .await
                .expect("active event deadline")
                .expect("watchdog event");
            if event == HdlcStreamEvent::Active {
                break;
            }
            assert_ne!(event, HdlcStreamEvent::ReadTimeout);
        }

        loop {
            let event = tokio::time::timeout(Duration::from_secs(6), event_rx.recv())
                .await
                .expect("read-timeout event deadline")
                .expect("watchdog event");
            if event == HdlcStreamEvent::ReadTimeout {
                break;
            }
        }

        drop(peer);
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("hdlc stream task timed out")
            .expect("hdlc stream task");
    }

    #[tokio::test]
    async fn backbone_hdlc_stream_backpressures_when_peer_stops_reading() {
        let (stream, mut peer) = tokio::io::duplex(1);
        let (read_stream, write_stream) = tokio::io::split(stream);
        let cancel = CancellationToken::new();
        let iface_stop = CancellationToken::new();
        let (rx_channel, _rx_messages) = tokio::sync::mpsc::channel(1);
        let (tx_sender, tx_receiver) = tokio::sync::mpsc::channel(1);

        let handle = tokio::spawn(run_hdlc_stream_with_runtime(
            "backbone-test".to_string(),
            AddressHash::new([0x47; 16]),
            256,
            cancel.clone(),
            iface_stop,
            rx_channel,
            Arc::new(tokio::sync::Mutex::new(tx_receiver)),
            read_stream,
            write_stream,
            HdlcStreamRuntime::new().with_watchdog(super::backbone_hdlc_watchdog()),
        ));

        tx_sender
            .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
            .await
            .expect("queue first packet");

        let mut first_wire_byte = [0_u8; 1];
        tokio::time::timeout(Duration::from_secs(1), peer.read_exact(&mut first_wire_byte))
            .await
            .expect("first wire byte deadline")
            .expect("read first wire byte");
        assert_eq!(first_wire_byte[0], 0x7e);
        tokio::task::yield_now().await;

        tx_sender
            .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
            .await
            .expect("queue second packet behind blocked writer");

        let third_send = tx_sender
            .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() });
        assert!(
            tokio::time::timeout(Duration::from_millis(100), third_send).await.is_err(),
            "slow Backbone peer should backpressure the tx queue instead of draining unbounded work"
        );

        cancel.cancel();
        drop(tx_sender);
        drop(peer);
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("hdlc stream task timed out")
            .expect("hdlc stream task");
    }

    #[tokio::test]
    async fn hdlc_stream_forced_bitrate_delays_packet_writes() {
        let (stream, mut peer) = tokio::io::duplex(256);
        let (read_stream, write_stream) = tokio::io::split(stream);
        let cancel = CancellationToken::new();
        let iface_stop = CancellationToken::new();
        let (rx_channel, _rx_messages) = tokio::sync::mpsc::channel(1);
        let (tx_sender, tx_receiver) = tokio::sync::mpsc::channel(1);

        let handle = tokio::spawn(run_hdlc_stream_with_runtime(
            "test".to_string(),
            AddressHash::new([0x45; 16]),
            256,
            cancel.clone(),
            iface_stop,
            rx_channel,
            Arc::new(tokio::sync::Mutex::new(tx_receiver)),
            read_stream,
            write_stream,
            HdlcStreamRuntime::new().with_forced_bitrate(1_000),
        ));

        tx_sender
            .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
            .await
            .expect("queue tx packet");

        let early = tokio::time::timeout(Duration::from_millis(20), peer.read_u8()).await;
        assert!(early.is_err(), "forced bitrate should delay the first wire byte");

        let mut first_byte = [0_u8; 1];
        tokio::time::timeout(Duration::from_secs(1), peer.read_exact(&mut first_byte))
            .await
            .expect("delayed write deadline")
            .expect("read delayed wire byte");
        assert_eq!(first_byte[0], 0x7e);

        cancel.cancel();
        peer.shutdown().await.expect("shutdown peer");
        handle.await.expect("hdlc stream task");
    }

    #[tokio::test]
    async fn tcp_client_spawn_uses_configured_hdlc_watchdog_keepalive() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let peer = TcpStream::connect(addr).await.expect("connect peer");
        let (server_stream, _) = listener.accept().await.expect("accept stream");
        let mut peer = peer;
        let mut manager = InterfaceManager::new(8);
        let client = TcpClient::new_from_stream(addr.to_string(), server_stream)
            .with_hdlc_watchdog(HdlcStreamWatchdog {
                keepalive_after: Duration::from_millis(10),
                stale_after: Duration::from_secs(1),
                read_timeout: Duration::from_secs(5),
            });
        let runtime_status = client.runtime_status_handle();
        let context = manager.new_context(client);
        let cancel = context.cancel.clone();
        let task = tokio::spawn(TcpClient::spawn(context));

        let mut keepalive = [0_u8; 2];
        tokio::time::timeout(Duration::from_secs(3), peer.read_exact(&mut keepalive))
            .await
            .expect("keepalive deadline")
            .expect("read keepalive");

        cancel.cancel();
        drop(peer);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("client task timed out")
            .expect("client task");

        assert_eq!(keepalive, [0x7e, 0x7e]);
        let status = runtime_status.to_json();
        assert_eq!(status["stream_state"].as_str(), Some("closed"));
        assert_eq!(status["liveness_enabled"].as_bool(), Some(true));
        assert_eq!(status["keepalives_sent"].as_u64(), Some(1));
    }

    #[tokio::test]
    async fn tcp_client_without_watchdog_does_not_emit_idle_keepalive() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let peer = TcpStream::connect(addr).await.expect("connect peer");
        let (server_stream, _) = listener.accept().await.expect("accept stream");
        let mut peer = peer;
        let mut manager = InterfaceManager::new(8);
        let context =
            manager.new_context(TcpClient::new_from_stream(addr.to_string(), server_stream));
        let cancel = context.cancel.clone();
        let task = tokio::spawn(TcpClient::spawn(context));

        let mut bytes = [0_u8; 2];
        let read =
            tokio::time::timeout(Duration::from_millis(200), peer.read_exact(&mut bytes)).await;

        cancel.cancel();
        drop(peer);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("client task timed out")
            .expect("client task");

        assert!(read.is_err(), "ordinary tcp client emitted an idle keepalive");
    }

    #[tokio::test]
    async fn tcp_client_reports_reconnect_events_after_initial_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::channel(32);
        let mut manager = InterfaceManager::new(8);
        let context = manager
            .new_context(TcpClient::new(addr.to_string()).with_reconnect_events(reconnect_tx));
        let iface_address = context.channel.address;
        let cancel = context.cancel.clone();
        let iface_stop = context.channel.stop.clone();
        let task = tokio::spawn(TcpClient::spawn(context));

        let (first_stream, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("first connect timed out")
            .expect("first accept");
        drop(first_stream);

        let (second_stream, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("second reconnect timed out")
            .expect("second accept");
        let reconnected_iface = tokio::time::timeout(Duration::from_secs(2), reconnect_rx.recv())
            .await
            .expect("reconnect event timed out")
            .expect("reconnect event");

        cancel.cancel();
        drop(second_stream);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("client task timed out")
            .expect("client task");

        assert!(iface_stop.is_cancelled());
        assert_eq!(reconnected_iface, iface_address);
    }

    /// Returns `Err` (a TCP reset, not a graceful FIN) on its very first
    /// poll — the case the existing reconnect tests above don't cover,
    /// since dropping one end of a real/duplex stream surfaces as `Ok(0)`.
    struct ResetOnceStream;

    impl tokio::io::AsyncRead for ResetOnceStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "simulated reset",
            )))
        }
    }

    #[tokio::test]
    async fn read_error_cancels_stop_so_the_tx_task_does_not_hang_forever() {
        // The exact scenario the P1 review on PR #494 flagged: no watchdog
        // configured and nothing queued to send means the tx task's only
        // live `select!` branches are `cancel`/`iface_stop`/`stop` — if the
        // rx task's read-error arm doesn't cancel `stop`, tx_task.await
        // (and so this whole function) never returns.
        let cancel = CancellationToken::new();
        let iface_stop = CancellationToken::new();
        let (rx_channel, _rx_messages) = tokio::sync::mpsc::channel(1);
        let (_tx_sender, tx_receiver) = tokio::sync::mpsc::channel(1);
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::channel(HDLC_STREAM_EVENT_CHANNEL_CAPACITY);

        let handle = tokio::spawn(run_hdlc_stream_with_runtime(
            "test".to_string(),
            AddressHash::new([0x47; 16]),
            256,
            cancel,
            iface_stop,
            rx_channel,
            Arc::new(tokio::sync::Mutex::new(tx_receiver)),
            ResetOnceStream,
            tokio::io::sink(),
            HdlcStreamRuntime::new().with_events(event_tx),
        ));

        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("run_hdlc_stream_with_runtime hung — tx task never saw stop cancelled")
            .expect("hdlc stream task panicked");

        let mut saw_error_event = false;
        while let Ok(event) = event_rx.try_recv() {
            if matches!(event, HdlcStreamEvent::Error { .. }) {
                saw_error_event = true;
            }
        }
        assert!(saw_error_event, "expected an Error event from the simulated reset");
    }
}

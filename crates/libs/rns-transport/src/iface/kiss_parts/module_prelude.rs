use std::collections::VecDeque;

use std::sync::{Arc, Mutex};

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
    payload_adapter: KissPayloadAdapter,
    runtime_status: KissRuntimeStatusHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KissRuntimeStatus {
    pub link_state: String,
    pub bearer: String,
    pub device: Option<String>,
    pub endpoint: Option<String>,
    pub baud_rate: Option<u32>,
    pub mtu: usize,
    pub preamble_ms: u16,
    pub tx_tail_ms: u16,
    pub persistence: u8,
    pub slot_time_ms: u16,
    pub kiss_flow_control: bool,
    pub ax25: bool,
    pub iface: Option<String>,
    pub interface_ready: bool,
    pub pending_depth: usize,
    pub reconnect_attempts: u64,
    pub open_errors: u64,
    pub connect_errors: u64,
    pub packets_rx: u64,
    pub packets_tx: u64,
    pub data_frames_rx: u64,
    pub data_frames_tx: u64,
    pub command_frames_rx: u64,
    pub ready_frames_rx: u64,
    pub init_frames_tx: u64,
    pub shutdown_frames_tx: u64,
    pub management_frames_tx: u64,
    pub activity_frames_tx: u64,
    pub id_beacon_frames_tx: u64,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub decode_errors: u64,
    pub deserialize_errors: u64,
    pub rx_queue_errors: u64,
    pub serialize_errors: u64,
    pub read_errors: u64,
    pub tx_errors: u64,
    pub eof_count: u64,
    pub flow_control_timeouts: u64,
    pub ax25_drops: u64,
    pub data_notifications_dropped: u64,
    pub command_notifications_dropped: u64,
    pub last_error: Option<String>,
}

impl KissRuntimeStatus {
    #[must_use]
    pub fn new_serial(
        device: String,
        baud_rate: u32,
        mtu: usize,
        kiss: &KissConfig,
        payload_adapter: &KissPayloadAdapter,
    ) -> Self {
        Self::new(
            "serial",
            Some(device),
            None,
            Some(baud_rate),
            mtu,
            kiss,
            matches!(payload_adapter, KissPayloadAdapter::Ax25(_)),
        )
    }

    #[must_use]
    pub fn new_tcp(endpoint: String, mtu: usize, kiss: &KissConfig) -> Self {
        Self::new("tcp", None, Some(endpoint), None, mtu, kiss, false)
    }

    fn new(
        bearer: &str,
        device: Option<String>,
        endpoint: Option<String>,
        baud_rate: Option<u32>,
        mtu: usize,
        kiss: &KissConfig,
        ax25: bool,
    ) -> Self {
        Self {
            link_state: "configured".to_string(),
            bearer: bearer.to_string(),
            device,
            endpoint,
            baud_rate,
            mtu,
            preamble_ms: kiss.preamble_ms,
            tx_tail_ms: kiss.tx_tail_ms,
            persistence: kiss.persistence,
            slot_time_ms: kiss.slot_time_ms,
            kiss_flow_control: kiss.flow_control,
            ax25,
            iface: None,
            interface_ready: true,
            pending_depth: 0,
            reconnect_attempts: 0,
            open_errors: 0,
            connect_errors: 0,
            packets_rx: 0,
            packets_tx: 0,
            data_frames_rx: 0,
            data_frames_tx: 0,
            command_frames_rx: 0,
            ready_frames_rx: 0,
            init_frames_tx: 0,
            shutdown_frames_tx: 0,
            management_frames_tx: 0,
            activity_frames_tx: 0,
            id_beacon_frames_tx: 0,
            bytes_rx: 0,
            bytes_tx: 0,
            decode_errors: 0,
            deserialize_errors: 0,
            rx_queue_errors: 0,
            serialize_errors: 0,
            read_errors: 0,
            tx_errors: 0,
            eof_count: 0,
            flow_control_timeouts: 0,
            ax25_drops: 0,
            data_notifications_dropped: 0,
            command_notifications_dropped: 0,
            last_error: None,
        }
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        root.insert("link_state".to_string(), self.link_state.clone().into());
        root.insert("bearer".to_string(), self.bearer.clone().into());
        root.insert("device".to_string(), optional_string_json(self.device.as_deref()));
        root.insert("endpoint".to_string(), optional_string_json(self.endpoint.as_deref()));
        root.insert("baud_rate".to_string(), optional_u32_json(self.baud_rate));
        root.insert("mtu".to_string(), self.mtu.into());
        root.insert("preamble_ms".to_string(), self.preamble_ms.into());
        root.insert("tx_tail_ms".to_string(), self.tx_tail_ms.into());
        root.insert("persistence".to_string(), self.persistence.into());
        root.insert("slot_time_ms".to_string(), self.slot_time_ms.into());
        root.insert("kiss_flow_control".to_string(), self.kiss_flow_control.into());
        root.insert("ax25".to_string(), self.ax25.into());
        root.insert("iface".to_string(), optional_string_json(self.iface.as_deref()));
        root.insert("interface_ready".to_string(), self.interface_ready.into());
        root.insert("pending_depth".to_string(), self.pending_depth.into());
        root.insert("reconnect_attempts".to_string(), self.reconnect_attempts.into());
        root.insert("open_errors".to_string(), self.open_errors.into());
        root.insert("connect_errors".to_string(), self.connect_errors.into());
        root.insert("packets_rx".to_string(), self.packets_rx.into());
        root.insert("packets_tx".to_string(), self.packets_tx.into());
        root.insert("data_frames_rx".to_string(), self.data_frames_rx.into());
        root.insert("data_frames_tx".to_string(), self.data_frames_tx.into());
        root.insert("command_frames_rx".to_string(), self.command_frames_rx.into());
        root.insert("ready_frames_rx".to_string(), self.ready_frames_rx.into());
        root.insert("init_frames_tx".to_string(), self.init_frames_tx.into());
        root.insert("shutdown_frames_tx".to_string(), self.shutdown_frames_tx.into());
        root.insert("management_frames_tx".to_string(), self.management_frames_tx.into());
        root.insert("activity_frames_tx".to_string(), self.activity_frames_tx.into());
        root.insert("id_beacon_frames_tx".to_string(), self.id_beacon_frames_tx.into());
        root.insert("bytes_rx".to_string(), self.bytes_rx.into());
        root.insert("bytes_tx".to_string(), self.bytes_tx.into());
        root.insert("decode_errors".to_string(), self.decode_errors.into());
        root.insert("deserialize_errors".to_string(), self.deserialize_errors.into());
        root.insert("rx_queue_errors".to_string(), self.rx_queue_errors.into());
        root.insert("serialize_errors".to_string(), self.serialize_errors.into());
        root.insert("read_errors".to_string(), self.read_errors.into());
        root.insert("tx_errors".to_string(), self.tx_errors.into());
        root.insert("eof_count".to_string(), self.eof_count.into());
        root.insert("flow_control_timeouts".to_string(), self.flow_control_timeouts.into());
        root.insert("ax25_drops".to_string(), self.ax25_drops.into());
        root.insert(
            "data_notifications_dropped".to_string(),
            self.data_notifications_dropped.into(),
        );
        root.insert(
            "command_notifications_dropped".to_string(),
            self.command_notifications_dropped.into(),
        );
        root.insert("last_error".to_string(), optional_string_json(self.last_error.as_deref()));
        serde_json::Value::Object(root)
    }
}

fn optional_string_json(value: Option<&str>) -> serde_json::Value {
    value.map_or(serde_json::Value::Null, serde_json::Value::from)
}

fn optional_u32_json(value: Option<u32>) -> serde_json::Value {
    value.map_or(serde_json::Value::Null, serde_json::Value::from)
}

#[derive(Debug, Clone)]
pub struct KissRuntimeStatusHandle {
    inner: Arc<Mutex<KissRuntimeStatus>>,
}

impl KissRuntimeStatusHandle {
    fn new(status: KissRuntimeStatus) -> Self {
        Self { inner: Arc::new(Mutex::new(status)) }
    }

    pub fn update(&self, update: impl FnOnce(&mut KissRuntimeStatus)) {
        update(&mut self.inner.lock().expect("kiss runtime status mutex poisoned"));
    }

    #[must_use]
    pub fn snapshot(&self) -> KissRuntimeStatus {
        self.inner.lock().expect("kiss runtime status mutex poisoned").clone()
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.snapshot().to_json()
    }
}

impl KissInterface {
    #[must_use]
    pub fn new<T: Into<String>>(device: T, baud_rate: u32) -> Self {
        let device = device.into();
        let mtu = 564;
        let kiss = KissConfig::default();
        let payload_adapter = KissPayloadAdapter::Raw;
        let runtime_status = KissRuntimeStatusHandle::new(KissRuntimeStatus::new_serial(
            device.clone(),
            baud_rate,
            mtu,
            &kiss,
            &payload_adapter,
        ));
        Self {
            device,
            baud_rate,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            mtu,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            kiss,
            payload_adapter,
            runtime_status,
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
    pub fn kiss_config(&self) -> KissConfig {
        self.kiss.clone()
    }

    #[must_use]
    pub fn with_payload_adapter(mut self, payload_adapter: KissPayloadAdapter) -> Self {
        self.payload_adapter = payload_adapter;
        self.runtime_status.update(|status| {
            status.ax25 = matches!(self.payload_adapter, KissPayloadAdapter::Ax25(_));
        });
        self
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> KissRuntimeStatusHandle {
        self.runtime_status.clone()
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
            payload_adapter,
            runtime_status,
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
                guard.payload_adapter.clone(),
                guard.runtime_status.clone(),
            )
        };
        runtime_status.update(|status| {
            status.iface = Some(iface_address.to_string());
        });

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
                    runtime_status.update(|status| {
                        status.link_state = "open_failed".to_string();
                        status.open_errors = status.open_errors.saturating_add(1);
                        status.reconnect_attempts = status.reconnect_attempts.saturating_add(1);
                        status.last_error = Some(err.to_string());
                    });
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
            runtime_status.update(|status| {
                status.link_state = "open".to_string();
                status.last_error = None;
            });

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
                    payload_adapter: payload_adapter.clone(),
                    strip_command_port_nibble: true,
                    command_tx: None,
                    data_rx_tx: None,
                    management_frame_rx: None,
                    runtime_status: Some(runtime_status.clone()),
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
        runtime_status.update(|status| {
            status.link_state = "stopped".to_string();
        });
    }
}

impl Interface for KissInterface {
    fn mtu() -> usize {
        564
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

#[derive(Debug, Clone)]
pub struct KissTcpClientInterface {
    addr: String,
    mtu: usize,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
    kiss: KissConfig,
    runtime_status: KissRuntimeStatusHandle,
}

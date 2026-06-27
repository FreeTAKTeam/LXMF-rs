use std::sync::{Arc, Mutex};

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};

use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};

use crate::hash::AddressHash;

use crate::iface::{IfaceSource, RxMessage, TxMessage};

use crate::packet::Packet;

use crate::serde::Serialize;

use super::hdlc::Hdlc;

use super::{Interface, InterfaceContext};

pub struct SerialInterface {
    device: String,
    baud_rate: u32,
    data_bits: DataBits,
    parity: Parity,
    stop_bits: StopBits,
    flow_control: FlowControl,
    mtu: usize,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
    runtime_status: SerialRuntimeStatusHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialRuntimeStatus {
    pub link_state: String,
    pub device: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: String,
    pub stop_bits: u8,
    pub flow_control: String,
    pub mtu: usize,
    pub iface: Option<String>,
    pub reconnect_attempts: u64,
    pub open_errors: u64,
    pub packets_rx: u64,
    pub packets_tx: u64,
    pub frames_rx: u64,
    pub frames_tx: u64,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub decode_errors: u64,
    pub deserialize_errors: u64,
    pub rx_queue_errors: u64,
    pub serialize_errors: u64,
    pub hdlc_encode_errors: u64,
    pub tx_errors: u64,
    pub read_errors: u64,
    pub eof_count: u64,
    pub last_error: Option<String>,
}

impl SerialRuntimeStatus {
    #[must_use]
    pub fn new(
        device: String,
        baud_rate: u32,
        data_bits: DataBits,
        parity: Parity,
        stop_bits: StopBits,
        flow_control: FlowControl,
        mtu: usize,
    ) -> Self {
        Self {
            link_state: "configured".to_string(),
            device,
            baud_rate,
            data_bits: serial_data_bits_value(data_bits),
            parity: serial_parity_name(parity).to_string(),
            stop_bits: serial_stop_bits_value(stop_bits),
            flow_control: serial_flow_control_name(flow_control).to_string(),
            mtu,
            iface: None,
            reconnect_attempts: 0,
            open_errors: 0,
            packets_rx: 0,
            packets_tx: 0,
            frames_rx: 0,
            frames_tx: 0,
            bytes_rx: 0,
            bytes_tx: 0,
            decode_errors: 0,
            deserialize_errors: 0,
            rx_queue_errors: 0,
            serialize_errors: 0,
            hdlc_encode_errors: 0,
            tx_errors: 0,
            read_errors: 0,
            eof_count: 0,
            last_error: None,
        }
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "link_state": self.link_state,
            "device": self.device,
            "baud_rate": self.baud_rate,
            "data_bits": self.data_bits,
            "parity": self.parity,
            "stop_bits": self.stop_bits,
            "flow_control": self.flow_control,
            "mtu": self.mtu,
            "iface": self.iface,
            "reconnect_attempts": self.reconnect_attempts,
            "open_errors": self.open_errors,
            "packets_rx": self.packets_rx,
            "packets_tx": self.packets_tx,
            "frames_rx": self.frames_rx,
            "frames_tx": self.frames_tx,
            "bytes_rx": self.bytes_rx,
            "bytes_tx": self.bytes_tx,
            "decode_errors": self.decode_errors,
            "deserialize_errors": self.deserialize_errors,
            "rx_queue_errors": self.rx_queue_errors,
            "serialize_errors": self.serialize_errors,
            "hdlc_encode_errors": self.hdlc_encode_errors,
            "tx_errors": self.tx_errors,
            "read_errors": self.read_errors,
            "eof_count": self.eof_count,
            "last_error": self.last_error,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SerialRuntimeStatusHandle {
    inner: Arc<Mutex<SerialRuntimeStatus>>,
}

impl SerialRuntimeStatusHandle {
    fn new(
        device: String,
        baud_rate: u32,
        data_bits: DataBits,
        parity: Parity,
        stop_bits: StopBits,
        flow_control: FlowControl,
        mtu: usize,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SerialRuntimeStatus::new(
                device,
                baud_rate,
                data_bits,
                parity,
                stop_bits,
                flow_control,
                mtu,
            ))),
        }
    }

    pub fn update(&self, update: impl FnOnce(&mut SerialRuntimeStatus)) {
        update(&mut self.inner.lock().expect("serial runtime status mutex poisoned"));
    }

    #[must_use]
    pub fn snapshot(&self) -> SerialRuntimeStatus {
        self.inner.lock().expect("serial runtime status mutex poisoned").clone()
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.snapshot().to_json()
    }
}

fn serial_data_bits_value(data_bits: DataBits) -> u8 {
    match data_bits {
        DataBits::Five => 5,
        DataBits::Six => 6,
        DataBits::Seven => 7,
        DataBits::Eight => 8,
    }
}

fn serial_parity_name(parity: Parity) -> &'static str {
    match parity {
        Parity::None => "none",
        Parity::Odd => "odd",
        Parity::Even => "even",
    }
}

fn serial_stop_bits_value(stop_bits: StopBits) -> u8 {
    match stop_bits {
        StopBits::One => 1,
        StopBits::Two => 2,
    }
}

fn serial_flow_control_name(flow_control: FlowControl) -> &'static str {
    match flow_control {
        FlowControl::None => "none",
        FlowControl::Software => "software",
        FlowControl::Hardware => "hardware",
    }
}

fn serial_wire_buffer_capacity(mtu: usize) -> usize {
    // Worst-case HDLC expansion doubles bytes (all escaped) plus frame delimiters.
    mtu.saturating_mul(2).saturating_add(16)
}

fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

impl SerialInterface {
    pub const DEFAULT_MTU: usize = 564;

    pub fn new<T: Into<String>>(device: T, baud_rate: u32) -> Self {
        let device = device.into();
        let data_bits = DataBits::Eight;
        let parity = Parity::None;
        let stop_bits = StopBits::One;
        let flow_control = FlowControl::None;
        let mtu = Self::DEFAULT_MTU;
        let runtime_status = SerialRuntimeStatusHandle::new(
            device.clone(),
            baud_rate,
            data_bits,
            parity,
            stop_bits,
            flow_control,
            mtu,
        );
        Self {
            device,
            baud_rate,
            data_bits,
            parity,
            stop_bits,
            flow_control,
            mtu,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            runtime_status,
        }
    }

    pub fn with_data_bits(mut self, data_bits: DataBits) -> Self {
        self.data_bits = data_bits;
        self.runtime_status.update(|status| {
            status.data_bits = serial_data_bits_value(data_bits);
        });
        self
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
        serial_data_bits_value(self.data_bits)
    }

    #[must_use]
    pub fn parity_name(&self) -> &'static str {
        serial_parity_name(self.parity)
    }

    #[must_use]
    pub fn stop_bits_value(&self) -> u8 {
        serial_stop_bits_value(self.stop_bits)
    }

    #[must_use]
    pub fn mtu_value(&self) -> usize {
        self.mtu
    }

    pub fn with_data_bits_raw(self, data_bits: u8) -> Result<Self, String> {
        let data_bits = match data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            8 => DataBits::Eight,
            _ => {
                return Err(format!(
                    "serial.data_bits must be one of: 5, 6, 7, 8 (got {data_bits})"
                ))
            }
        };
        Ok(self.with_data_bits(data_bits))
    }

    pub fn with_parity(mut self, parity: Parity) -> Self {
        self.parity = parity;
        self.runtime_status.update(|status| {
            status.parity = serial_parity_name(parity).to_string();
        });
        self
    }

    pub fn with_parity_name(self, parity: &str) -> Result<Self, String> {
        let parity = match parity.trim().to_ascii_lowercase().as_str() {
            "n" | "none" => Parity::None,
            "e" | "even" => Parity::Even,
            "o" | "odd" => Parity::Odd,
            _ => {
                return Err(format!(
                    "serial.parity must be one of: n, none, e, even, o, odd (got {parity})"
                ))
            }
        };
        Ok(self.with_parity(parity))
    }

    pub fn with_stop_bits(mut self, stop_bits: StopBits) -> Self {
        self.stop_bits = stop_bits;
        self.runtime_status.update(|status| {
            status.stop_bits = serial_stop_bits_value(stop_bits);
        });
        self
    }

    pub fn with_stop_bits_raw(self, stop_bits: u8) -> Result<Self, String> {
        let stop_bits = match stop_bits {
            1 => StopBits::One,
            2 => StopBits::Two,
            _ => return Err(format!("serial.stop_bits must be one of: 1, 2 (got {stop_bits})")),
        };
        Ok(self.with_stop_bits(stop_bits))
    }

    pub fn with_flow_control(mut self, flow_control: FlowControl) -> Self {
        self.flow_control = flow_control;
        self.runtime_status.update(|status| {
            status.flow_control = serial_flow_control_name(flow_control).to_string();
        });
        self
    }

    pub fn with_flow_control_name(self, flow_control: &str) -> Result<Self, String> {
        let flow_control = match flow_control.trim().to_ascii_lowercase().as_str() {
            "none" => FlowControl::None,
            "software" => FlowControl::Software,
            "hardware" => FlowControl::Hardware,
            _ => {
                return Err(format!(
                "serial.flow_control must be one of: none, software, hardware (got {flow_control})"
            ))
            }
        };
        Ok(self.with_flow_control(flow_control))
    }

    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(256);
        self.runtime_status.update(|status| {
            status.mtu = self.mtu;
        });
        self
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> SerialRuntimeStatusHandle {
        self.runtime_status.clone()
    }

    pub fn with_reconnect_backoff(mut self, reconnect_backoff: Duration) -> Self {
        self.reconnect_backoff = reconnect_backoff;
        if self.max_reconnect_backoff < self.reconnect_backoff {
            self.max_reconnect_backoff = self.reconnect_backoff;
        }
        self
    }

    pub fn with_max_reconnect_backoff(mut self, max_reconnect_backoff: Duration) -> Self {
        self.max_reconnect_backoff = max_reconnect_backoff.max(self.reconnect_backoff);
        self
    }

    pub fn preflight_open(&self) -> Result<(), String> {
        tokio_serial::new(self.device.clone(), self.baud_rate)
            .data_bits(self.data_bits)
            .parity(self.parity)
            .stop_bits(self.stop_bits)
            .flow_control(self.flow_control)
            .open_native_async()
            .map(|_| ())
            .map_err(|err| {
                format!(
                    "serial preflight open failed device={} baud_rate={} data_bits={:?} parity={:?} stop_bits={:?} flow_control={:?} err={}",
                    self.device,
                    self.baud_rate,
                    self.data_bits,
                    self.parity,
                    self.stop_bits,
                    self.flow_control,
                    err
                )
            })
    }

    pub async fn spawn(context: InterfaceContext<SerialInterface>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (
            device,
            baud_rate,
            data_bits,
            parity,
            stop_bits,
            flow_control,
            mtu,
            reconnect_backoff,
            max_reconnect_backoff,
            runtime_status,
        ) = {
            let guard = context.inner.lock().expect("serial interface mutex poisoned");
            (
                guard.device.clone(),
                guard.baud_rate,
                guard.data_bits,
                guard.parity,
                guard.stop_bits,
                guard.flow_control,
                guard.mtu,
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
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
                .flow_control(flow_control)
                .open_native_async()
            {
                Ok(port) => port,
                Err(err) => {
                    log::warn!(
                        "failed to open device={} baud_rate={} data_bits={:?} parity={:?} stop_bits={:?} flow_control={:?} err={}",
                        device,
                        baud_rate,
                        data_bits,
                        parity,
                        stop_bits,
                        flow_control,
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
                "opened device={} baud_rate={} data_bits={:?} parity={:?} stop_bits={:?} flow_control={:?} iface={}",
                device,
                baud_rate,
                data_bits,
                parity,
                stop_bits,
                flow_control,
                iface_address
            );
            active_backoff = reconnect_backoff;
            runtime_status.update(|status| {
                status.link_state = "open".to_string();
                status.last_error = None;
            });

            run_serial_stream(
                port,
                SerialStreamOptions {
                    iface_address,
                    device: device.clone(),
                    mtu,
                    cancel: context.cancel.clone(),
                    rx_channel: rx_channel.clone(),
                    tx_channel: tx_channel.clone(),
                    runtime_status: runtime_status.clone(),
                },
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

impl Interface for SerialInterface {
    fn mtu() -> usize {
        SerialInterface::DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

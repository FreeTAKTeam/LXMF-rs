use std::sync::Arc;

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
    pub fn new<T: Into<String>>(device: T, baud_rate: u32) -> Self {
        Self {
            device: device.into(),
            baud_rate,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            mtu: 2048,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
        }
    }

    pub fn with_data_bits(mut self, data_bits: DataBits) -> Self {
        self.data_bits = data_bits;
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
        self
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

            run_serial_stream(
                port,
                iface_address,
                device.clone(),
                mtu,
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

impl Interface for SerialInterface {
    fn mtu() -> usize {
        2048
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

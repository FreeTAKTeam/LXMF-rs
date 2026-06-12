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
}

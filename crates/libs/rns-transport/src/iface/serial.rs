use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::hash::AddressHash;
use crate::iface::{RxMessage, TxMessage};
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
            "none" => Parity::None,
            "even" => Parity::Even,
            "odd" => Parity::Odd,
            _ => {
                return Err(format!("serial.parity must be one of: none, even, odd (got {parity})"))
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
                        "serial: failed to open device={} baud_rate={} data_bits={:?} parity={:?} stop_bits={:?} flow_control={:?} err={}",
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
                "serial: opened device={} baud_rate={} data_bits={:?} parity={:?} stop_bits={:?} flow_control={:?} iface={}",
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
}

async fn run_serial_stream<IO>(
    stream: IO,
    iface_address: AddressHash,
    device: String,
    mtu: usize,
    cancel: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let stop = CancellationToken::new();
    let (mut read_port, mut write_port) = tokio::io::split(stream);
    let rx_device = device.clone();
    let tx_device = device;

    let rx_task = {
        let cancel = cancel.clone();
        let stop = stop.clone();
        let rx_channel = rx_channel.clone();
        tokio::spawn(async move {
            let mut hdlc_rx_buffer = vec![0_u8; mtu];
            let mut frame_buffer = Vec::<u8>::with_capacity(mtu * 4);
            let mut read_buffer = vec![0_u8; mtu.max(256)];

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = stop.cancelled() => break,
                    result = read_port.read(&mut read_buffer[..]) => {
                        match result {
                            Ok(0) => {
                                log::warn!(
                                    "serial: EOF on iface={} device={}",
                                    iface_address,
                                    rx_device
                                );
                                stop.cancel();
                                break;
                            }
                            Ok(n) => {
                                frame_buffer.extend_from_slice(&read_buffer[..n]);

                                while let Some((start, end)) = Hdlc::find(&frame_buffer) {
                                    let frame = &frame_buffer[start..=end];
                                    let mut output = OutputBuffer::new(&mut hdlc_rx_buffer[..]);
                                    if Hdlc::decode(frame, &mut output).is_ok() {
                                        if let Ok(packet) =
                                            Packet::deserialize(&mut InputBuffer::new(output.as_slice()))
                                        {
                                            let _ = rx_channel
                                                .send(RxMessage {
                                                    address: iface_address,
                                                    packet,
                                                })
                                                .await;
                                        }
                                    }
                                    frame_buffer.drain(..=end);
                                }

                                if frame_buffer.len() > mtu * 64 {
                                    frame_buffer.clear();
                                }
                            }
                            Err(err) => {
                                log::warn!(
                                    "serial: read error iface={} device={} err={}",
                                    iface_address,
                                    rx_device,
                                    err
                                );
                                stop.cancel();
                                break;
                            }
                        }
                    }
                }
            }
        })
    };

    let tx_task = {
        let cancel = cancel.clone();
        let stop = stop.clone();
        let tx_channel = tx_channel.clone();
        tokio::spawn(async move {
            loop {
                if stop.is_cancelled() {
                    break;
                }

                let mut hdlc_tx_buffer = vec![0_u8; serial_wire_buffer_capacity(mtu)];
                let mut tx_buffer = vec![0_u8; mtu];
                let mut tx_channel = tx_channel.lock().await;

                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = stop.cancelled() => break,
                    Some(message) = tx_channel.recv() => {
                        let mut output = OutputBuffer::new(&mut tx_buffer[..]);
                        if message.packet.serialize(&mut output).is_ok() {
                            let mut hdlc_output = OutputBuffer::new(&mut hdlc_tx_buffer[..]);
                            if Hdlc::encode(output.as_slice(), &mut hdlc_output).is_ok() {
                                if let Err(err) = write_port.write_all(hdlc_output.as_slice()).await {
                                    log::warn!(
                                        "serial: write error iface={} device={} err={}",
                                        iface_address,
                                        tx_device,
                                        err
                                    );
                                    stop.cancel();
                                    break;
                                }
                                if let Err(err) = write_port.flush().await {
                                    log::warn!(
                                        "serial: flush error iface={} device={} err={}",
                                        iface_address,
                                        tx_device,
                                        err
                                    );
                                    stop.cancel();
                                    break;
                                }
                            } else {
                                log::warn!(
                                    "serial: hdlc encode failed iface={} device={} payload_len={}",
                                    iface_address,
                                    tx_device,
                                    output.as_slice().len()
                                );
                            }
                        } else {
                            log::warn!(
                                "serial: packet serialize failed iface={} device={} mtu={}",
                                iface_address,
                                tx_device,
                                mtu
                            );
                        }
                    }
                }
            }
        })
    };

    let _ = tx_task.await;
    let _ = rx_task.await;
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_backoff_next, run_serial_stream, serial_wire_buffer_capacity, SerialInterface,
    };
    use crate::buffer::OutputBuffer;
    use crate::hash::AddressHash;
    use crate::iface::{hdlc::Hdlc, InterfaceChannel, InterfaceContext, TxMessage, TxMessageType};
    use crate::packet::Packet;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;
    use tokio::sync::mpsc;
    use tokio::time::timeout;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn wire_capacity_handles_worst_case_hdlc_escape_expansion() {
        let mtu = 512;
        let raw = vec![0x7e_u8; mtu];
        let mut wire = vec![0_u8; serial_wire_buffer_capacity(mtu)];
        let mut output = OutputBuffer::new(&mut wire[..]);

        let encoded_len = Hdlc::encode(&raw, &mut output).expect("encode worst-case payload");
        assert!(encoded_len >= (mtu * 2) + 2, "wire len must cover escaped payload plus flags");
    }

    #[test]
    fn wire_capacity_grows_with_configured_mtu() {
        assert!(serial_wire_buffer_capacity(256) < serial_wire_buffer_capacity(2048));
    }

    #[test]
    fn reconnect_backoff_growth_is_bounded() {
        assert_eq!(
            bounded_backoff_next(Duration::from_millis(500), Duration::from_millis(5_000)),
            Duration::from_millis(1_000)
        );
        assert_eq!(
            bounded_backoff_next(Duration::from_millis(4_000), Duration::from_millis(5_000)),
            Duration::from_millis(5_000)
        );
        assert_eq!(
            bounded_backoff_next(Duration::from_millis(5_000), Duration::from_millis(5_000)),
            Duration::from_millis(5_000)
        );
    }

    #[test]
    fn serial_option_helpers_reject_invalid_values() {
        let err = SerialInterface::new("dummy", 115200)
            .with_data_bits_raw(9)
            .err()
            .expect("invalid data bits");
        assert!(err.contains("serial.data_bits"));

        let err = SerialInterface::new("dummy", 115200)
            .with_stop_bits_raw(3)
            .err()
            .expect("invalid stop bits");
        assert!(err.contains("serial.stop_bits"));

        let err = SerialInterface::new("dummy", 115200)
            .with_parity_name("mark")
            .err()
            .expect("invalid parity");
        assert!(err.contains("serial.parity"));

        let err = SerialInterface::new("dummy", 115200)
            .with_flow_control_name("xonxoff")
            .err()
            .expect("invalid flow control");
        assert!(err.contains("serial.flow_control"));
    }

    #[tokio::test]
    async fn spawn_retry_loop_honors_cancel_after_open_failures() {
        let (rx_send, _rx_recv) = InterfaceChannel::make_rx_channel(1);
        let (_tx_send, tx_recv) = InterfaceChannel::make_tx_channel(1);
        let stop = CancellationToken::new();
        let channel = InterfaceChannel::new(
            rx_send,
            tx_recv,
            AddressHash::new_from_slice(b"serial-cancel"),
            stop.clone(),
        );
        let cancel = CancellationToken::new();
        let context = InterfaceContext::<SerialInterface> {
            inner: Arc::new(Mutex::new(
                SerialInterface::new("__definitely_not_a_device__", 115200)
                    .with_reconnect_backoff(Duration::from_millis(25)),
            )),
            channel,
            cancel: cancel.clone(),
        };

        let task = tokio::spawn(async move {
            SerialInterface::spawn(context).await;
        });

        tokio::time::sleep(Duration::from_millis(90)).await;
        cancel.cancel();

        timeout(Duration::from_secs(2), task)
            .await
            .expect("serial spawn should stop after cancel")
            .expect("join serial task");
        assert!(stop.is_cancelled(), "stop token should be cancelled on shutdown");
    }

    #[tokio::test]
    async fn serial_stream_stops_after_write_failure() {
        let (io_a, io_b) = tokio::io::duplex(64);
        drop(io_b);

        let (rx_send, _rx_recv) = mpsc::channel(4);
        let (tx_send, tx_recv) = mpsc::channel(4);
        let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
        let cancel = CancellationToken::new();

        let session = tokio::spawn(run_serial_stream(
            io_a,
            AddressHash::new_from_slice(b"serial-write-fail"),
            "duplex".to_string(),
            512,
            cancel.clone(),
            rx_send,
            tx_recv,
        ));

        tx_send
            .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
            .await
            .expect("queue tx message");

        timeout(Duration::from_secs(1), session)
            .await
            .expect("session should stop on write failure")
            .expect("join session task");
    }

    #[tokio::test]
    async fn serial_stream_survives_malformed_frame_then_eof() {
        let (io_a, mut io_b) = tokio::io::duplex(256);
        let (rx_send, mut rx_recv) = mpsc::channel(4);
        let (_tx_send, tx_recv) = mpsc::channel(4);
        let tx_recv = Arc::new(tokio::sync::Mutex::new(tx_recv));
        let cancel = CancellationToken::new();

        let session = tokio::spawn(run_serial_stream(
            io_a,
            AddressHash::new_from_slice(b"serial-malformed"),
            "duplex".to_string(),
            512,
            cancel.clone(),
            rx_send,
            tx_recv,
        ));

        io_b.write_all(&[0x7e, 0x7d, 0x00, 0x7e]).await.expect("write malformed frame");
        drop(io_b);

        timeout(Duration::from_secs(1), session)
            .await
            .expect("session should stop on EOF")
            .expect("join session task");
        assert!(rx_recv.try_recv().is_err(), "malformed frame must not emit packets");
    }
}

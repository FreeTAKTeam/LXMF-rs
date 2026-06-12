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
                                    "EOF on iface={} device={}",
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
                                                    source: IfaceSource::None,
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
                                    "read error iface={} device={} err={}",
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
                                        "write error iface={} device={} err={}",
                                        iface_address,
                                        tx_device,
                                        err
                                    );
                                    stop.cancel();
                                    break;
                                }
                                if let Err(err) = write_port.flush().await {
                                    log::warn!(
                                        "flush error iface={} device={} err={}",
                                        iface_address,
                                        tx_device,
                                        err
                                    );
                                    stop.cancel();
                                    break;
                                }
                            } else {
                                log::warn!(
                                    "hdlc encode failed iface={} device={} payload_len={}",
                                    iface_address,
                                    tx_device,
                                    output.as_slice().len()
                                );
                            }
                        } else {
                            log::warn!(
                                "packet serialize failed iface={} device={} mtu={}",
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

    #[test]
    fn preflight_open_reports_device_open_failures() {
        let err = SerialInterface::new("__definitely_not_a_device__", 115200)
            .preflight_open()
            .expect_err("invalid device should fail preflight");
        assert!(err.contains("serial preflight open failed"));
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

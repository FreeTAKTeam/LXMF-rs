use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_hdlc_stream_with_runtime_and_ifac<R, W>(
    label: String,
    iface_address: crate::hash::AddressHash,
    mtu: usize,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: InterfaceRxSender,
    tx_channel: Arc<tokio::sync::Mutex<InterfaceTxReceiver>>,
    ifac_state: IfacState,
    ifac_violations: Arc<std::sync::atomic::AtomicU64>,
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
    let wire_mtu = mtu.saturating_add(crate::iface::MAX_IFAC_SIZE_BYTES);
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
        let ifac_state = ifac_state.clone();
        let ifac_violations = ifac_violations.clone();

        tokio::spawn(async move {
            let mut buffers = TcpRxBuffers::new(wire_mtu);

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
                    result = stream.read(buffers.tcp.as_mut_slice()) => {
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
                                    buffers.frame.extend_from_slice(&buffers.tcp[..n]);

                                    while let Some((start, end)) = Hdlc::find(&buffers.frame) {
                                        // Removing the two HDLC flags is an upper bound for
                                        // decoded data; escaped frames decode to less. The MTU
                                        // remains the hard ceiling.
                                        let decoded_len = (end - start + 1)
                                            .saturating_sub(2)
                                            .min(wire_mtu);
                                        buffers.decoded.resize(decoded_len, 0);
                                        let frame = &buffers.frame[start..=end];
                                        let mut output = OutputBuffer::new(buffers.decoded.as_mut_slice());
                                        if Hdlc::decode(frame, &mut output).is_ok() {
                                            // RNS 1.5.2 treats an empty HDLC
                                            // frame as a keepalive, not as a
                                            // malformed packet. It must not
                                            // enter packet admission or emit a
                                            // protocol warning.
                                            if output.as_slice().is_empty() {
                                                buffers.frame.drain(..=end);
                                                continue;
                                            }
                                            match decode_packet_ifac(&ifac_state, output.as_slice()) {
                                                Ok(packet) => {
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
                                                }
                                                Err(error) => {
                                                    if is_ifac_violation(&error) {
                                                        record_ifac_violation(&ifac_violations, &error);
                                                    }
                                                    log::debug!("couldn't decode packet: {error}");
                                                }
                                            }
                                        } else {
                                            log::warn!("couldn't decode hdlc frame");
                                        }

                                        // Drop all bytes up to and including the closing
                                        // flag of the frame we just handled.
                                        buffers.frame.drain(..=end);
                                    }

                                    let frame_buffer_limit = max_hdlc_frame_buffer_len(wire_mtu);
                                    let buffered = buffers.frame.len();
                                    if trim_malformed_hdlc_backlog(
                                        &mut buffers.frame,
                                        frame_buffer_limit,
                                        wire_mtu,
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
                                            buffers.frame.len(),
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
        let ifac_state = ifac_state.clone();

        tokio::spawn(async move {
            let mut last_write_at = Instant::now();
            let mut stale = false;
            let mut buffers = TcpTxBuffers::default();
            let mut watchdog_tick = tokio::time::interval(Duration::from_secs(1));
            watchdog_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                if stop.is_cancelled() {
                    break;
                }

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
                        match buffers.encode_ifac_packet(&packet, mtu, &ifac_state) {
                            Ok((raw_len, wire)) => {
                            if let Some(bitrate_bps) = forced_bitrate_bps {
                                if let Some(delay) = forced_bitrate_delay(raw_len, bitrate_bps) {
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
                                if let Err(err) = stream.write_all(wire).await {
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
                                    HdlcStreamEvent::Write { bytes: wire.len() },
                                );
                                log::debug!(
                                    "[tp-diag] {} tx_write_ok iface={} wire_len={} raw_len={}",
                                    label,
                                    iface_address,
                                    wire.len(),
                                    raw_len
                                );
                            }
                            Err(err) => {
                            log::warn!(
                                "[tp-diag] serialize or HDLC encode failed iface={} mtu={} err={:?}",
                                iface_address,
                                mtu,
                                err
                            );
                            }
                        }
                    }
                };
            }
        })
    };

    tx_task.await.unwrap();
    rx_task.await.unwrap();
}

pub async fn run_kiss_stream_with_ifac<IO>(
    mut stream: IO,
    mut options: KissStreamOptions,
    cancel: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
    ifac_state: IfacState,
    ifac_violations: Arc<std::sync::atomic::AtomicU64>,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut decoder = KissStreamDecoder::new(options.mtu)
        .with_command_port_nibble_stripping(options.strip_command_port_nibble);
    let mut read_buffer = vec![0_u8; options.mtu.max(256)];
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
    let operation = async {
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
            return false;
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
                    if probe
                        .silence_timeout
                        .is_some_and(|timeout| last_read_at.elapsed() >= timeout)
                    {
                        log::warn!(
                            "KISS peer silent iface={} device={} silent_ms={}",
                            options.iface_address,
                            options.device,
                            last_read_at.elapsed().as_millis()
                        );
                        update_kiss_status(&options, |status| {
                            status.link_state = "peer_silent".to_string();
                            status.last_error = Some("peer stopped answering the activity probe".to_string());
                        });
                        break;
                    }
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
                                                match decode_packet_ifac(&ifac_state, &payload) {
                                                    Ok(packet) => {
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
                                                    }
                                                    Err(error) => {
                                                        if is_ifac_violation(&error) {
                                                            record_ifac_violation(&ifac_violations, &error);
                                                        }
                                                        update_kiss_status(&options, |status| {
                                                            status.deserialize_errors =
                                                                status.deserialize_errors.saturating_add(1);
                                                            status.last_error =
                                                                Some("packet deserialize failed".to_string());
                                                        });
                                                    }
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
                    let encoded = match encode_packet_ifac(&ifac_state, &message.packet) {
                        Ok(encoded) => encoded,
                        Err(err) => {
                            log::warn!(
                                "KISS packet serialize failed iface={} device={} mtu={} err={:?}",
                                options.iface_address,
                                options.device,
                                options.mtu,
                                err
                            );
                            update_kiss_status(&options, |status| {
                                status.serialize_errors = status.serialize_errors.saturating_add(1);
                                status.last_error = Some(format!("packet serialize failed: {err:?}"));
                            });
                            continue;
                        }
                    };
                    let payload = options.payload_adapter.outbound(&encoded);
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
                }
            }
        }
        true
    };
    // Let an ordinary loop stop retain its ready shutdown frames.
    let permit_shutdown = tokio::select! {
        biased;
        completed = operation => {
            if !completed {
                return;
            }
            true
        }
        _ = cancel.cancelled() => false,
    };
    // An interrupted body may have left a partial frame on this retired stream.
    if permit_shutdown {
        tokio::select! {
            biased;
            () = write_shutdown_frames(&mut stream, &options) => {}
            _ = cancel.cancelled() => {}
        }
    }
    update_kiss_status(&options, |status| {
        status.link_state = "closed".to_string();
        status.interface_ready = interface_ready;
        status.pending_depth = pending.len();
    });
}

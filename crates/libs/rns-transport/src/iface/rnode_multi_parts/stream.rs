async fn run_rnode_multi_stream_with_ifac<IO>(
    mut stream: IO,
    options: RNodeMultiStreamOptions,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
    ifac: IfacRuntime,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let wire_mtu = options.mtu.saturating_add(MAX_IFAC_SIZE_BYTES);
    let mut decoder = KissStreamDecoder::new(wire_mtu.max(256));
    let mut read_buffer = vec![0_u8; wire_mtu.max(256)];
    let mut display_capable = false;
    {
        let mut status =
            options.runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
        *status = RNodeMultiRuntimeStatus::from_subinterfaces(&options.subinterfaces);
        status.set_stream_state("probing", None);
    }

    if let Some(probe) = options.startup_probe.as_ref() {
        match run_rnode_multi_startup_probe(
            &mut stream,
            probe,
            &mut decoder,
            &mut read_buffer,
            &cancel,
            &iface_stop,
        )
        .await
        {
            Ok(status) => {
                display_capable = status.has_display();
                options
                    .runtime_status
                    .lock()
                    .expect("rnode multi runtime status mutex poisoned")
                    .set_startup_probe(status.clone());
                log::info!(
                    "RNodeMulti startup probe accepted iface={} device={} firmware={:?} platform={:?} mcu={:?} interfaces={}",
                    options.parent_iface,
                    options.device,
                    status.firmware_version,
                    status.platform,
                    status.mcu,
                    status.interface_summary()
                );
            }
            Err(err) => {
                log::warn!(
                    "RNodeMulti startup probe failed iface={} device={} err={}",
                    options.parent_iface,
                    options.device,
                    err.message
                );
                if err.is_cancel_or_stop() {
                    update_rnode_multi_runtime_state(&options.runtime_status, "closed", None);
                } else {
                    let message = err.message;
                    options
                        .runtime_status
                        .lock()
                        .expect("rnode multi runtime status mutex poisoned")
                        .set_startup_probe(err.status);
                    update_rnode_multi_runtime_state(
                        &options.runtime_status,
                        "probe_failed",
                        Some(message),
                    );
                }
                return;
            }
        }
    }

    for frame in &options.initial_frames {
        if let Err(err) = stream.write_all(frame).await {
            log::warn!(
                "RNodeMulti init write error iface={} device={} err={}",
                options.parent_iface,
                options.device,
                err
            );
            update_rnode_multi_runtime_state(
                &options.runtime_status,
                "init_failed",
                Some(err.to_string()),
            );
            return;
        }
    }
    if let Err(err) = stream.flush().await {
        log::warn!(
            "RNodeMulti init flush error iface={} device={} err={}",
            options.parent_iface,
            options.device,
            err
        );
        update_rnode_multi_runtime_state(
            &options.runtime_status,
            "init_failed",
            Some(err.to_string()),
        );
        return;
    }

    let mut tx_buffer = vec![0_u8; wire_mtu];
    let mut last_read_at = tokio::time::Instant::now();
    let mut first_tx_at: Option<tokio::time::Instant> = None;
    let mut id_tick = tokio::time::interval(Duration::from_millis(250));
    let mut mark_closed_on_exit = false;
    update_rnode_multi_runtime_state(&options.runtime_status, "running", None);

    'stream_loop: loop {
        let mut tx_channel = tx_channel.lock().await;
        let mut management_frame_rx = options.management_frame_rx.lock().await;
        tokio::select! {
            _ = cancel.cancelled() => {
                mark_closed_on_exit = true;
                break;
            }
            _ = iface_stop.cancelled() => {
                mark_closed_on_exit = true;
                break;
            }
            _ = id_tick.tick(), if options.id_beacon.is_some() && first_tx_at.is_some() => {
                let Some(beacon) = options.id_beacon.as_ref() else {
                    continue;
                };
                let Some(first_tx) = first_tx_at else {
                    continue;
                };
                if first_tx.elapsed() >= beacon.interval
                    && write_rnode_multi_id_beacon(&mut stream, &options, beacon).await
                {
                    first_tx_at = None;
                }
            }
            result = stream.read(&mut read_buffer[..]) => {
                match result {
                    Ok(0) => {
                        mark_closed_on_exit = true;
                        break;
                    }
                    Ok(n) => {
                        if decoder.has_partial_frame()
                            && last_read_at.elapsed() >= KISS_READ_FRAME_TIMEOUT
                        {
                            decoder.clear_partial_frame();
                        }
                        last_read_at = tokio::time::Instant::now();
                        match decoder.push_bytes(&read_buffer[..n]) {
                            Ok(frames) => {
                                for frame in frames {
                                    process_rnode_multi_frame(
                                        frame,
                                        &options,
                                        &rx_channel,
                                        &ifac.state,
                                        &ifac.violations,
                                    )
                                    .await;
                                }
                            }
                            Err(err) => log::warn!(
                                "RNodeMulti KISS decode error iface={} device={} err={:?}",
                                options.parent_iface,
                                options.device,
                                err
                            ),
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "RNodeMulti read error iface={} device={} err={}",
                            options.parent_iface,
                            options.device,
                            err
                        );
                        update_rnode_multi_runtime_state(
                            &options.runtime_status,
                            "read_failed",
                            Some(err.to_string()),
                        );
                        break;
                    }
                }
            }
            Some(message) = tx_channel.recv() => {
                let Some(vports) = rnode_multi_tx_vports(&message, &options) else {
                    continue;
                };
                let raw = match encode_packet_ifac(&ifac.state, &message.packet) {
                    Ok(raw) if raw.len() <= tx_buffer.len() => raw,
                    Ok(raw) => {
                        log::warn!(
                            "RNodeMulti IFAC packet exceeds configured MTU iface={} device={} wire_len={} mtu={}",
                            options.parent_iface,
                            options.device,
                            raw.len(),
                            tx_buffer.len()
                        );
                        continue;
                    }
                    Err(err) => {
                        if is_ifac_violation(&err) {
                            record_ifac_violation(&ifac.violations, &err);
                        }
                        log::warn!(
                            "RNodeMulti packet serialize failed iface={} device={} mtu={} error={err:?}",
                            options.parent_iface,
                            options.device,
                            options.mtu
                        );
                        continue;
                    }
                };
                tx_buffer[..raw.len()].copy_from_slice(&raw);
                for vport in vports {
                    if !write_rnode_multi_data(&mut stream, vport, &tx_buffer[..raw.len()]).await {
                        log::warn!(
                            "RNodeMulti data frame write failed iface={} device={} vport={}",
                            options.parent_iface,
                            options.device,
                            vport
                        );
                        update_rnode_multi_runtime_state(
                            &options.runtime_status,
                            "write_failed",
                            Some("data frame write failed".to_string()),
                        );
                        break 'stream_loop;
                    }
                }
                if first_tx_at.is_none() {
                    first_tx_at = Some(tokio::time::Instant::now());
                }
            }
            Some((vport, frame)) = management_frame_rx.recv() => {
                if !write_rnode_multi_management_frame(&mut stream, vport, &frame).await {
                    log::warn!(
                        "RNodeMulti management frame write failed iface={} device={} vport={}",
                        options.parent_iface,
                        options.device,
                        vport
                    );
                    update_rnode_multi_runtime_state(
                        &options.runtime_status,
                        "write_failed",
                        Some("management frame write failed".to_string()),
                    );
                    break;
                }
                if first_tx_at.is_none() {
                    first_tx_at = Some(tokio::time::Instant::now());
                }
            }
        }
    }

    let mut shutdown_error = None;
    if display_capable {
        if let Err(error) = stream.write_all(&rnode_multi_external_framebuffer_frame(false)).await {
            shutdown_error = Some(format!("external framebuffer shutdown write failed: {error}"));
        }
    }
    if shutdown_error.is_none() {
        for frame in &options.shutdown_frames {
            if let Err(error) = stream.write_all(frame).await {
                shutdown_error = Some(format!("shutdown frame write failed: {error}"));
                break;
            }
        }
    }
    if shutdown_error.is_none() {
        if let Err(error) = stream.flush().await {
            shutdown_error = Some(format!("shutdown flush failed: {error}"));
        }
    }
    if let Some(error) = shutdown_error {
        log::warn!(
            "RNodeMulti shutdown error iface={} device={} error={error}",
            options.parent_iface,
            options.device
        );
        update_rnode_multi_runtime_state(&options.runtime_status, "shutdown_failed", Some(error));
    } else if mark_closed_on_exit {
        update_rnode_multi_runtime_state(&options.runtime_status, "closed", None);
    }
}

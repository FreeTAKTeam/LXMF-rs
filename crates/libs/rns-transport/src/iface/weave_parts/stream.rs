async fn flush_weave_stream<IO>(
    stream: &mut IO,
    options: &WeaveStreamOptions,
    operation: &str,
) -> bool
where
    IO: AsyncWrite + Unpin,
{
    if let Err(err) = stream.flush().await {
        update_weave_status(&options.runtime_status, |status| {
            status.mark_reconnecting(format!(
                "weave {operation} flush failed iface={} device={} err={err}",
                options.parent_iface, options.device
            ));
        });
        log::warn!(
            "Weave {operation} flush error iface={} device={} err={err}",
            options.parent_iface,
            options.device
        );
        return false;
    }
    true
}

#[cfg(test)]
pub(crate) async fn run_weave_stream<IO>(
    stream: IO,
    options: WeaveStreamOptions,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
    management_frame_rx: WeaveManagementFrameReceiver,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    run_weave_stream_with_ifac(
        stream,
        options,
        WeaveStreamControl { cancel, iface_stop },
        rx_channel,
        tx_channel,
        management_frame_rx,
        IfacRuntime::from_parts(
            Arc::new(std::sync::RwLock::new(None)),
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        ),
    )
    .await;
}

async fn run_weave_stream_with_ifac<IO>(
    mut stream: IO,
    options: WeaveStreamOptions,
    control: WeaveStreamControl,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
    management_frame_rx: WeaveManagementFrameReceiver,
    ifac: IfacRuntime,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let switch_id = switch_id_for_identity(&options.switch_identity);
    let discover = weave_wire_frame(&weave_wdcl_frame(WDCL_BROADCAST, WDCL_T_DISCOVER, &switch_id));
    if let Err(err) = stream.write_all(&discover).await {
        update_weave_status(&options.runtime_status, |status| {
            status.mark_reconnecting(format!(
                "weave discovery write failed iface={} device={} err={}",
                options.parent_iface, options.device, err
            ));
        });
        log::warn!(
            "Weave discovery write error iface={} device={} err={}",
            options.parent_iface,
            options.device,
            err
        );
        return;
    }
    if !flush_weave_stream(&mut stream, &options, "discovery").await {
        return;
    }
    update_weave_status(&options.runtime_status, |status| {
        status.link_state = WeaveLinkState::Discovering;
        status.bytes_tx = status.bytes_tx.saturating_add(discover.len() as u64);
        status.frames_tx = status.frames_tx.saturating_add(1);
    });

    let state = Arc::new(tokio::sync::Mutex::new(WeaveRuntimeState::default()));
    let mut frame_buffer = Vec::<u8>::with_capacity(options.mtu * 4);
    let mut hdlc_rx_buffer = vec![0_u8; options.mtu.max(DEFAULT_MTU) + MAX_IFAC_SIZE_BYTES + 128];
    let mut read_buffer = vec![0_u8; READ_CAPACITY.max(options.mtu)];
    let mut cleanup_interval = tokio::time::interval(WEAVE_ENDPOINT_CLEANUP_INTERVAL);
    let mut stopped_by_control = false;

    'stream_loop: loop {
        let mut tx_channel = tx_channel.lock().await;
        let mut management_frame_rx = management_frame_rx.lock().await;
        tokio::select! {
            _ = control.cancel.cancelled() => {
                stopped_by_control = true;
                break;
            }
            _ = control.iface_stop.cancelled() => {
                stopped_by_control = true;
                break;
            }
            _ = cleanup_interval.tick() => {
                gc_weave_endpoints(&options, state.clone(), Instant::now()).await;
            }
            result = stream.read(&mut read_buffer[..]) => {
                match result {
                    Ok(0) => {
                        update_weave_status(&options.runtime_status, |status| {
                            status.link_state = WeaveLinkState::Closed;
                            status.wdcl_connected = false;
                        });
                        break;
                    }
                    Ok(n) => {
                        update_weave_status(&options.runtime_status, |status| {
                            status.bytes_rx = status.bytes_rx.saturating_add(n as u64);
                        });
                        frame_buffer.extend_from_slice(&read_buffer[..n]);
                        while let Some((start, end)) = Hdlc::find(&frame_buffer) {
                            let frame = &frame_buffer[start..=end];
                            let mut output = OutputBuffer::new(&mut hdlc_rx_buffer[..]);
                            if Hdlc::decode(frame, &mut output).is_ok() {
                                update_weave_status(&options.runtime_status, |status| {
                                    status.frames_rx = status.frames_rx.saturating_add(1);
                                });
                                if !process_weave_frame(
                                    output.as_slice(),
                                    &options,
                                    state.clone(),
                                    &rx_channel,
                                    &mut stream,
                                    &ifac.state,
                                    &ifac.violations,
                                )
                                .await
                                {
                                    break 'stream_loop;
                                }
                            } else {
                                update_weave_status(&options.runtime_status, |status| {
                                    status.invalid_frames = status.invalid_frames.saturating_add(1);
                                });
                            }
                            frame_buffer.drain(..=end);
                        }
                        if frame_buffer.len() > options.mtu * 64 {
                            frame_buffer.clear();
                        }
                    }
                    Err(err) => {
                        update_weave_status(&options.runtime_status, |status| {
                            status.mark_reconnecting(format!(
                                "weave read failed iface={} device={} err={}",
                                options.parent_iface, options.device, err
                            ));
                        });
                        log::warn!(
                            "Weave read error iface={} device={} err={}",
                            options.parent_iface,
                            options.device,
                            err
                        );
                        break;
                    }
                }
            }
            Some(message) = tx_channel.recv() => {
                let Some(frames) =
                    weave_tx_frames(&message, &options, state.clone(), &ifac.state).await
                else {
                    continue;
                };
                for frame in frames {
                    if let Err(err) = stream.write_all(&frame).await {
                        update_weave_status(&options.runtime_status, |status| {
                            status.mark_reconnecting(format!(
                                "weave write failed iface={} device={} err={}",
                                options.parent_iface, options.device, err
                            ));
                        });
                        log::warn!(
                            "Weave write error iface={} device={} err={}",
                            options.parent_iface,
                            options.device,
                            err
                        );
                        break 'stream_loop;
                    }
                    if !flush_weave_stream(&mut stream, &options, "packet").await {
                        break 'stream_loop;
                    }
                    update_weave_status(&options.runtime_status, |status| {
                        status.bytes_tx = status.bytes_tx.saturating_add(frame.len() as u64);
                        status.frames_tx = status.frames_tx.saturating_add(1);
                    });
                }
            }
            Some(frame) = management_frame_rx.recv() => {
                if let Err(err) = stream.write_all(&frame).await {
                    update_weave_status(&options.runtime_status, |status| {
                        status.mark_reconnecting(format!(
                            "weave management write failed iface={} device={} err={}",
                            options.parent_iface, options.device, err
                        ));
                    });
                    log::warn!(
                        "Weave management write error iface={} device={} err={}",
                        options.parent_iface,
                        options.device,
                        err
                    );
                    break 'stream_loop;
                }
                if !flush_weave_stream(&mut stream, &options, "management frame").await {
                    break 'stream_loop;
                }
                update_weave_status(&options.runtime_status, |status| {
                    status.bytes_tx = status.bytes_tx.saturating_add(frame.len() as u64);
                    status.frames_tx = status.frames_tx.saturating_add(1);
                });
            }
        }
    }
    cleanup_weave_endpoints(&options, state).await;
    if stopped_by_control {
        update_weave_status(&options.runtime_status, |status| {
            status.link_state = WeaveLinkState::Closed;
            status.wdcl_connected = false;
            status.last_error = None;
        });
    }
}

async fn flush_pending_kiss<IO>(
    stream: &mut IO,
    options: &KissStreamOptions,
    interface_ready: &mut bool,
    flow_control_locked_at: &mut Option<Instant>,
    pending: &mut VecDeque<PendingKissPayload>,
    first_tx_at: &mut Option<Instant>,
    last_write_at: &mut Instant,
) where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    while *interface_ready {
        let Some(pending_payload) = pending.pop_front() else {
            break;
        };
        update_kiss_pending_depth(options, pending.len());
        let is_id_beacon = pending_payload.kind == KissDataFrameKind::IdBeacon;
        if write_kiss_payload(
            stream,
            options,
            interface_ready,
            flow_control_locked_at,
            pending_payload.payload,
            pending_payload.kind,
        )
        .await
        {
            *last_write_at = Instant::now();
        }
        if is_id_beacon {
            *first_tx_at = None;
        } else if first_tx_at.is_none() {
            *first_tx_at = Some(Instant::now());
        }
    }
}

async fn write_kiss_payload<IO>(
    stream: &mut IO,
    options: &KissStreamOptions,
    interface_ready: &mut bool,
    flow_control_locked_at: &mut Option<Instant>,
    payload: Vec<u8>,
    kind: KissDataFrameKind,
) -> bool
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let frame = encode_data_frame(&payload);
    if let Err(err) = stream.write_all(&frame).await {
        log::warn!(
            "KISS write error iface={} device={} err={}",
            options.iface_address,
            options.device,
            err
        );
        update_kiss_status(options, |status| {
            status.link_state = "write_error".to_string();
            status.tx_errors = status.tx_errors.saturating_add(1);
            status.last_error = Some(err.to_string());
        });
        return false;
    }
    if let Err(err) = stream.flush().await {
        log::warn!(
            "KISS flush error iface={} device={} err={}",
            options.iface_address,
            options.device,
            err
        );
        update_kiss_status(options, |status| {
            status.link_state = "flush_error".to_string();
            status.tx_errors = status.tx_errors.saturating_add(1);
            status.last_error = Some(err.to_string());
        });
        return false;
    }
    update_kiss_status(options, |status| {
        status.link_state = "running".to_string();
        status.data_frames_tx = status.data_frames_tx.saturating_add(1);
        status.bytes_tx = status.bytes_tx.saturating_add(frame.len() as u64);
        status.interface_ready = !options.flow_control;
        status.last_error = None;
        match kind {
            KissDataFrameKind::Packet => {
                status.packets_tx = status.packets_tx.saturating_add(1);
            }
            KissDataFrameKind::IdBeacon => {
                status.id_beacon_frames_tx = status.id_beacon_frames_tx.saturating_add(1);
            }
        }
    });
    if options.flow_control {
        *interface_ready = false;
        *flow_control_locked_at = Some(Instant::now());
    }
    true
}

async fn write_raw_kiss_frames<IO>(
    stream: &mut IO,
    options: &KissStreamOptions,
    frames: &[Vec<u8>],
    reason: &str,
    kind: KissRawFrameKind,
) -> bool
where
    IO: AsyncWrite + Unpin,
{
    if frames.is_empty() {
        return false;
    }
    for frame in frames {
        if let Err(err) = stream.write_all(frame).await {
            log::warn!(
                "KISS {} write error iface={} device={} err={}",
                reason,
                options.iface_address,
                options.device,
                err
            );
            update_kiss_status(options, |status| {
                status.link_state = "write_error".to_string();
                status.tx_errors = status.tx_errors.saturating_add(1);
                status.last_error = Some(err.to_string());
            });
            return false;
        }
    }
    if let Err(err) = stream.flush().await {
        log::warn!(
            "KISS {} flush error iface={} device={} err={}",
            reason,
            options.iface_address,
            options.device,
            err
        );
        update_kiss_status(options, |status| {
            status.link_state = "flush_error".to_string();
            status.tx_errors = status.tx_errors.saturating_add(1);
            status.last_error = Some(err.to_string());
        });
        return false;
    }
    let frame_count = frames.len() as u64;
    let byte_count = frames.iter().map(Vec::len).sum::<usize>() as u64;
    update_kiss_status(options, |status| {
        status.link_state = "running".to_string();
        status.bytes_tx = status.bytes_tx.saturating_add(byte_count);
        status.last_error = None;
        match kind {
            KissRawFrameKind::Init => {
                status.init_frames_tx = status.init_frames_tx.saturating_add(frame_count);
            }
            KissRawFrameKind::Shutdown => {
                status.shutdown_frames_tx = status.shutdown_frames_tx.saturating_add(frame_count);
            }
            KissRawFrameKind::Management => {
                status.management_frames_tx = status.management_frames_tx.saturating_add(frame_count);
            }
            KissRawFrameKind::Activity => {
                status.activity_frames_tx = status.activity_frames_tx.saturating_add(frame_count);
            }
        }
    });
    true
}

fn update_kiss_status(options: &KissStreamOptions, update: impl FnOnce(&mut KissRuntimeStatus)) {
    if let Some(status) = &options.runtime_status {
        status.update(update);
    }
}

fn update_kiss_pending_depth(options: &KissStreamOptions, pending_depth: usize) {
    update_kiss_status(options, |status| {
        status.pending_depth = pending_depth;
    });
}

fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

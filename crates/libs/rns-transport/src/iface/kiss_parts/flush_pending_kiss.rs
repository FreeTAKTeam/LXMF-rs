async fn flush_pending_kiss<IO>(
    stream: &mut IO,
    options: &KissStreamOptions,
    interface_ready: &mut bool,
    flow_control_locked_at: &mut Option<Instant>,
    pending: &mut VecDeque<Vec<u8>>,
    first_tx_at: &mut Option<Instant>,
    last_write_at: &mut Instant,
) where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    while *interface_ready {
        let Some(payload) = pending.pop_front() else {
            break;
        };
        let is_id_beacon =
            options.id_beacon.as_ref().is_some_and(|beacon| beacon.matches_payload(&payload));
        if write_kiss_payload(stream, options, interface_ready, flow_control_locked_at, payload)
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
        return false;
    }
    if let Err(err) = stream.flush().await {
        log::warn!(
            "KISS flush error iface={} device={} err={}",
            options.iface_address,
            options.device,
            err
        );
        return false;
    }
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
        return false;
    }
    true
}

fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

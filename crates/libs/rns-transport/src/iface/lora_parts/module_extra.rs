async fn run_lora_kiss_stream<IO>(stream: IO, run: LoraStreamRun)
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let stream_cancel = run.cancel.child_token();
    let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
    let (data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
    let probe_status_task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
        run.interface,
        command_rx,
        data_rx,
        stream_cancel.clone(),
        Some(run.startup_response_timeout),
    ));

    let stream = run_kiss_stream_with_ifac(
        stream,
        KissStreamOptions {
            iface_address: run.iface_address,
            device: run.endpoint_label,
            mtu: usize::from(run.config.max_payload_bytes),
            flow_control: run.flow_control,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: run.config.command_frames(),
            shutdown_frames: run.config.shutdown_frames(),
            id_beacon: run.id_beacon,
            activity_probe: run.activity_probe,
            payload_adapter: KissPayloadAdapter::Raw,
            strip_command_port_nibble: false,
            command_tx: Some(command_tx),
            data_rx_tx: Some(data_rx_tx),
            management_frame_rx: Some(run.management_frame_rx),
            runtime_status: None,
        },
        stream_cancel.clone(),
        run.rx_channel,
        run.tx_channel,
        run.ifac_state,
        run.ifac_violations,
    );
    tokio::pin!(stream);
    tokio::select! {
        () = &mut stream => {}
        () = run.iface_stop.cancelled() => {
            stream_cancel.cancel();
            stream.await;
        }
    }
    if let Err(err) = probe_status_task.await {
        if !err.is_cancelled() {
            log::warn!("LoRa probe status task failed iface={} err={}", run.iface_address, err);
        }
    }
}

impl Interface for LoraInterface {
    fn ifac_default_size_bytes() -> usize {
        8
    }

    fn mtu() -> usize {
        220
    }

    fn configured_mtu(&self) -> usize {
        usize::from(self.config.max_payload_bytes)
    }
}

async fn record_probe_status_commands_with_startup_timeout(
    interface: Arc<std::sync::Mutex<LoraInterface>>,
    mut command_rx: tokio::sync::mpsc::Receiver<KissCommandFrame>,
    mut data_rx: tokio::sync::mpsc::Receiver<()>,
    cancel: tokio_util::sync::CancellationToken,
    startup_response_timeout: Option<Duration>,
) {
    if startup_response_timeout.is_some() {
        let mut guard = interface.lock().expect("lora interface mutex poisoned");
        guard.begin_startup_response_collection();
    }
    let mut startup_deadline =
        startup_response_timeout.map(|timeout| Box::pin(tokio::time::sleep(timeout)));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = async {
                if let Some(deadline) = startup_deadline.as_mut() {
                    deadline.as_mut().await;
                }
            }, if startup_deadline.is_some() => {
                startup_deadline = None;
                let result = {
                    let mut guard = interface.lock().expect("lora interface mutex poisoned");
                    match guard.validate_startup_responses() {
                        Ok(()) => Ok(()),
                        Err(err) => {
                            guard.last_command_error = Some(err.clone());
                            Err(err)
                        }
                    }
                };
                match result {
                    Ok(()) => log::debug!("validated LoRa RNode startup responses"),
                    Err(err) => {
                        log::warn!("LoRa RNode startup response validation failed err={}", err);
                        cancel.cancel();
                        break;
                    }
                }
            }
            command = command_rx.recv() => {
                let Some(command) = command else {
                    break;
                };
                let result = {
                    let mut guard = interface.lock().expect("lora interface mutex poisoned");
                    let result = guard.record_command_response(command.command, &command.payload);
                    let fatal = match &result {
                        Ok(_) => false,
                        Err(err) => guard.last_command_error() == Some(err.as_str()),
                    };
                    (result, fatal)
                };
                match result {
                    (Ok(true), _) => log::debug!(
                        "recorded LoRa RNode command response command=0x{:02x}",
                        command.command
                    ),
                    (Ok(false), _) => {}
                    (Err(err), true) => {
                        log::warn!(
                            "fatal LoRa RNode command response command=0x{:02x} err={}",
                            command.command,
                            err
                        );
                        cancel.cancel();
                        break;
                    }
                    (Err(err), false) => log::warn!(
                        "ignored malformed LoRa RNode probe response command=0x{:02x} err={}",
                        command.command,
                        err
                    ),
                }
            }
            data = data_rx.recv() => {
                if data.is_none() {
                    break;
                }
                let mut guard = interface.lock().expect("lora interface mutex poisoned");
                guard.record_inbound_data_frame();
            }
        }
    }
}

fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

fn preflight_tcp_connect(addr: &str) -> Result<(), String> {
    let socket_addr = addr
        .to_socket_addrs()
        .map_err(|err| format!("lora tcp preflight resolve failed addr={addr} err={err}"))?
        .next()
        .ok_or_else(|| format!("lora tcp preflight resolve failed addr={addr}"))?;
    StdTcpStream::connect_timeout(&socket_addr, Duration::from_secs(3))
        .map(|_| ())
        .map_err(|err| format!("lora tcp preflight connect failed addr={addr} err={err}"))
}

#[cfg(test)]
mod tests {
    include!("tests/core.rs");
}

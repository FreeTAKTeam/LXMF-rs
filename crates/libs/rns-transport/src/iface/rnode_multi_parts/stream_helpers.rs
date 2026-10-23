async fn run_rnode_multi_startup_probe<IO>(
    stream: &mut IO,
    probe: &RNodeMultiStartupProbe,
    decoder: &mut KissStreamDecoder,
    read_buffer: &mut [u8],
    cancel: &CancellationToken,
    iface_stop: &CancellationToken,
) -> Result<RNodeMultiProbeStatus, RNodeMultiStartupProbeError>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let mut status = RNodeMultiProbeStatus::default();
    for frame in &probe.frames {
        stream.write_all(frame).await.map_err(|err| {
            RNodeMultiStartupProbeError::new(format!("probe write failed: {err}"), status.clone())
        })?;
    }
    stream.flush().await.map_err(|err| {
        RNodeMultiStartupProbeError::new(format!("probe flush failed: {err}"), status.clone())
    })?;

    let deadline = tokio::time::Instant::now() + probe.timeout;

    loop {
        if status.validate_startup_probe(&probe.required_vports).is_ok() {
            return Ok(status);
        }

        tokio::select! {
            _ = cancel.cancelled() => {
                return Err(RNodeMultiStartupProbeError::new("startup cancelled", status));
            }
            _ = iface_stop.cancelled() => {
                return Err(RNodeMultiStartupProbeError::new("interface stopped", status));
            }
            _ = tokio::time::sleep_until(deadline) => {
                let message = status
                    .validate_startup_probe(&probe.required_vports)
                    .unwrap_err();
                return Err(RNodeMultiStartupProbeError::new(message, status));
            }
            result = stream.read(read_buffer) => {
                let n = result.map_err(|err| {
                    RNodeMultiStartupProbeError::new(
                        format!("probe read failed: {err}"),
                        status.clone(),
                    )
                })?;
                if n == 0 {
                    return Err(RNodeMultiStartupProbeError::new(
                        "probe stream closed",
                        status,
                    ));
                }
                let frames = decoder
                    .push_bytes(&read_buffer[..n])
                    .map_err(|err| {
                        RNodeMultiStartupProbeError::new(
                            format!("probe KISS decode failed: {err:?}"),
                            status.clone(),
                        )
                    })?;
                for frame in frames {
                    if let KissFrame::Command(KissCommand::Unknown(command, payload)) = frame {
                        status.accept_command(command, &payload).map_err(|err| {
                            RNodeMultiStartupProbeError::new(err, status.clone())
                        })?;
                    }
                }
            }
        }
    }
}

async fn process_rnode_multi_frame(
    frame: KissFrame,
    options: &RNodeMultiStreamOptions,
    rx_channel: &tokio::sync::mpsc::Sender<RxMessage>,
    ifac_state: &IfacState,
    ifac_violations: &Arc<std::sync::atomic::AtomicU64>,
) {
    match frame {
        KissFrame::Data(payload) => {
            process_rnode_multi_payload(
                0,
                &payload,
                options,
                rx_channel,
                ifac_state,
                ifac_violations,
            )
            .await;
        }
        KissFrame::Command(KissCommand::Unknown(command, payload)) => {
            if let Some(vport) = rnode_multi_data_command_vport(command) {
                process_rnode_multi_payload(
                    vport,
                    &payload,
                    options,
                    rx_channel,
                    ifac_state,
                    ifac_violations,
                )
                .await;
            } else {
                let mut runtime_status = options
                    .runtime_status
                    .lock()
                    .expect("rnode multi runtime status mutex poisoned");
                match runtime_status.accept_command(command, &payload) {
                    Ok(true) => log::debug!(
                        "RNodeMulti status updated iface={} device={} vport={} command=0x{:02x}",
                        options.parent_iface,
                        options.device,
                        runtime_status.selected_vport,
                        command
                    ),
                    Ok(false) => {}
                    Err(err) => log::warn!(
                        "RNodeMulti status response rejected iface={} device={} vport={} command=0x{:02x} err={}",
                        options.parent_iface,
                        options.device,
                        runtime_status.selected_vport,
                        command,
                        err
                    ),
                }
            }
        }
        KissFrame::Command(KissCommand::Ready) => {}
    }
}

async fn process_rnode_multi_payload(
    vport: u8,
    payload: &[u8],
    options: &RNodeMultiStreamOptions,
    rx_channel: &tokio::sync::mpsc::Sender<RxMessage>,
    ifac_state: &IfacState,
    ifac_violations: &Arc<std::sync::atomic::AtomicU64>,
) {
    let Some(address) = options
        .vport_map
        .iter()
        .find_map(|(address, mapped)| (*mapped == vport).then_some(*address))
    else {
        return;
    };
    match decode_packet_ifac(ifac_state, payload) {
        Ok(packet) => {
            if let Err(err) =
                rx_channel.send(RxMessage { address, packet, source: IfaceSource::None }).await
            {
                log::warn!("failed to enqueue RNodeMulti inbound packet iface={address}: {err}");
            }
        }
        Err(err) => {
            if is_ifac_violation(&err) {
                record_ifac_violation(ifac_violations, &err);
            }
            log::debug!("RNodeMulti inbound packet rejected iface={address} error={err:?}");
        }
    }
}

fn rnode_multi_tx_vports(
    message: &TxMessage,
    options: &RNodeMultiStreamOptions,
) -> Option<Vec<u8>> {
    match message.tx_type {
        TxMessageType::Direct(address) => options.vport_map.get(&address).copied().map(|v| vec![v]),
        TxMessageType::Broadcast(_) => {
            let vports = options
                .subinterfaces
                .iter()
                .filter(|sub| sub.outgoing)
                .map(|sub| sub.vport)
                .collect::<Vec<_>>();
            (!vports.is_empty()).then_some(vports)
        }
    }
}

async fn write_rnode_multi_data<IO>(stream: &mut IO, vport: u8, payload: &[u8]) -> bool
where
    IO: AsyncWrite + Unpin,
{
    let select = encode_command_frame(CMD_SEL_INT, &[vport]);
    let data = encode_command_frame(crate::kiss::CMD_DATA, payload);
    stream.write_all(&select).await.is_ok()
        && stream.write_all(&data).await.is_ok()
        && stream.flush().await.is_ok()
}

async fn write_rnode_multi_management_frame<IO>(stream: &mut IO, vport: u8, frame: &[u8]) -> bool
where
    IO: AsyncWrite + Unpin,
{
    let select = encode_command_frame(CMD_SEL_INT, &[vport]);
    stream.write_all(&select).await.is_ok()
        && stream.write_all(frame).await.is_ok()
        && stream.flush().await.is_ok()
}

async fn write_rnode_multi_id_beacon<IO>(
    stream: &mut IO,
    options: &RNodeMultiStreamOptions,
    beacon: &KissIdBeaconConfig,
) -> bool
where
    IO: AsyncWrite + Unpin,
{
    let payload = beacon.payload();
    let vports = options
        .subinterfaces
        .iter()
        .filter(|subinterface| subinterface.outgoing)
        .map(|subinterface| subinterface.vport)
        .collect::<Vec<_>>();
    if vports.is_empty() {
        return false;
    }
    for vport in vports {
        if !write_rnode_multi_data(stream, vport, &payload).await {
            return false;
        }
    }
    true
}

fn rnode_multi_initial_frames(subinterfaces: &[RNodeMultiSubInterfaceConfig]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    for subinterface in subinterfaces {
        frames.push(encode_command_frame(CMD_SEL_INT, &[subinterface.vport]));
        frames.extend(subinterface.config.command_frames());
        frames.push(encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_ON]));
    }
    frames
}

fn rnode_multi_probe_frames() -> Vec<Vec<u8>> {
    vec![
        encode_command_frame(CMD_DETECT, &[DETECT_REQ]),
        encode_command_frame(CMD_FW_VERSION, &[0x00]),
        encode_command_frame(CMD_PLATFORM, &[0x00]),
        encode_command_frame(CMD_MCU, &[0x00]),
        encode_command_frame(CMD_INTERFACES, &[0x00]),
    ]
}

fn rnode_multi_shutdown_frames(subinterfaces: &[RNodeMultiSubInterfaceConfig]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    for subinterface in subinterfaces {
        frames.push(encode_command_frame(CMD_SEL_INT, &[subinterface.vport]));
        frames.push(encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_OFF]));
        frames.push(encode_command_frame(CMD_LEAVE, &[0xff]));
    }
    frames
}

fn rnode_multi_external_framebuffer_frame(enable: bool) -> Vec<u8> {
    encode_command_frame(CMD_FB_EXT, &[u8::from(enable)])
}

fn rnode_multi_data_command_vport(command: u8) -> Option<u8> {
    match command {
        0x00 => Some(0),
        0x10 => Some(1),
        0x20 => Some(2),
        0x70 => Some(3),
        0x75 => Some(4),
        0x90 => Some(5),
        0xA0 => Some(6),
        0xB0 => Some(7),
        0xC0 => Some(8),
        0xD0 => Some(9),
        0xE0 => Some(10),
        0xF0 => Some(11),
        _ => None,
    }
}

async fn process_weave_frame<IO>(
    frame: &[u8],
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
    rx_channel: &tokio::sync::mpsc::Sender<RxMessage>,
    stream: &mut IO,
    ifac_state: &IfacState,
    ifac_violations: &Arc<std::sync::atomic::AtomicU64>,
) -> bool
where
    IO: AsyncWrite + Unpin,
{
    if frame.len() <= SWITCH_ID_LEN {
        return true;
    }
    let target = switch_id_from_slice(&frame[..SWITCH_ID_LEN]);
    let packet_type = frame[SWITCH_ID_LEN];
    let payload = &frame[SWITCH_ID_LEN + 1..];
    let local_switch_id = switch_id_for_identity(&options.switch_identity);

    match packet_type {
        WDCL_T_DISCOVER if target == local_switch_id => {
            if let Some(remote_switch_id) =
                accept_discovery_response(&options.switch_identity, frame)
            {
                state.lock().await.remote_switch_id = Some(remote_switch_id);
                let handshake = weave_handshake_frame(&options.switch_identity, remote_switch_id);
                update_weave_status(&options.runtime_status, |status| {
                    status.remote_switch_id = Some(remote_switch_id);
                    status.last_error = None;
                });
                let handshake = weave_wire_frame(&handshake);
                match stream.write_all(&handshake).await {
                    Ok(()) => {
                        if !flush_weave_stream(stream, options, "handshake").await {
                            return false;
                        }
                        update_weave_status(&options.runtime_status, |status| {
                            status.bytes_tx =
                                status.bytes_tx.saturating_add(handshake.len() as u64);
                            status.frames_tx = status.frames_tx.saturating_add(1);
                        });
                    }
                    Err(err) => {
                        update_weave_status(&options.runtime_status, |status| {
                            status.mark_reconnecting(format!(
                                "weave handshake write failed iface={} device={} err={}",
                                options.parent_iface, options.device, err
                            ));
                        });
                        log::warn!(
                            "Weave handshake write error iface={} device={} err={}",
                            options.parent_iface,
                            options.device,
                            err
                        );
                        return false;
                    }
                }
            }
        }
        WDCL_T_ENDPOINT_PKT if target == local_switch_id && payload.len() > ENDPOINT_ID_LEN => {
            let data_len = payload.len() - ENDPOINT_ID_LEN;
            let mut endpoint = [0_u8; ENDPOINT_ID_LEN];
            endpoint.copy_from_slice(&payload[data_len..]);
            let address = ensure_weave_endpoint(endpoint, options, state.clone()).await;
            if let Some(address) = address {
                match decode_packet_ifac(ifac_state, &payload[..data_len]) {
                    Ok(packet) => {
                        update_weave_status(&options.runtime_status, |status| {
                            status.mark_endpoint_packet_rx(endpoint, address);
                        });
                        if rx_channel
                            .send(RxMessage { address, packet, source: IfaceSource::None })
                            .await
                            .is_err()
                        {
                            log::warn!(
                                "Weave receive queue closed iface={} device={} endpoint={}",
                                options.parent_iface,
                                options.device,
                                hex::encode(endpoint)
                            );
                            return false;
                        }
                    }
                    Err(err) => {
                        if is_ifac_violation(&err) {
                            record_ifac_violation(ifac_violations, &err);
                        }
                        log::debug!(
                            "Weave endpoint packet rejected iface={} endpoint={} error={err:?}",
                            options.parent_iface,
                            hex::encode(endpoint)
                        );
                    }
                }
            }
        }
        WDCL_T_LOG if target == local_switch_id => {
            process_weave_log(payload, options, state).await;
        }
        WDCL_T_DISP if target == local_switch_id => {
            process_weave_display(payload, options);
        }
        _ => {}
    }
    true
}

fn process_weave_display(payload: &[u8], options: &WeaveStreamOptions) {
    if payload.len() < 9 {
        return;
    }
    let color_format = payload[0];
    let offset = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]) as usize;
    let total_size = u32::from_be_bytes([payload[5], payload[6], payload[7], payload[8]]) as usize;
    let data = &payload[9..];
    update_weave_status(&options.runtime_status, |status| {
        status.mark_display_chunk(color_format, offset, total_size, data);
    });
}

async fn process_weave_log(
    payload: &[u8],
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
) {
    if payload.len() < 8 {
        return;
    }
    let event = u16::from_be_bytes([payload[6], payload[7]]);
    let data = &payload[8..];
    update_weave_status(&options.runtime_status, |status| {
        status.last_log_event = Some(event);
        *status.log_events.entry(event).or_insert(0) += 1;
    });
    match event {
        ET_PROTO_WDCL_CONNECTION => {
            state.lock().await.wdcl_connected = true;
            update_weave_status(&options.runtime_status, |status| {
                status.wdcl_connected = true;
                status.link_state = WeaveLinkState::Connected;
                status.last_error = None;
            });
        }
        ET_PROTO_WDCL_HOST_ENDPOINT if data.len() == ENDPOINT_ID_LEN => {
            let mut endpoint = [0_u8; ENDPOINT_ID_LEN];
            endpoint.copy_from_slice(data);
            state.lock().await.local_endpoint_id = Some(endpoint);
            update_weave_status(&options.runtime_status, |status| {
                status.local_endpoint_id = Some(endpoint);
            });
        }
        ET_PROTO_WEAVE_EP_ALIVE if data.len() == ENDPOINT_ID_LEN => {
            let mut endpoint = [0_u8; ENDPOINT_ID_LEN];
            endpoint.copy_from_slice(data);
            if let Some(address) = ensure_weave_endpoint(endpoint, options, state).await {
                update_weave_status(&options.runtime_status, |status| {
                    status.mark_endpoint_alive(endpoint, address);
                });
            }
        }
        ET_PROTO_WEAVE_EP_VIA if data.len() >= ENDPOINT_ID_LEN => {
            let mut endpoint = [0_u8; ENDPOINT_ID_LEN];
            endpoint.copy_from_slice(&data[..ENDPOINT_ID_LEN]);
            if let Some(address) = ensure_weave_endpoint(endpoint, options, state).await {
                update_weave_status(&options.runtime_status, |status| {
                    status.mark_endpoint_via(endpoint, address);
                });
            }
        }
        ET_STAT_CPU if !data.is_empty() => {
            update_weave_status(&options.runtime_status, |status| {
                status.device_stats.mark_cpu(data[0]);
            });
        }
        ET_STAT_TASK_CPU if data.len() >= 2 => {
            let task = String::from_utf8_lossy(&data[1..]).trim_end_matches('\0').to_string();
            if !task.is_empty() {
                update_weave_status(&options.runtime_status, |status| {
                    status.device_stats.mark_task_cpu(task, data[0]);
                });
            }
        }
        ET_STAT_MEMORY if data.len() >= 8 => {
            let memory_free = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            let memory_total = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            update_weave_status(&options.runtime_status, |status| {
                status.device_stats.mark_memory(memory_free, memory_total);
            });
        }
        _ => {}
    }
}

async fn weave_tx_frames(
    message: &TxMessage,
    _options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
    ifac_state: &IfacState,
) -> Option<Vec<Vec<u8>>> {
    let payload = match encode_packet_ifac(ifac_state, &message.packet) {
        Ok(payload) => payload,
        Err(err) => {
            log::warn!("Weave packet encode failed before transmit: {err:?}");
            return None;
        }
    };
    let mut state = state.lock().await;
    let remote_switch_id = state.remote_switch_id?;
    let endpoints: Vec<[u8; ENDPOINT_ID_LEN]> = match message.tx_type {
        TxMessageType::Direct(address) => match state.addresses.get(&address).copied() {
            Some(endpoint) => {
                if let Some(entry) = state.endpoints.get_mut(&endpoint) {
                    entry.last_seen = Instant::now();
                }
                vec![endpoint]
            }
            None => Vec::new(),
        },
        TxMessageType::Broadcast(_) => {
            let endpoints: Vec<[u8; ENDPOINT_ID_LEN]> = state.endpoints.keys().copied().collect();
            let now = Instant::now();
            for endpoint in &endpoints {
                if let Some(entry) = state.endpoints.get_mut(endpoint) {
                    entry.last_seen = now;
                }
            }
            endpoints
        }
    };
    drop(state);

    let mut frames = Vec::new();
    for endpoint in endpoints {
        frames.push(weave_wire_frame(&weave_endpoint_command_frame(
            remote_switch_id,
            endpoint,
            &payload,
        )));
    }
    (!frames.is_empty()).then_some(frames)
}

async fn ensure_weave_endpoint(
    endpoint: [u8; ENDPOINT_ID_LEN],
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
) -> Option<AddressHash> {
    {
        let mut state = state.lock().await;
        if let Some(entry) = state.endpoints.get_mut(&endpoint) {
            entry.last_seen = Instant::now();
            return Some(entry.iface);
        }
    }
    let mut manager = options.iface_manager.lock().await;
    let address =
        manager.register_virtual_iface(options.parent_iface, IfaceRole::VirtualUnicast)?;
    drop(manager);

    let mut state = state.lock().await;
    state
        .endpoints
        .insert(endpoint, WeaveEndpointState { iface: address, last_seen: Instant::now() });
    state.addresses.insert(address, endpoint);
    Some(address)
}

async fn gc_weave_endpoints(
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
    now: Instant,
) {
    let stale = {
        let mut state = state.lock().await;
        let stale: Vec<([u8; ENDPOINT_ID_LEN], AddressHash)> = state
            .endpoints
            .iter()
            .filter(|(_, endpoint)| {
                now.duration_since(endpoint.last_seen) >= WEAVE_ENDPOINT_IDLE_TIMEOUT
            })
            .map(|(endpoint_id, endpoint)| (*endpoint_id, endpoint.iface))
            .collect();
        for (endpoint_id, iface) in &stale {
            state.endpoints.remove(endpoint_id);
            state.addresses.remove(iface);
        }
        stale
    };

    if stale.is_empty() {
        return;
    }

    for (_, iface) in &stale {
        if !options.iface_manager.lock().await.stop_interface(*iface) {
            log::debug!(
                "Weave stale endpoint interface already absent parent={} iface={iface}",
                options.parent_iface
            );
        }
    }
    update_weave_status(&options.runtime_status, |status| {
        for (endpoint_id, _) in &stale {
            status.remove_endpoint(endpoint_id);
        }
    });
}

async fn cleanup_weave_endpoints(
    options: &WeaveStreamOptions,
    state: Arc<tokio::sync::Mutex<WeaveRuntimeState>>,
) {
    // Registration and GC may be cancelled between native and private-index commits.
    {
        let mut manager = options.iface_manager.lock().await;
        let children: Vec<_> = manager
            .interface_tree_hashes(options.parent_iface)
            .into_iter()
            .filter(|iface| *iface != options.parent_iface)
            .collect();
        for iface in children {
            if !manager.stop_interface(iface) {
                log::debug!(
                    "Weave endpoint cleanup interface already absent parent={} iface={iface}",
                    options.parent_iface
                );
            }
        }
    }
    {
        let mut state = state.lock().await;
        state.endpoints.clear();
        state.addresses.clear();
    }
    update_weave_status(&options.runtime_status, WeaveRuntimeStatus::clear_endpoints);
}

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_i2p_accept_loop_with_ifac(
    parent_iface: AddressHash,
    name: String,
    sam_addr: String,
    state_path: Option<PathBuf>,
    transport_identity_hash: Option<[u8; 16]>,
    mtu: usize,
    reconnect_wait: Duration,
    runtime_status: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    peer_routes: Arc<
        tokio::sync::Mutex<BTreeMap<AddressHash, tokio::sync::mpsc::Sender<TxMessage>>>,
    >,
    ifac_state: IfacState,
    ifac_violations: Arc<std::sync::atomic::AtomicU64>,
) {
    let session_id =
        format!("{}-accept", sam_session_id(parent_iface, transport_identity_hash.as_ref()));
    // SAM session recreation must not retire already accepted peer streams.
    let mut streams = tokio::task::JoinSet::new();

    loop {
        if cancel.is_cancelled() || iface_stop.is_cancelled() {
            break;
        }

        while let Some(result) = streams.try_join_next() {
            if let Err(error) = result {
                log::error!("I2P accepted worker failed iface={parent_iface} error={error}");
            }
        }
        let destination_result = tokio::select! {
            _ = cancel.cancelled() => break,
            _ = iface_stop.cancelled() => break,
            result = connectable_session_destination_with_identity(
                sam_addr.as_str(), &name, state_path.as_ref(), transport_identity_hash.as_ref(),
            ) => result,
        };
        let session_destination = match destination_result {
            Ok(destination) => destination,
            Err(err) => {
                runtime_status
                    .lock()
                    .expect("i2p runtime status mutex poisoned")
                    .mark_accept_reconnecting(err.to_string());
                log::warn!(
                    "failed to prepare I2P connectable destination sam={} iface={} err={}",
                    sam_addr,
                    parent_iface,
                    err
                );
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = tokio::time::sleep(reconnect_wait) => {}
                }
                continue;
            }
        };

        let session_result = tokio::select! {
            _ = cancel.cancelled() => break,
            _ = iface_stop.cancelled() => break,
            result = create_sam_session(sam_addr.as_str(), session_id.as_str(), &session_destination) => result,
        };
        let (session_stream, _destination) = match session_result {
            Ok(session) => session,
            Err(err) => {
                runtime_status
                    .lock()
                    .expect("i2p runtime status mutex poisoned")
                    .mark_accept_reconnecting(err.to_string());
                log::warn!(
                    "failed to create I2P SAM accept session sam={} iface={} err={}",
                    sam_addr,
                    parent_iface,
                    err
                );
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = tokio::time::sleep(reconnect_wait) => {}
                }
                continue;
            }
        };

        runtime_status.lock().expect("i2p runtime status mutex poisoned").mark_accept_listening();
        log::info!("I2P SAM accept session ready iface={} sam={}", parent_iface, sam_addr);
        run_i2p_accept_session(
            session_stream,
            session_id.as_str(),
            sam_addr.as_str(),
            parent_iface,
            mtu,
            reconnect_wait,
            runtime_status.clone(),
            cancel.clone(),
            iface_stop.clone(),
            rx_channel.clone(),
            iface_manager.clone(),
            peer_routes.clone(),
            ifac_state.clone(),
            ifac_violations.clone(),
            &mut streams,
        )
        .await;
    }
    while let Some(result) = streams.join_next().await {
        if let Err(error) = result {
            log::error!(
                "I2P accepted worker failed during shutdown iface={parent_iface} error={error}"
            );
        }
    }
    runtime_status.lock().expect("i2p runtime status mutex poisoned").mark_accept_closed();
}

#[allow(clippy::too_many_arguments)]
async fn run_i2p_accept_session(
    _session_stream: TcpStream,
    session_id: &str,
    sam_addr: &str,
    parent_iface: AddressHash,
    mtu: usize,
    reconnect_wait: Duration,
    runtime_status: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    peer_routes: Arc<
        tokio::sync::Mutex<BTreeMap<AddressHash, tokio::sync::mpsc::Sender<TxMessage>>>,
    >,
    ifac_state: IfacState,
    ifac_violations: Arc<std::sync::atomic::AtomicU64>,
    streams: &mut tokio::task::JoinSet<()>,
) {
    loop {
        if cancel.is_cancelled() || iface_stop.is_cancelled() {
            break;
        }

        let accepted = accept_sam_stream(sam_addr, session_id);
        tokio::pin!(accepted);
        let accept_result = loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = iface_stop.cancelled() => return,
                Some(result) = streams.join_next(), if !streams.is_empty() => {
                    if let Err(error) = result {
                        log::error!("I2P accepted worker failed iface={parent_iface} error={error}");
                    }
                }
                result = &mut accepted => break result,
            }
        };
        let (remote_destination, stream) = match accept_result {
            Ok(accepted) => accepted,
            Err(err) => {
                let recreate_session = accept_error_requires_session_recreate(&err);
                runtime_status
                    .lock()
                    .expect("i2p runtime status mutex poisoned")
                    .mark_accept_reconnecting(err.to_string());
                log::warn!(
                    "failed to accept I2P SAM stream sam={} iface={} err={}",
                    sam_addr,
                    parent_iface,
                    err
                );
                if recreate_session {
                    break;
                }
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = iface_stop.cancelled() => break,
                    _ = tokio::time::sleep(reconnect_wait) => {}
                }
                continue;
            }
        };

        let Some(child_iface) = iface_manager
            .lock()
            .await
            .register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
        else {
            log::warn!("failed to register incoming I2P virtual iface for {}", remote_destination);
            continue;
        };
        let (peer_tx, peer_rx) = tokio::sync::mpsc::channel(128);
        peer_routes.lock().await.insert(child_iface, peer_tx);
        runtime_status
            .lock()
            .expect("i2p runtime status mutex poisoned")
            .mark_incoming_connected(&remote_destination, child_iface);

        log::info!(
            "I2P SAM incoming stream accepted peer={} iface={}",
            remote_destination,
            child_iface
        );
        streams.spawn(run_i2p_accepted_stream_with_ifac(
            remote_destination,
            child_iface,
            mtu,
            runtime_status.clone(),
            cancel.clone(),
            iface_stop.clone(),
            rx_channel.clone(),
            peer_routes.clone(),
            iface_manager.clone(),
            stream,
            peer_rx,
            ifac_state.clone(),
            ifac_violations.clone(),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_i2p_accepted_stream_with_ifac(
    remote_destination: String,
    child_iface: AddressHash,
    mtu: usize,
    runtime_status: Arc<std::sync::Mutex<I2pRuntimeStatus>>,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    peer_routes: Arc<
        tokio::sync::Mutex<BTreeMap<AddressHash, tokio::sync::mpsc::Sender<TxMessage>>>,
    >,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    stream: TcpStream,
    peer_rx: tokio::sync::mpsc::Receiver<TxMessage>,
    ifac_state: IfacState,
    ifac_violations: Arc<std::sync::atomic::AtomicU64>,
) {
    let peer_rx = Arc::new(tokio::sync::Mutex::new(peer_rx));
    let status_key = format!("incoming:{child_iface}");
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(HDLC_STREAM_EVENT_CHANNEL_CAPACITY);
    let status_task =
        tokio::spawn(track_i2p_stream_events(status_key, runtime_status.clone(), event_rx));
    let (read_stream, write_stream) = stream.into_split();
    run_hdlc_stream_with_runtime_and_ifac(
        "i2p_accept".to_string(),
        child_iface,
        mtu,
        cancel,
        iface_stop,
        rx_channel,
        peer_rx,
        ifac_state,
        ifac_violations,
        read_stream,
        write_stream,
        i2p_hdlc_runtime(event_tx),
    )
    .await;
    if let Err(error) = status_task.await {
        log::error!(
            "I2P incoming status task failed peer={} iface={} error={error}",
            remote_destination,
            child_iface
        );
    }
    runtime_status
        .lock()
        .expect("i2p runtime status mutex poisoned")
        .mark_incoming_closed(child_iface);
    peer_routes.lock().await.remove(&child_iface);
    if !iface_manager.lock().await.stop_interface(child_iface) {
        log::debug!("I2P incoming child interface already absent iface={child_iface}");
    }
    log::info!("I2P SAM incoming stream closed peer={} iface={}", remote_destination, child_iface);
}

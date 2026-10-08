use super::*;

pub(crate) async fn run_zmq_rpc_loop_until(
    config: ZmqRpcLoopConfig,
    daemon: Arc<RpcDaemon>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    validate_zmq_loop_config(&config, daemon.as_ref())?;
    let command_endpoint_requires_auth =
        config.require_auth_for_remote && !is_local_zmq_endpoint(&config.command_endpoint);
    let mut commands = PullSocket::new();
    commands.bind(config.command_endpoint.as_str()).await.map_err(zmq_io_error)?;
    let (response_tx, response_rx) =
        mpsc::channel::<ZmqOutboundResponse>(ZMQ_RPC_RESPONSE_QUEUE_CAPACITY);
    let (writer_shutdown_tx, writer_shutdown_rx) = watch::channel(false);
    let metrics = daemon.zmq_pipeline_metrics();
    let response_writer = tokio::spawn(run_zmq_response_writer(
        response_rx,
        writer_shutdown_rx,
        Arc::clone(&metrics),
    ));
    let rpc_permits = Arc::new(Semaphore::new(ZMQ_RPC_WORKER_CONCURRENCY));
    let mut rpc_workers = JoinSet::new();
    log::info!("reticulumd listening on zmq {}", config.command_endpoint);

    let loop_result = loop {
        if *shutdown.borrow() {
            break Ok(());
        }
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break Ok(());
                }
            }
            completed = rpc_workers.join_next(), if !rpc_workers.is_empty() => {
                if let Some(Err(error)) = completed {
                    log::error!("[daemon] zmq rpc worker task failed: {error}");
                }
            }
            message = commands.recv(), if rpc_workers.len() < ZMQ_RPC_WORKER_CONCURRENCY => {
                let message = match message {
                    Ok(message) => message,
                    Err(err) if is_recoverable_zmq_transport_error(&err) => {
                        log::warn!("[daemon] zmq rpc receive dropped client connection: {}", err);
                        continue;
                    }
                    Err(err) => break Err(zmq_io_error(err)),
                };
                let input_bytes = message.iter().map(|frame| frame.len()).sum();
                let dispatch_wait = metrics.enter(ZmqStage::DispatchWait, input_bytes);
                let daemon = Arc::clone(&daemon);
                let response_tx = response_tx.clone();
                let rpc_permits = Arc::clone(&rpc_permits);
                rpc_workers.spawn(async move {
                    let Ok(_permit) = rpc_permits.acquire_owned().await else {
                        return;
                    };
                    // Management requests can wait for the async interface worker.
                    let response = tokio::task::spawn_blocking(move || {
                        dispatch_with_metrics(daemon.as_ref(), message, command_endpoint_requires_auth, input_bytes, dispatch_wait)
                    }).await;
                    if let Ok(Ok(response)) = response {
                        if response_tx.send(response).await.is_err() {
                            log::warn!("[daemon] zmq rpc response writer stopped");
                        }
                    } else if let Err(error) = response {
                        log::error!("[daemon] zmq rpc dispatch task failed: {error}");
                    }
                });
            }
        }
    };
    rpc_permits.close();
    drop(response_tx);
    if writer_shutdown_tx.send(true).is_err() {
        log::warn!("[daemon] zmq rpc response writer stopped before shutdown");
    }
    // Dispatch jobs own synchronous mutations until they finish. Cancelling only
    // response sockets must not detach an in-progress spawn_blocking mutation.
    while let Some(completed) = rpc_workers.join_next().await {
        if let Err(error) = completed {
            log::error!("[daemon] zmq rpc shutdown worker failed: {error}");
        }
    }
    response_writer
        .await
        .map_err(|err| io::Error::other(format!("zmq response writer task failed: {err}")))?;
    loop_result
}

// Attach response ownership before returning from the blocking task. Tokio can
// retain this completed output even while its awaiting worker is unscheduled.
pub(super) fn dispatch_with_metrics(
    daemon: &RpcDaemon,
    message: ZmqMessage,
    requires_auth: bool,
    input_bytes: usize,
    dispatch_wait: ZmqStageGuard,
) -> Result<ZmqOutboundResponse, &'static str> {
    let metrics = daemon.zmq_pipeline_metrics();
    let handler = metrics.enter(ZmqStage::Handler, input_bytes);
    dispatch_wait.finish(ZmqStageOutcome::Succeeded);
    let mut result = handle_zmq_command_message(daemon, message, requires_auth);
    if let Ok(response) = &mut result {
        response.queue_stage =
            Some(metrics.enter(ZmqStage::ResponseQueue, response.owned_wire_bytes()));
    }
    handler.finish(if result.is_ok() {
        ZmqStageOutcome::Succeeded
    } else {
        ZmqStageOutcome::Failed
    });
    result
}

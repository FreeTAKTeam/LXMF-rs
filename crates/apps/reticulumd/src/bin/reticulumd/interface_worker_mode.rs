use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::interfaces::ble;
use crate::Args;
use reticulum_daemon::config::InterfaceConfig;
use rns_transport::hash::AddressHash;
use rns_transport::iface::{
    serial::SerialInterface, tcp_client::TcpClient, tcp_server::TcpServer, udp::UdpInterface,
    IfaceRole, InterfaceChannel, InterfaceContext, InterfaceManager, InterfaceMode,
    InterfaceRxSender, InterfaceTxReceiver, RxMessage, TxMessageType,
};
use rns_transport::transport::interface_boundary::{
    read_interface_worker_envelope, serve_interface_worker_envelopes,
    write_interface_worker_envelope, InterfaceWorkerEnvelope, InterfaceWorkerEvent,
    InterfaceWorkerServeStopReason, InterfaceWorkerServeSummary,
};
use rns_transport::transport::worker_boundary::WorkerCodecError;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const INTERFACE_WORKER_TX_QUEUE_CAPACITY: usize = 128;
pub(super) const DEFAULT_INTERFACE_WORKER_RESTART_BACKOFF_MS: u64 = 100;

#[allow(dead_code)]
pub(super) struct InterfaceWorkerStdioProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
}

#[allow(dead_code)]
impl InterfaceWorkerStdioProcess {
    pub(super) fn spawn(executable: impl AsRef<Path>) -> Result<Self, InterfaceWorkerProcessError> {
        Self::spawn_with_args(executable, std::iter::empty::<String>())
    }

    pub(super) fn spawn_with_args(
        executable: impl AsRef<Path>,
        args: impl IntoIterator<Item = String>,
    ) -> Result<Self, InterfaceWorkerProcessError> {
        let executable = executable.as_ref();
        let mut command = Command::new(executable);
        command.arg("--interface-worker-stdio").args(args);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| InterfaceWorkerProcessError::Spawn {
                executable: executable.to_path_buf(),
                message: err.to_string(),
            })?;
        let stdin =
            child.stdin.take().ok_or(InterfaceWorkerProcessError::MissingPipe { name: "stdin" })?;
        let stdout = child
            .stdout
            .take()
            .ok_or(InterfaceWorkerProcessError::MissingPipe { name: "stdout" })?;
        Ok(Self { child, stdin: Some(stdin), stdout: Some(stdout) })
    }

    pub(super) async fn send(
        &mut self,
        envelope: &InterfaceWorkerEnvelope,
    ) -> Result<(), InterfaceWorkerProcessError> {
        let stdin =
            self.stdin.as_mut().ok_or(InterfaceWorkerProcessError::ClosedPipe { name: "stdin" })?;
        write_interface_worker_envelope(stdin, envelope)
            .await
            .map_err(InterfaceWorkerProcessError::Write)
    }

    pub(super) async fn recv(
        &mut self,
    ) -> Result<InterfaceWorkerEnvelope, InterfaceWorkerProcessError> {
        let stdout = self
            .stdout
            .as_mut()
            .ok_or(InterfaceWorkerProcessError::ClosedPipe { name: "stdout" })?;
        read_interface_worker_envelope(stdout).await.map_err(InterfaceWorkerProcessError::Read)
    }

    pub(super) async fn shutdown(
        mut self,
        wait: Duration,
    ) -> Result<ExitStatus, InterfaceWorkerProcessError> {
        if let Some(stdin) = self.stdin.as_mut() {
            write_interface_worker_envelope(
                stdin,
                &InterfaceWorkerEnvelope::new(0, InterfaceWorkerEvent::Shutdown),
            )
            .await
            .map_err(InterfaceWorkerProcessError::Write)?;
        }
        drop(self.stdin.take());
        timeout(wait, self.child.wait())
            .await
            .map_err(|_| InterfaceWorkerProcessError::ShutdownTimedOut)?
            .map_err(|err| InterfaceWorkerProcessError::Wait { message: err.to_string() })
    }

    pub(super) async fn run_channel_bridge(
        self,
        tx_receiver: &mut InterfaceTxReceiver,
        rx_sender: &InterfaceRxSender,
        shutdown_wait: Duration,
    ) -> Result<InterfaceWorkerBridgeSummary, InterfaceWorkerProcessError> {
        self.run_channel_bridge_inner(tx_receiver, rx_sender, shutdown_wait, None, None).await
    }

    pub(super) async fn run_channel_bridge_until_cancelled(
        self,
        tx_receiver: &mut InterfaceTxReceiver,
        rx_sender: &InterfaceRxSender,
        shutdown_wait: Duration,
        cancellation: CancellationToken,
    ) -> Result<InterfaceWorkerBridgeSummary, InterfaceWorkerProcessError> {
        self.run_channel_bridge_inner(
            tx_receiver,
            rx_sender,
            shutdown_wait,
            Some(cancellation),
            None,
        )
        .await
    }

    pub(super) async fn run_channel_bridge_until_cancelled_with_aliases(
        self,
        tx_receiver: &mut InterfaceTxReceiver,
        rx_sender: &InterfaceRxSender,
        shutdown_wait: Duration,
        cancellation: CancellationToken,
        alias_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        host_address: AddressHash,
    ) -> Result<InterfaceWorkerBridgeSummary, InterfaceWorkerProcessError> {
        self.run_channel_bridge_inner(
            tx_receiver,
            rx_sender,
            shutdown_wait,
            Some(cancellation),
            Some((alias_manager, host_address)),
        )
        .await
    }

    async fn run_channel_bridge_inner(
        mut self,
        tx_receiver: &mut InterfaceTxReceiver,
        rx_sender: &InterfaceRxSender,
        shutdown_wait: Duration,
        cancellation: Option<CancellationToken>,
        alias_registration: Option<(Arc<tokio::sync::Mutex<InterfaceManager>>, AddressHash)>,
    ) -> Result<InterfaceWorkerBridgeSummary, InterfaceWorkerProcessError> {
        let mut sequence = 0u64;
        let mut sent = 0usize;
        let mut received = 0usize;
        let cancellation = cancellation.unwrap_or_default();
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    let _status = self.shutdown(shutdown_wait).await?;
                    return Ok(InterfaceWorkerBridgeSummary {
                        sent,
                        received,
                        stop_reason: InterfaceWorkerBridgeStopReason::Cancelled,
                    });
                }
                message = tx_receiver.recv() => {
                    let Some(message) = message else {
                        let _status = self.shutdown(shutdown_wait).await?;
                        return Ok(InterfaceWorkerBridgeSummary {
                            sent,
                            received,
                            stop_reason: InterfaceWorkerBridgeStopReason::TransportTxClosed,
                        });
                    };
                    let envelope = InterfaceWorkerEnvelope::outbound_from_tx_message(sequence, &message)
                        .map_err(InterfaceWorkerProcessError::Write)?;
                    sequence = sequence.saturating_add(1);
                    self.send(&envelope).await?;
                    sent = sent.saturating_add(1);
                }
                result = self.recv() => {
                    let envelope = match result {
                        Ok(envelope) => envelope,
                        Err(InterfaceWorkerProcessError::Read(WorkerCodecError::Io { message }))
                            if is_eof_io(&message) =>
                        {
                            return Ok(InterfaceWorkerBridgeSummary {
                                sent,
                                received,
                                stop_reason: InterfaceWorkerBridgeStopReason::ChildEof,
                            });
                        }
                        Err(err) => return Err(err),
                    };
                    if matches!(envelope.event, InterfaceWorkerEvent::Shutdown) {
                        return Ok(InterfaceWorkerBridgeSummary {
                            sent,
                            received,
                            stop_reason: InterfaceWorkerBridgeStopReason::ChildShutdown,
                        });
                    }
                    if let Some(message) = envelope.event.to_rx_message().map_err(InterfaceWorkerProcessError::Read)? {
                        if let Some((alias_manager, host_address)) = alias_registration.as_ref() {
                            if message.address != *host_address
                                && message.address != AddressHash::new_empty()
                            {
                                alias_manager.lock().await.register_remote_iface_alias(
                                    *host_address,
                                    message.address,
                                    IfaceRole::Unicast,
                                    InterfaceMode::Full,
                                );
                            }
                        }
                        let _ = rx_sender.try_send(message);
                    }
                    received = received.saturating_add(1);
                }
            }
        }
    }
}

#[allow(dead_code)]
pub(super) struct InterfaceWorkerBridgeHandle {
    pub(super) address: AddressHash,
    pub(super) cancellation: CancellationToken,
    pub(super) metrics: Arc<InterfaceWorkerBridgeMetrics>,
    pub(super) task:
        Option<JoinHandle<Result<InterfaceWorkerBridgeSummary, InterfaceWorkerProcessError>>>,
}

#[allow(dead_code)]
#[derive(Default)]
pub(super) struct InterfaceWorkerBridgeMetrics {
    child_restarts: AtomicUsize,
    child_errors: AtomicUsize,
}

#[allow(dead_code)]
pub(super) async fn spawn_interface_worker_bridge(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    executable: impl AsRef<Path>,
    role: IfaceRole,
    mode: InterfaceMode,
    shutdown_wait: Duration,
    restart_backoff: Duration,
    cancellation: CancellationToken,
) -> Result<InterfaceWorkerBridgeHandle, InterfaceWorkerProcessError> {
    spawn_interface_worker_bridge_with_args(
        iface_manager,
        executable,
        std::iter::empty::<String>(),
        role,
        mode,
        shutdown_wait,
        restart_backoff,
        cancellation,
    )
    .await
}

#[allow(dead_code)]
pub(super) async fn spawn_interface_worker_bridge_with_args(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    executable: impl AsRef<Path>,
    child_args: impl IntoIterator<Item = String>,
    role: IfaceRole,
    mode: InterfaceMode,
    shutdown_wait: Duration,
    restart_backoff: Duration,
    cancellation: CancellationToken,
) -> Result<InterfaceWorkerBridgeHandle, InterfaceWorkerProcessError> {
    let channel = iface_manager.lock().await.new_channel_with_role_and_mode(
        INTERFACE_WORKER_TX_QUEUE_CAPACITY,
        role,
        mode,
    );
    let address = channel.address;
    let (rx_sender, mut tx_receiver) = channel.split();
    let mut child_args = child_args.into_iter().collect::<Vec<_>>();
    child_args.push("--interface-worker-address".to_string());
    child_args.push(hex::encode(address.as_slice()));
    let executable = executable.as_ref().to_path_buf();
    let worker = InterfaceWorkerStdioProcess::spawn_with_args(&executable, child_args.clone())?;
    let bridge_cancellation = cancellation.clone();
    let metrics = Arc::new(InterfaceWorkerBridgeMetrics::default());
    let bridge_metrics = metrics.clone();
    let task = tokio::spawn(async move {
        run_supervised_interface_worker_bridge(
            worker,
            executable,
            child_args,
            &mut tx_receiver,
            &rx_sender,
            shutdown_wait,
            restart_backoff,
            bridge_cancellation,
            bridge_metrics,
            iface_manager.clone(),
            address,
        )
        .await
    });

    Ok(InterfaceWorkerBridgeHandle { address, cancellation, metrics, task: Some(task) })
}

async fn run_supervised_interface_worker_bridge(
    mut worker: InterfaceWorkerStdioProcess,
    executable: PathBuf,
    child_args: Vec<String>,
    tx_receiver: &mut InterfaceTxReceiver,
    rx_sender: &InterfaceRxSender,
    shutdown_wait: Duration,
    restart_backoff: Duration,
    cancellation: CancellationToken,
    metrics: Arc<InterfaceWorkerBridgeMetrics>,
    alias_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    host_address: AddressHash,
) -> Result<InterfaceWorkerBridgeSummary, InterfaceWorkerProcessError> {
    let restart_backoff = restart_backoff.max(Duration::from_millis(1));
    let mut total_sent = 0usize;
    let mut total_received = 0usize;
    loop {
        let summary = worker
            .run_channel_bridge_until_cancelled_with_aliases(
                tx_receiver,
                rx_sender,
                shutdown_wait,
                cancellation.clone(),
                alias_manager.clone(),
                host_address,
            )
            .await;
        match summary {
            Ok(summary) => {
                total_sent = total_sent.saturating_add(summary.sent);
                total_received = total_received.saturating_add(summary.received);
                match summary.stop_reason {
                    InterfaceWorkerBridgeStopReason::Cancelled
                    | InterfaceWorkerBridgeStopReason::TransportTxClosed => {
                        return Ok(InterfaceWorkerBridgeSummary {
                            sent: total_sent,
                            received: total_received,
                            stop_reason: summary.stop_reason,
                        });
                    }
                    InterfaceWorkerBridgeStopReason::ChildEof
                    | InterfaceWorkerBridgeStopReason::ChildShutdown => {
                        metrics.child_restarts.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Err(err @ InterfaceWorkerProcessError::ChannelClosed { .. }) => return Err(err),
            Err(err) if cancellation.is_cancelled() => return Err(err),
            Err(_) => {
                metrics.child_errors.fetch_add(1, Ordering::Relaxed);
                metrics.child_restarts.fetch_add(1, Ordering::Relaxed);
            }
        }

        tokio::select! {
            _ = cancellation.cancelled() => {
                return Ok(InterfaceWorkerBridgeSummary {
                    sent: total_sent,
                    received: total_received,
                    stop_reason: InterfaceWorkerBridgeStopReason::Cancelled,
                });
            }
            _ = tokio::time::sleep(restart_backoff) => {}
        }
        worker = match InterfaceWorkerStdioProcess::spawn_with_args(&executable, child_args.clone())
        {
            Ok(worker) => worker,
            Err(err) => {
                metrics.child_errors.fetch_add(1, Ordering::Relaxed);
                return Err(err);
            }
        };
    }
}

impl Drop for InterfaceWorkerBridgeHandle {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl InterfaceWorkerBridgeHandle {
    pub(super) fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub(super) fn child_restarts(&self) -> usize {
        self.metrics.child_restarts.load(Ordering::Relaxed)
    }

    pub(super) fn child_errors(&self) -> usize {
        self.metrics.child_errors.load(Ordering::Relaxed)
    }
}

impl Drop for InterfaceWorkerStdioProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(super) enum InterfaceWorkerProcessError {
    Spawn { executable: PathBuf, message: String },
    MissingPipe { name: &'static str },
    ClosedPipe { name: &'static str },
    Write(WorkerCodecError),
    Read(WorkerCodecError),
    ChannelClosed { message: String },
    Wait { message: String },
    ShutdownTimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum InterfaceWorkerBridgeStopReason {
    TransportTxClosed,
    ChildEof,
    ChildShutdown,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) struct InterfaceWorkerBridgeSummary {
    pub(super) sent: usize,
    pub(super) received: usize,
    pub(super) stop_reason: InterfaceWorkerBridgeStopReason,
}

pub(super) async fn run_interface_worker_stdio(args: &Args) {
    let address = args
        .interface_worker_address
        .as_deref()
        .and_then(|value| AddressHash::new_from_hex_string(value).ok())
        .unwrap_or_else(AddressHash::new_empty);
    if let Some(bind_addr) = args.interface_worker_udp_bind.as_ref() {
        if let Err(err) = run_udp_interface_worker_stdio(
            address,
            bind_addr.clone(),
            args.interface_worker_udp_forward.clone(),
        )
        .await
        {
            eprintln!("[interface-worker-stdio] udp stopped err={err:?}");
        }
        return;
    }
    if let Some(connect_addr) = args.interface_worker_tcp_connect.as_ref() {
        run_tcp_client_interface_worker_stdio(address, connect_addr.clone()).await;
        return;
    }
    if let Some(listen_addr) = args.interface_worker_tcp_listen.as_ref() {
        run_tcp_server_interface_worker_stdio(address, listen_addr.clone()).await;
        return;
    }
    if let Some(device) = args.interface_worker_serial_device.as_ref() {
        let Some(baud_rate) = args.interface_worker_serial_baud_rate else {
            eprintln!("[interface-worker-stdio] serial stopped err=missing baud rate");
            return;
        };
        match serial_interface_from_worker_args(args, device.clone(), baud_rate) {
            Ok(adapter) => {
                run_serial_interface_worker_stdio(address, adapter).await;
            }
            Err(err) => {
                eprintln!("[interface-worker-stdio] serial stopped err={err}");
            }
        }
        return;
    }
    if args.interface_worker_ble_peripheral_id.is_some() {
        match ble_interface_config_from_worker_args(args) {
            Ok(config) => {
                run_ble_interface_worker_stdio(address, config).await;
            }
            Err(err) => {
                eprintln!("[interface-worker-stdio] ble_gatt stopped err={err}");
            }
        }
        return;
    }

    let mut stdin = tokio::io::stdin();
    let summary = run_interface_worker_stream(&mut stdin).await;
    match summary {
        Ok(InterfaceWorkerServeSummary { handled, stop_reason }) => {
            eprintln!(
                "[interface-worker-stdio] stopped handled={handled} reason={}",
                stop_reason_label(stop_reason)
            );
        }
        Err(err) => {
            eprintln!("[interface-worker-stdio] stopped err={err:?}");
        }
    }
}

fn ble_interface_config_from_worker_args(args: &Args) -> Result<InterfaceConfig, String> {
    let missing = |field: &str| format!("missing {field}");
    Ok(InterfaceConfig {
        kind: "ble_gatt".to_string(),
        enabled: Some(true),
        name: Some("interface-worker-ble-gatt".to_string()),
        adapter: args.interface_worker_ble_adapter.clone(),
        peripheral_id: Some(
            args.interface_worker_ble_peripheral_id
                .clone()
                .ok_or_else(|| missing("--interface-worker-ble-peripheral-id"))?,
        ),
        service_uuid: Some(
            args.interface_worker_ble_service_uuid
                .clone()
                .ok_or_else(|| missing("--interface-worker-ble-service-uuid"))?,
        ),
        write_char_uuid: Some(
            args.interface_worker_ble_write_char_uuid
                .clone()
                .ok_or_else(|| missing("--interface-worker-ble-write-char-uuid"))?,
        ),
        notify_char_uuid: Some(
            args.interface_worker_ble_notify_char_uuid
                .clone()
                .ok_or_else(|| missing("--interface-worker-ble-notify-char-uuid"))?,
        ),
        mtu: args.interface_worker_ble_mtu,
        scan_timeout_ms: args.interface_worker_ble_scan_timeout_ms,
        connect_timeout_ms: args.interface_worker_ble_connect_timeout_ms,
        reconnect_backoff_ms: args.interface_worker_ble_reconnect_backoff_ms,
        max_reconnect_backoff_ms: args.interface_worker_ble_max_reconnect_backoff_ms,
        ..InterfaceConfig::default()
    })
}

fn serial_interface_from_worker_args(
    args: &Args,
    device: String,
    baud_rate: u32,
) -> Result<SerialInterface, String> {
    if baud_rate == 0 {
        return Err("serial baud rate must be > 0".to_string());
    }
    let mut adapter = SerialInterface::new(device, baud_rate);
    if let Some(data_bits) = args.interface_worker_serial_data_bits {
        adapter = adapter.with_data_bits_raw(data_bits)?;
    }
    if let Some(stop_bits) = args.interface_worker_serial_stop_bits {
        adapter = adapter.with_stop_bits_raw(stop_bits)?;
    }
    if let Some(parity) = args.interface_worker_serial_parity.as_deref() {
        adapter = adapter.with_parity_name(parity)?;
    }
    if let Some(flow_control) = args.interface_worker_serial_flow_control.as_deref() {
        adapter = adapter.with_flow_control_name(flow_control)?;
    }
    if let Some(mtu) = args.interface_worker_serial_mtu {
        adapter = adapter.with_mtu(mtu);
    }
    if let Some(reconnect_backoff_ms) = args.interface_worker_serial_reconnect_backoff_ms {
        adapter = adapter.with_reconnect_backoff(Duration::from_millis(reconnect_backoff_ms));
    }
    if let Some(max_reconnect_backoff_ms) = args.interface_worker_serial_max_reconnect_backoff_ms {
        adapter =
            adapter.with_max_reconnect_backoff(Duration::from_millis(max_reconnect_backoff_ms));
    }
    Ok(adapter)
}

async fn run_ble_interface_worker_stdio(parent_address: AddressHash, config: InterfaceConfig) {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(128)));
    let child_address = match ble::spawn(iface_manager.clone(), &config).await {
        Ok(address) => address,
        Err(err) => {
            eprintln!("[interface-worker-stdio] ble_gatt stopped err={err}");
            return;
        }
    };
    let rx_receiver = iface_manager.lock().await.receiver();
    let mut rx_receiver = rx_receiver.lock().await;
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let result = run_interface_manager_worker_stdio_streams(
        &mut stdin,
        &mut stdout,
        parent_address,
        child_address,
        iface_manager.clone(),
        &mut rx_receiver,
    )
    .await;
    iface_manager.lock().await.stop_interface(child_address);
    if let Err(err) = result {
        eprintln!("[interface-worker-stdio] ble_gatt stopped err={err:?}");
    }
}

async fn run_tcp_server_interface_worker_stdio(parent_address: AddressHash, listen_addr: String) {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(128)));
    let server_address = iface_manager
        .lock()
        .await
        .spawn(TcpServer::new(listen_addr, iface_manager.clone()), TcpServer::spawn);
    let rx_receiver = iface_manager.lock().await.receiver();
    let mut rx_receiver = rx_receiver.lock().await;
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let result = run_interface_manager_worker_stdio_streams(
        &mut stdin,
        &mut stdout,
        parent_address,
        server_address,
        iface_manager.clone(),
        &mut rx_receiver,
    )
    .await;
    iface_manager.lock().await.stop_interface(server_address);
    if let Err(err) = result {
        eprintln!("[interface-worker-stdio] tcp_server stopped err={err:?}");
    }
}

async fn run_interface_manager_worker_stdio_streams<R, W>(
    reader: &mut R,
    writer: &mut W,
    parent_address: AddressHash,
    child_address: AddressHash,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    rx_receiver: &mut rns_transport::iface::InterfaceRxReceiver,
) -> Result<(), WorkerCodecError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut sequence = 0u64;
    loop {
        tokio::select! {
            result = read_interface_worker_envelope(reader) => {
                let envelope = match result {
                    Ok(envelope) => envelope,
                    Err(WorkerCodecError::Io { message }) if is_eof_io(&message) => return Ok(()),
                    Err(err) => return Err(err),
                };
                if matches!(envelope.event, InterfaceWorkerEvent::Shutdown) {
                    return Ok(());
                }
                if let Some(mut message) = envelope.event.to_tx_message()? {
                    if matches!(message.tx_type, TxMessageType::Direct(address) if address == parent_address) {
                        message.tx_type = TxMessageType::Direct(child_address);
                    }
                    InterfaceManager::send_from_shared(&iface_manager, message).await;
                }
            }
            message = rx_receiver.recv() => {
                let Some(mut message) = message else {
                    return Ok(());
                };
                if message.address == child_address || message.address == AddressHash::new_empty() {
                    message.address = parent_address;
                }
                write_interface_worker_rx_message(writer, sequence, &message).await?;
                sequence = sequence.saturating_add(1);
            }
        }
    }
}

async fn run_tcp_client_interface_worker_stdio(address: AddressHash, connect_addr: String) {
    let (rx_sender, mut rx_receiver) = InterfaceChannel::make_rx_channel(128);
    let (tx_sender, tx_receiver) = InterfaceChannel::make_tx_channel(128);
    let cancellation = CancellationToken::new();
    let channel = InterfaceChannel::new(rx_sender, tx_receiver, address, cancellation.clone());
    let context = InterfaceContext::<TcpClient> {
        inner: Arc::new(Mutex::new(TcpClient::new(connect_addr))),
        channel,
        cancel: cancellation.clone(),
    };
    let tcp_task = tokio::spawn(async move {
        TcpClient::spawn(context).await;
    });
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let result = run_udp_interface_worker_stdio_streams(
        &mut stdin,
        &mut stdout,
        address,
        tx_sender,
        &mut rx_receiver,
        cancellation.clone(),
    )
    .await;
    cancellation.cancel();
    let _ = tcp_task.await;
    if let Err(err) = result {
        eprintln!("[interface-worker-stdio] tcp_client stopped err={err:?}");
    }
}

async fn run_serial_interface_worker_stdio(address: AddressHash, adapter: SerialInterface) {
    let (rx_sender, mut rx_receiver) = InterfaceChannel::make_rx_channel(128);
    let (tx_sender, tx_receiver) = InterfaceChannel::make_tx_channel(128);
    let cancellation = CancellationToken::new();
    let channel = InterfaceChannel::new(rx_sender, tx_receiver, address, cancellation.clone());
    let context = InterfaceContext::<SerialInterface> {
        inner: Arc::new(Mutex::new(adapter)),
        channel,
        cancel: cancellation.clone(),
    };
    let serial_task = tokio::spawn(async move {
        SerialInterface::spawn(context).await;
    });
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let result = run_udp_interface_worker_stdio_streams(
        &mut stdin,
        &mut stdout,
        address,
        tx_sender,
        &mut rx_receiver,
        cancellation.clone(),
    )
    .await;
    cancellation.cancel();
    let _ = serial_task.await;
    if let Err(err) = result {
        eprintln!("[interface-worker-stdio] serial stopped err={err:?}");
    }
}

async fn run_udp_interface_worker_stdio(
    address: AddressHash,
    bind_addr: String,
    forward_addr: Option<String>,
) -> Result<(), WorkerCodecError> {
    let (rx_sender, mut rx_receiver) = InterfaceChannel::make_rx_channel(128);
    let (tx_sender, tx_receiver) = InterfaceChannel::make_tx_channel(128);
    let cancellation = CancellationToken::new();
    let channel = InterfaceChannel::new(rx_sender, tx_receiver, address, cancellation.clone());
    let context = InterfaceContext::<UdpInterface> {
        inner: Arc::new(Mutex::new(UdpInterface::new(bind_addr, forward_addr))),
        channel,
        cancel: cancellation.clone(),
    };
    let udp_task = tokio::spawn(async move {
        UdpInterface::spawn(context).await;
    });
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let result = run_udp_interface_worker_stdio_streams(
        &mut stdin,
        &mut stdout,
        address,
        tx_sender,
        &mut rx_receiver,
        cancellation.clone(),
    )
    .await;
    cancellation.cancel();
    let _ = udp_task.await;
    result
}

async fn run_udp_interface_worker_stdio_streams<R, W>(
    reader: &mut R,
    writer: &mut W,
    address: AddressHash,
    tx_sender: rns_transport::iface::InterfaceTxSender,
    rx_receiver: &mut rns_transport::iface::InterfaceRxReceiver,
    cancellation: CancellationToken,
) -> Result<(), WorkerCodecError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut sequence = 0u64;
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                return Ok(());
            }
            result = read_interface_worker_envelope(reader) => {
                let envelope = match result {
                    Ok(envelope) => envelope,
                    Err(WorkerCodecError::Io { message }) if is_eof_io(&message) => return Ok(()),
                    Err(err) => return Err(err),
                };
                if matches!(envelope.event, InterfaceWorkerEvent::Shutdown) {
                    return Ok(());
                }
                if let Some(message) = envelope.event.to_tx_message()? {
                    let _ = tx_sender.try_send(message);
                }
            }
            message = rx_receiver.recv() => {
                let Some(mut message) = message else {
                    return Ok(());
                };
                if message.address == AddressHash::new_empty() {
                    message.address = address;
                }
                write_interface_worker_rx_message(writer, sequence, &message).await?;
                sequence = sequence.saturating_add(1);
            }
        }
    }
}

async fn write_interface_worker_rx_message<W>(
    writer: &mut W,
    sequence: u64,
    message: &RxMessage,
) -> Result<(), WorkerCodecError>
where
    W: AsyncWrite + Unpin,
{
    let envelope = InterfaceWorkerEnvelope::inbound_from_rx_message(sequence, message)?;
    write_interface_worker_envelope(writer, &envelope).await
}

pub(super) async fn run_interface_worker_stream<R>(
    reader: &mut R,
) -> Result<InterfaceWorkerServeSummary, WorkerCodecError>
where
    R: AsyncRead + Unpin,
{
    serve_interface_worker_envelopes(reader, |_envelope| async { Ok(()) }).await
}

fn stop_reason_label(reason: InterfaceWorkerServeStopReason) -> &'static str {
    match reason {
        InterfaceWorkerServeStopReason::Eof => "eof",
        InterfaceWorkerServeStopReason::Shutdown => "shutdown",
        InterfaceWorkerServeStopReason::Cancelled => "cancelled",
    }
}

fn is_eof_io(message: &str) -> bool {
    message.contains("early eof") || message.contains("unexpected end of file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_transport::hash::AddressHash;
    use rns_transport::iface::{IfaceSource, RxMessage, TxMessage, TxMessageType};
    use rns_transport::packet::{DestinationType, Packet, PacketDataBuffer, PacketType};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::duplex;

    #[cfg(unix)]
    #[tokio::test]
    async fn interface_worker_stdio_process_client_sends_event_and_shutdown() {
        let temp = tempfile::tempdir().expect("tempdir");
        let marker = temp.path().join("interface-event-seen");
        let script = temp.path().join("interface-worker.py");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import os
import struct
import sys

marker = {marker:?}

def read_frame():
    header = sys.stdin.buffer.read(4)
    if len(header) != 4:
        return None
    length = struct.unpack(">I", header)[0]
    payload = sys.stdin.buffer.read(length)
    return header + payload

first = read_frame()
if first is not None:
    open(marker, "w").close()
    sys.stdout.buffer.write(first)
    sys.stdout.buffer.flush()
read_frame()
"#,
                marker = marker.to_string_lossy(),
            ),
        )
        .expect("write interface worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let mut worker = InterfaceWorkerStdioProcess::spawn(&script).expect("spawn worker");
        let packet = Packet {
            header: rns_transport::packet::Header {
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            destination: AddressHash::new([0x33; rns_transport::hash::ADDRESS_HASH_SIZE]),
            data: PacketDataBuffer::new_from_slice(b"client event"),
            ..Default::default()
        };
        let envelope = InterfaceWorkerEnvelope::outbound_from_tx_message(
            1,
            &TxMessage {
                tx_type: TxMessageType::Direct(AddressHash::new(
                    [0x44; rns_transport::hash::ADDRESS_HASH_SIZE],
                )),
                packet,
            },
        )
        .expect("interface envelope");

        worker.send(&envelope).await.expect("send interface envelope");
        let echoed = worker.recv().await.expect("receive echoed interface envelope");
        let status = worker.shutdown(Duration::from_secs(5)).await.expect("shutdown worker");

        assert_eq!(echoed, envelope);
        assert!(status.success(), "interface worker exited with {status}");
        assert!(marker.exists(), "interface worker should receive framed event");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn interface_worker_channel_bridge_forwards_both_directions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script = temp.path().join("interface-worker-bridge.py");
        let inbound = RxMessage {
            address: AddressHash::new([0x66; rns_transport::hash::ADDRESS_HASH_SIZE]),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0x77; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"child inbound"),
                ..Default::default()
            },
            source: IfaceSource::None,
        };
        let inbound_frame = InterfaceWorkerEnvelope::inbound_from_rx_message(9, &inbound)
            .expect("inbound envelope")
            .encode_frame()
            .expect("inbound frame");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import struct
import sys

def read_frame():
    header = sys.stdin.buffer.read(4)
    if len(header) != 4:
        return None
    length = struct.unpack(">I", header)[0]
    return sys.stdin.buffer.read(length)

read_frame()
sys.stdout.buffer.write(bytes.fromhex({inbound_frame_hex:?}))
sys.stdout.buffer.flush()
read_frame()
"#,
                inbound_frame_hex = hex::encode(inbound_frame),
            ),
        )
        .expect("write interface worker bridge script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let worker = InterfaceWorkerStdioProcess::spawn(&script).expect("spawn worker");
        let (tx_sender, mut tx_receiver) = tokio::sync::mpsc::channel(4);
        let (rx_sender, mut rx_receiver) = tokio::sync::mpsc::channel(4);
        let outbound = TxMessage {
            tx_type: TxMessageType::Direct(AddressHash::new(
                [0x88; rns_transport::hash::ADDRESS_HASH_SIZE],
            )),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0x99; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"parent outbound"),
                ..Default::default()
            },
        };

        let bridge = tokio::spawn(async move {
            worker
                .run_channel_bridge(&mut tx_receiver, &rx_sender, Duration::from_secs(5))
                .await
                .expect("run bridge")
        });
        tx_sender.send(outbound).await.expect("send outbound");
        assert_eq!(rx_receiver.recv().await, Some(inbound));
        drop(tx_sender);
        let summary = bridge.await.expect("bridge task");

        assert_eq!(
            summary,
            InterfaceWorkerBridgeSummary {
                sent: 1,
                received: 1,
                stop_reason: InterfaceWorkerBridgeStopReason::TransportTxClosed,
            }
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn interface_worker_channel_bridge_cancellation_shuts_down_child() {
        let temp = tempfile::tempdir().expect("tempdir");
        let marker = temp.path().join("interface-shutdown-seen");
        let script = temp.path().join("interface-worker-cancel.py");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import struct
import sys

marker = {marker:?}

def read_frame():
    header = sys.stdin.buffer.read(4)
    if len(header) != 4:
        return None
    length = struct.unpack(">I", header)[0]
    payload = sys.stdin.buffer.read(length)
    return header + payload

if read_frame() is not None:
    open(marker, "w").close()
"#,
                marker = marker.to_string_lossy(),
            ),
        )
        .expect("write interface worker cancel script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let worker = InterfaceWorkerStdioProcess::spawn(&script).expect("spawn worker");
        let (_tx_sender, mut tx_receiver) = tokio::sync::mpsc::channel(4);
        let (rx_sender, _rx_receiver) = tokio::sync::mpsc::channel(4);
        let cancellation = CancellationToken::new();
        let bridge_cancellation = cancellation.clone();

        let bridge = tokio::spawn(async move {
            worker
                .run_channel_bridge_until_cancelled(
                    &mut tx_receiver,
                    &rx_sender,
                    Duration::from_secs(5),
                    bridge_cancellation,
                )
                .await
                .expect("run bridge")
        });
        cancellation.cancel();
        let summary = bridge.await.expect("bridge task");

        assert_eq!(
            summary,
            InterfaceWorkerBridgeSummary {
                sent: 0,
                received: 0,
                stop_reason: InterfaceWorkerBridgeStopReason::Cancelled,
            }
        );
        assert!(marker.exists(), "interface worker should observe shutdown frame");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn interface_worker_bridge_registers_manager_channel() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script = temp.path().join("interface-worker-manager.py");
        let inbound = RxMessage {
            address: AddressHash::new([0xaa; rns_transport::hash::ADDRESS_HASH_SIZE]),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0xbb; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"manager inbound"),
                ..Default::default()
            },
            source: IfaceSource::None,
        };
        let inbound_frame = InterfaceWorkerEnvelope::inbound_from_rx_message(11, &inbound)
            .expect("inbound envelope")
            .encode_frame()
            .expect("inbound frame");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import struct
import sys

def read_frame():
    header = sys.stdin.buffer.read(4)
    if len(header) != 4:
        return None
    length = struct.unpack(">I", header)[0]
    return sys.stdin.buffer.read(length)

read_frame()
sys.stdout.buffer.write(bytes.fromhex({inbound_frame_hex:?}))
sys.stdout.buffer.flush()
read_frame()
"#,
                inbound_frame_hex = hex::encode(inbound_frame),
            ),
        )
        .expect("write interface worker manager script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(4)));
        let cancellation = CancellationToken::new();
        let mut handle = spawn_interface_worker_bridge(
            iface_manager.clone(),
            &script,
            IfaceRole::Unicast,
            InterfaceMode::Full,
            Duration::from_secs(5),
            Duration::from_millis(DEFAULT_INTERFACE_WORKER_RESTART_BACKOFF_MS),
            cancellation.clone(),
        )
        .await
        .expect("spawn interface worker bridge");

        assert_eq!(iface_manager.lock().await.role(&handle.address), Some(IfaceRole::Unicast));
        let rx_receiver = iface_manager.lock().await.receiver();
        let outbound = TxMessage {
            tx_type: TxMessageType::Direct(handle.address),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0xcc; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"manager outbound"),
                ..Default::default()
            },
        };

        let trace = InterfaceManager::send_from_shared(&iface_manager, outbound).await;
        assert_eq!(trace.sent_ifaces, 1);
        let received =
            timeout(Duration::from_secs(5), async { rx_receiver.lock().await.recv().await })
                .await
                .expect("receive inbound from bridge");
        assert_eq!(received, Some(inbound));

        cancellation.cancel();
        let summary = handle
            .task
            .take()
            .expect("bridge task")
            .await
            .expect("bridge join")
            .expect("bridge result");
        assert_eq!(
            summary,
            InterfaceWorkerBridgeSummary {
                sent: 1,
                received: 1,
                stop_reason: InterfaceWorkerBridgeStopReason::Cancelled,
            }
        );
    }

    #[tokio::test]
    async fn interface_worker_manager_stream_translates_parent_and_child_addresses() {
        let parent_address = AddressHash::new([0xab; rns_transport::hash::ADDRESS_HASH_SIZE]);
        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(4)));
        let channel = iface_manager.lock().await.new_channel_with_role_and_mode(
            4,
            IfaceRole::Unicast,
            InterfaceMode::Full,
        );
        let child_address = channel.address;
        let (rx_sender, mut tx_receiver) = channel.split();
        let rx_receiver = iface_manager.lock().await.receiver();
        let (mut client, mut server) = duplex(16 * 1024);
        let manager = iface_manager.clone();
        let bridge = tokio::spawn(async move {
            let (mut server_reader, mut server_writer) = tokio::io::split(&mut server);
            let mut rx_receiver = rx_receiver.lock().await;
            run_interface_manager_worker_stdio_streams(
                &mut server_reader,
                &mut server_writer,
                parent_address,
                child_address,
                manager,
                &mut rx_receiver,
            )
            .await
        });

        let outbound = TxMessage {
            tx_type: TxMessageType::Direct(parent_address),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0xcd; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"to child"),
                ..Default::default()
            },
        };
        let envelope = InterfaceWorkerEnvelope::outbound_from_tx_message(0, &outbound)
            .expect("outbound envelope");
        write_interface_worker_envelope(&mut client, &envelope)
            .await
            .expect("write outbound envelope");
        let delivered = timeout(Duration::from_secs(2), tx_receiver.recv())
            .await
            .expect("child outbound")
            .expect("child message");
        assert_eq!(delivered.tx_type, TxMessageType::Direct(child_address));
        assert_eq!(delivered.packet.data.as_slice(), b"to child");

        let inbound = RxMessage {
            address: child_address,
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0xef; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"from child"),
                ..Default::default()
            },
            source: IfaceSource::None,
        };
        rx_sender.send(inbound).await.expect("send inbound");
        let received = timeout(Duration::from_secs(2), read_interface_worker_envelope(&mut client))
            .await
            .expect("read inbound envelope")
            .expect("inbound envelope");
        let received = received.event.to_rx_message().expect("decode inbound").expect("rx message");
        assert_eq!(received.address, parent_address);
        assert_eq!(received.packet.data.as_slice(), b"from child");

        write_interface_worker_envelope(
            &mut client,
            &InterfaceWorkerEnvelope::new(1, InterfaceWorkerEvent::Shutdown),
        )
        .await
        .expect("write shutdown");
        bridge.await.expect("bridge join").expect("bridge result");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn interface_worker_bridge_restarts_child_after_early_exit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script = temp.path().join("interface-worker-restart.py");
        let counter = temp.path().join("restart-count");
        let inbound = RxMessage {
            address: AddressHash::new([0xdd; rns_transport::hash::ADDRESS_HASH_SIZE]),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0xee; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"restarted child inbound"),
                ..Default::default()
            },
            source: IfaceSource::None,
        };
        let inbound_frame = InterfaceWorkerEnvelope::inbound_from_rx_message(12, &inbound)
            .expect("inbound envelope")
            .encode_frame()
            .expect("inbound frame");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import pathlib
import struct
import sys

counter = pathlib.Path({counter:?})
count = int(counter.read_text()) if counter.exists() else 0
counter.write_text(str(count + 1))
if count == 0:
    sys.exit(0)

def read_frame():
    header = sys.stdin.buffer.read(4)
    if len(header) != 4:
        return None
    length = struct.unpack(">I", header)[0]
    return sys.stdin.buffer.read(length)

read_frame()
sys.stdout.buffer.write(bytes.fromhex({inbound_frame_hex:?}))
sys.stdout.buffer.flush()
read_frame()
"#,
                counter = counter.to_string_lossy(),
                inbound_frame_hex = hex::encode(inbound_frame),
            ),
        )
        .expect("write restart interface worker script");
        let mut permissions = std::fs::metadata(&script).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("script permissions");

        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(4)));
        let cancellation = CancellationToken::new();
        let mut handle = spawn_interface_worker_bridge(
            iface_manager.clone(),
            &script,
            IfaceRole::Unicast,
            InterfaceMode::Full,
            Duration::from_secs(5),
            Duration::from_millis(25),
            cancellation.clone(),
        )
        .await
        .expect("spawn interface worker bridge");

        let mut observed_count = 0usize;
        for _ in 0..80 {
            observed_count = std::fs::read_to_string(&counter)
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or_default();
            if observed_count >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(observed_count, 2);
        assert_eq!(handle.child_restarts(), 1);
        assert_eq!(handle.child_errors(), 0);

        let rx_receiver = iface_manager.lock().await.receiver();
        let outbound = TxMessage {
            tx_type: TxMessageType::Direct(handle.address),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0xef; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"after restart"),
                ..Default::default()
            },
        };
        let trace = InterfaceManager::send_from_shared(&iface_manager, outbound).await;
        assert_eq!(trace.sent_ifaces, 1);
        let received =
            timeout(Duration::from_secs(5), async { rx_receiver.lock().await.recv().await })
                .await
                .expect("receive inbound after restart");
        assert_eq!(received, Some(inbound));

        cancellation.cancel();
        let summary = handle
            .task
            .take()
            .expect("bridge task")
            .await
            .expect("bridge join")
            .expect("bridge result");
        assert_eq!(
            summary,
            InterfaceWorkerBridgeSummary {
                sent: 1,
                received: 1,
                stop_reason: InterfaceWorkerBridgeStopReason::Cancelled,
            }
        );
    }

    #[tokio::test]
    async fn udp_interface_worker_stdio_streams_bridge_tx_and_rx_channels() {
        let address = AddressHash::new([0x12; rns_transport::hash::ADDRESS_HASH_SIZE]);
        let (mut parent_to_child, mut child_stdin) = duplex(16 * 1024);
        let (mut child_stdout, mut parent_from_child) = duplex(16 * 1024);
        let (tx_sender, mut tx_receiver) = InterfaceChannel::make_tx_channel(4);
        let (rx_sender, mut rx_receiver) = InterfaceChannel::make_rx_channel(4);
        let cancellation = CancellationToken::new();
        let bridge_cancellation = cancellation.clone();

        let bridge = tokio::spawn(async move {
            run_udp_interface_worker_stdio_streams(
                &mut child_stdin,
                &mut child_stdout,
                address,
                tx_sender,
                &mut rx_receiver,
                bridge_cancellation,
            )
            .await
        });

        let outbound = TxMessage {
            tx_type: TxMessageType::Direct(address),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0x34; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"udp child outbound"),
                ..Default::default()
            },
        };
        let outbound_envelope =
            InterfaceWorkerEnvelope::outbound_from_tx_message(1, &outbound).expect("outbound");
        write_interface_worker_envelope(&mut parent_to_child, &outbound_envelope)
            .await
            .expect("write outbound envelope");
        assert_eq!(
            timeout(Duration::from_secs(5), tx_receiver.recv()).await.expect("tx recv"),
            Some(outbound)
        );

        let inbound = RxMessage {
            address: AddressHash::new_empty(),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0x56; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"udp child inbound"),
                ..Default::default()
            },
            source: IfaceSource::None,
        };
        rx_sender.send(inbound).await.expect("send inbound");
        let envelope =
            timeout(Duration::from_secs(5), read_interface_worker_envelope(&mut parent_from_child))
                .await
                .expect("read inbound envelope")
                .expect("inbound envelope");
        let received = envelope.event.to_rx_message().expect("rx message").expect("rx");
        assert_eq!(received.address, address);
        assert_eq!(received.packet.data.as_slice(), b"udp child inbound");

        cancellation.cancel();
        bridge.await.expect("bridge join").expect("bridge result");
    }

    #[tokio::test]
    async fn udp_interface_worker_stdio_drops_outbound_when_tx_channel_is_full() {
        let address = AddressHash::new([0x12; rns_transport::hash::ADDRESS_HASH_SIZE]);
        let (mut parent_to_child, mut child_stdin) = duplex(16 * 1024);
        let (mut child_stdout, _parent_from_child) = duplex(16 * 1024);
        let (tx_sender, mut tx_receiver) = InterfaceChannel::make_tx_channel(1);
        let (_rx_sender, mut rx_receiver) = InterfaceChannel::make_rx_channel(4);
        let existing = TxMessage {
            tx_type: TxMessageType::Direct(address),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0x22; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"existing"),
                ..Default::default()
            },
        };
        tx_sender.try_send(existing.clone()).expect("prefill tx channel");
        let cancellation = CancellationToken::new();
        let bridge_cancellation = cancellation.clone();

        let bridge = tokio::spawn(async move {
            run_udp_interface_worker_stdio_streams(
                &mut child_stdin,
                &mut child_stdout,
                address,
                tx_sender,
                &mut rx_receiver,
                bridge_cancellation,
            )
            .await
        });

        let outbound = TxMessage {
            tx_type: TxMessageType::Direct(address),
            packet: Packet {
                header: rns_transport::packet::Header {
                    destination_type: DestinationType::Single,
                    packet_type: PacketType::Data,
                    ..Default::default()
                },
                destination: AddressHash::new([0x34; rns_transport::hash::ADDRESS_HASH_SIZE]),
                data: PacketDataBuffer::new_from_slice(b"dropped outbound"),
                ..Default::default()
            },
        };
        let outbound_envelope =
            InterfaceWorkerEnvelope::outbound_from_tx_message(1, &outbound).expect("outbound");
        let shutdown = InterfaceWorkerEnvelope::new(2, InterfaceWorkerEvent::Shutdown);
        timeout(
            Duration::from_millis(50),
            write_interface_worker_envelope(&mut parent_to_child, &outbound_envelope),
        )
        .await
        .expect("full tx channel should not back up child stdout reader")
        .expect("write outbound envelope");
        write_interface_worker_envelope(&mut parent_to_child, &shutdown)
            .await
            .expect("write shutdown");

        bridge.await.expect("bridge join").expect("bridge result");
        assert_eq!(tx_receiver.try_recv().expect("existing message"), existing);
        assert!(tx_receiver.try_recv().is_err());
    }
}

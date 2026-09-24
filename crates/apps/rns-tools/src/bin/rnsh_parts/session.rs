use crate::rnsh_parts::network::{close_link, establish_link, wait_for_destination, Runtime};
use crate::rnsh_parts::process::{run_command, send_stdin, CommandSpec};
use crate::rnsh_parts::protocol::{
    CommandExitedMessage, ErrorMessage, ExecuteCommandMessage, NoopMessage, StreamDataMessage,
    VersionInfoMessage, WindowSizeMessage, PROTOCOL_VERSION,
};
use crate::rnsh_parts::terminal_size::current_terminal_window_size;
#[cfg(unix)]
use crate::rnsh_parts::terminal_size::send_window_size_changes;
use rns_transport::hash::AddressHash;
use rns_transport::transport::{SendPacketOutcome, TransportChannel};
use std::io;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{timeout, Duration};

#[derive(Debug)]
enum IncomingMessage {
    Noop(NoopMessage),
    Version(VersionInfoMessage),
    Window,
    Execute(ExecuteCommandMessage),
    Stream(StreamDataMessage),
    Error(ErrorMessage),
    Exited(CommandExitedMessage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerState {
    WaitingForVersion,
    WaitingForCommand,
    Running,
}

pub(crate) async fn serve_link(
    runtime: Runtime,
    link_id: AddressHash,
    mut link_closed: watch::Receiver<bool>,
) -> io::Result<()> {
    let channel = runtime.transport.channel(link_id);
    let (message_tx, mut message_rx) = mpsc::channel(64);
    let queue_overflowed = Arc::new(AtomicBool::new(false));
    register_handlers(&channel, &message_tx, queue_overflowed.clone()).await?;

    let mut state = ServerState::WaitingForVersion;
    let mut stdin_tx: Option<mpsc::Sender<Vec<u8>>> = None;
    let mut command_cancel: Option<oneshot::Sender<()>> = None;
    let mut command_task = None;

    let session_result = async {
        loop {
            let message = tokio::select! {
                changed = link_closed.changed() => {
                    if changed.is_err() || *link_closed.borrow() {
                        break;
                    }
                    continue;
                }
                message = message_rx.recv() => match message {
                    Some(message) => message,
                    None => break,
                }
            };
            if queue_overflowed.load(Ordering::Acquire) {
                break;
            }
            match message {
                IncomingMessage::Version(version) => {
                    if state != ServerState::WaitingForVersion {
                        send_protocol_error(&channel, "unexpected version message").await?;
                        break;
                    }
                    if version.protocol_version != PROTOCOL_VERSION {
                        send_protocol_error(&channel, "incompatible rnsh protocol").await?;
                        break;
                    }
                    channel
                        .send_typed(&VersionInfoMessage::current())
                        .await
                        .map_err(channel_error)?;
                    state = ServerState::WaitingForCommand;
                }
                IncomingMessage::Execute(message) => {
                    if state != ServerState::WaitingForCommand {
                        send_protocol_error(&channel, "unexpected execute message").await?;
                        break;
                    }
                    let command = resolve_server_command(&runtime, message.command.as_deref());
                    if command.is_empty() {
                        send_protocol_error(&channel, "no remote shell command configured").await?;
                        break;
                    }
                    let (new_stdin_tx, stdin_rx) = mpsc::channel(16);
                    if message.pipe_stdin {
                        stdin_tx = Some(new_stdin_tx);
                    }
                    let (cancel_tx, cancel_rx) = oneshot::channel();
                    command_cancel = Some(cancel_tx);
                    let process_channel = channel.clone();
                    let root = runtime.root.clone();
                    let remote_identity = runtime
                        .peer_identity(link_id)
                        .await
                        .map(|identity| identity.address_hash.to_hex_string());
                    command_task = Some(tokio::spawn(async move {
                        if let Err(error) = run_command(
                            process_channel.clone(),
                            CommandSpec {
                                command,
                                root,
                                pipe_stdin: message.pipe_stdin,
                                pipe_stdout: message.pipe_stdout,
                                pipe_stderr: message.pipe_stderr,
                                term: message.term,
                                remote_identity,
                            },
                            stdin_rx,
                            cancel_rx,
                        )
                        .await
                        {
                            if let Err(send_error) = process_channel
                                .send_typed(&ErrorMessage::fatal(error.to_string()))
                                .await
                            {
                                log::debug!(
                                    "rnsh session {} could not send command error: {:?}",
                                    process_channel.link_id().to_hex_string(),
                                    send_error
                                );
                            }
                        }
                    }));
                    state = ServerState::Running;
                }
                IncomingMessage::Stream(message) => {
                    if state != ServerState::Running || message.stream_id != 0 {
                        send_protocol_error(&channel, "invalid stdin stream").await?;
                        break;
                    }
                    if let Some(sender) = stdin_tx.as_ref() {
                        if !message.data.is_empty() {
                            sender.send(message.data).await.map_err(|_| {
                                io::Error::new(
                                    io::ErrorKind::BrokenPipe,
                                    "remote command stdin closed",
                                )
                            })?;
                        }
                        if message.eof {
                            stdin_tx = None;
                        }
                    }
                }
                IncomingMessage::Window => {}
                IncomingMessage::Noop(NoopMessage) => {
                    if state == ServerState::Running {
                        channel.send_typed(&NoopMessage).await.map_err(channel_error)?;
                    }
                }
                IncomingMessage::Error(error) => {
                    if error.fatal {
                        break;
                    }
                }
                IncomingMessage::Exited(_) => {
                    send_protocol_error(&channel, "unexpected command exit message").await?;
                    break;
                }
            }
        }
        Ok::<(), io::Error>(())
    }
    .await;

    if let Some(cancel) = command_cancel {
        let _ = cancel.send(());
    }
    if let Some(task) = command_task {
        if let Err(error) = task.await {
            log::debug!("rnsh session {} command task ended: {}", link_id.to_hex_string(), error);
        }
    }

    session_result
}

pub(crate) async fn initiate(
    runtime: &Runtime,
    destination: AddressHash,
    command: Vec<String>,
) -> io::Result<i64> {
    let description = wait_for_destination(runtime, destination).await?;
    let (link, link_id) = establish_link(runtime, description).await?;
    if !runtime.no_id {
        let packet = {
            let guard = link.lock().await;
            let payload = guard.identify_payload(&runtime.identity);
            guard.identify_packet(&payload).map_err(|error| {
                io::Error::other(format!("could not build identity packet: {error:?}"))
            })?
        };
        let outcome = runtime.transport.send_link_packet_on_bound_iface(&link, packet).await;
        if !matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast) {
            close_link(&runtime.transport, &link).await;
            return Err(io::Error::other(format!("could not send identity packet: {outcome:?}")));
        }
    }

    let channel = runtime.transport.channel(link_id);
    let (message_tx, mut message_rx) = mpsc::channel(64);
    let queue_overflowed = Arc::new(AtomicBool::new(false));
    register_handlers(&channel, &message_tx, queue_overflowed.clone()).await?;
    channel.send_typed(&VersionInfoMessage::current()).await.map_err(channel_error)?;

    let peer_version =
        wait_for_version(&mut message_rx, &queue_overflowed, runtime.timeout).await?;
    if peer_version.protocol_version != PROTOCOL_VERSION {
        close_link(&runtime.transport, &link).await;
        return Err(io::Error::other("remote rnsh protocol version is incompatible"));
    }

    let initial_window_size = current_terminal_window_size();
    channel
        .send_typed(&ExecuteCommandMessage {
            command: (!command.is_empty()).then_some(command),
            pipe_stdin: true,
            pipe_stdout: true,
            pipe_stderr: true,
            term: std::env::var("TERM").ok(),
            rows: initial_window_size.map(|size| size.rows),
            cols: initial_window_size.map(|size| size.cols),
            hpix: initial_window_size.map(|size| size.hpix),
            vpix: initial_window_size.map(|size| size.vpix),
        })
        .await
        .map_err(channel_error)?;
    #[cfg(unix)]
    let mut resize_task = tokio::spawn(send_window_size_changes(channel.clone()));
    let stdin = tokio::io::stdin();
    let stdin_task = tokio::spawn(send_stdin(channel.clone(), stdin));

    let return_code = wait_for_command(&mut message_rx, &queue_overflowed, runtime.timeout).await;
    stdin_task.abort();
    #[cfg(unix)]
    {
        resize_task.abort();
        let _ = (&mut resize_task).await;
    }
    close_link(&runtime.transport, &link).await;
    return_code
}

async fn wait_for_version(
    messages: &mut mpsc::Receiver<IncomingMessage>,
    queue_overflowed: &AtomicBool,
    duration: Duration,
) -> io::Result<VersionInfoMessage> {
    timeout(duration, async {
        while let Some(message) = messages.recv().await {
            if queue_overflowed.load(Ordering::Acquire) {
                return Err(io::Error::other("rnsh incoming message queue overflowed"));
            }
            match message {
                IncomingMessage::Version(version) => return Ok(version),
                IncomingMessage::Error(error) => {
                    return Err(io::Error::other(format!("remote rnsh error: {}", error.message)))
                }
                _ => {}
            }
        }
        Err(io::Error::new(io::ErrorKind::UnexpectedEof, "rnsh link closed before version"))
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rnsh version negotiation timed out"))?
}

async fn wait_for_command(
    messages: &mut mpsc::Receiver<IncomingMessage>,
    queue_overflowed: &AtomicBool,
    duration: Duration,
) -> io::Result<i64> {
    timeout(duration, async {
        while let Some(message) = messages.recv().await {
            if queue_overflowed.load(Ordering::Acquire) {
                return Err(io::Error::other("rnsh incoming message queue overflowed"));
            }
            match message {
                IncomingMessage::Stream(stream) => match stream.stream_id {
                    1 => write_output(tokio::io::stdout(), &stream.data).await?,
                    2 => write_output(tokio::io::stderr(), &stream.data).await?,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "remote rnsh sent an invalid output stream",
                        ))
                    }
                },
                IncomingMessage::Exited(exited) => return Ok(exited.return_code),
                IncomingMessage::Error(error) => {
                    return Err(io::Error::other(format!("remote rnsh error: {}", error.message)))
                }
                IncomingMessage::Noop(NoopMessage) => {}
                IncomingMessage::Version(_)
                | IncomingMessage::Window
                | IncomingMessage::Execute(_) => {}
            }
        }
        Err(io::Error::new(io::ErrorKind::UnexpectedEof, "rnsh link closed before command exit"))
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rnsh command timed out"))?
}

fn resolve_server_command(runtime: &Runtime, remote: Option<&[String]>) -> Vec<String> {
    let default = runtime.default_command.clone();
    let Some(remote) = remote.filter(|command| !command.is_empty()) else {
        return default;
    };
    if runtime.no_remote_command {
        return default;
    }
    if runtime.remote_command_as_args {
        default.into_iter().chain(remote.iter().cloned()).collect()
    } else {
        remote.to_vec()
    }
}

async fn write_output<W: AsyncWrite + Unpin>(mut output: W, bytes: &[u8]) -> io::Result<()> {
    output.write_all(bytes).await?;
    output.flush().await
}

async fn register_handlers(
    channel: &TransportChannel,
    sender: &mpsc::Sender<IncomingMessage>,
    queue_overflowed: Arc<AtomicBool>,
) -> io::Result<()> {
    let tx = sender.clone();
    let overflow = queue_overflowed.clone();
    channel
        .register_typed_handler::<NoopMessage, _>(move |message| {
            enqueue_message(&tx, &overflow, IncomingMessage::Noop(message));
            true
        })
        .await
        .map_err(channel_error)?;
    let tx = sender.clone();
    let overflow = queue_overflowed.clone();
    channel
        .register_typed_handler::<VersionInfoMessage, _>(move |message| {
            enqueue_message(&tx, &overflow, IncomingMessage::Version(message));
            true
        })
        .await
        .map_err(channel_error)?;
    let tx = sender.clone();
    let overflow = queue_overflowed.clone();
    channel
        .register_typed_handler::<WindowSizeMessage, _>(move |_message| {
            enqueue_message(&tx, &overflow, IncomingMessage::Window);
            true
        })
        .await
        .map_err(channel_error)?;
    let tx = sender.clone();
    let overflow = queue_overflowed.clone();
    channel
        .register_typed_handler::<ExecuteCommandMessage, _>(move |message| {
            enqueue_message(&tx, &overflow, IncomingMessage::Execute(message));
            true
        })
        .await
        .map_err(channel_error)?;
    let tx = sender.clone();
    let overflow = queue_overflowed.clone();
    channel
        .register_typed_handler::<StreamDataMessage, _>(move |message| {
            enqueue_message(&tx, &overflow, IncomingMessage::Stream(message));
            true
        })
        .await
        .map_err(channel_error)?;
    let tx = sender.clone();
    let overflow = queue_overflowed.clone();
    channel
        .register_typed_handler::<ErrorMessage, _>(move |message| {
            enqueue_message(&tx, &overflow, IncomingMessage::Error(message));
            true
        })
        .await
        .map_err(channel_error)?;
    let tx = sender.clone();
    let overflow = queue_overflowed;
    channel
        .register_typed_handler::<CommandExitedMessage, _>(move |message| {
            enqueue_message(&tx, &overflow, IncomingMessage::Exited(message));
            true
        })
        .await
        .map_err(channel_error)?;
    Ok(())
}

fn enqueue_message(
    sender: &mpsc::Sender<IncomingMessage>,
    queue_overflowed: &AtomicBool,
    message: IncomingMessage,
) {
    if let Err(error) = sender.try_send(message) {
        queue_overflowed.store(true, Ordering::Release);
        match error {
            mpsc::error::TrySendError::Full(_) => {
                log::error!("rnsh incoming message queue is full; closing session")
            }
            mpsc::error::TrySendError::Closed(_) => {
                log::debug!("rnsh incoming message queue is closed")
            }
        }
    }
}

async fn send_protocol_error(channel: &TransportChannel, message: &str) -> io::Result<()> {
    channel.send_typed(&ErrorMessage::fatal(message)).await.map_err(channel_error).map(|_| ())
}

fn channel_error(error: impl std::fmt::Debug) -> io::Error {
    io::Error::other(format!("remote shell channel error: {error:?}"))
}

use crate::rnsh_parts::protocol::WindowSizeMessage;
use crate::rnsh_parts::protocol::{CommandExitedMessage, StreamDataMessage};
use crate::rnsh_parts::pty::{window_size, PtyCommand};
use portable_pty::ChildKiller;
use rns_transport::channel::{ChannelError, TypedMessage};
use rns_transport::transport::TransportChannel;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::Duration as StdDuration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{sleep, Duration, Instant};

const STREAM_STDIN: u16 = 0;
const STREAM_STDOUT: u16 = 1;
const STREAM_STDERR: u16 = 2;
// Give short-lived reference listeners time to publish command exit before
// their stdin-close cleanup path tears down a still-starting child.
const STDIN_EOF_GRACE: Duration = Duration::from_secs(1);
const STREAM_WRITE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub(crate) struct CommandSpec {
    pub(crate) command: Vec<String>,
    pub(crate) root: PathBuf,
    pub(crate) pipe_stdin: bool,
    pub(crate) pipe_stdout: bool,
    pub(crate) pipe_stderr: bool,
    pub(crate) term: Option<String>,
    pub(crate) remote_identity: Option<String>,
    pub(crate) rows: Option<u64>,
    pub(crate) cols: Option<u64>,
    pub(crate) hpix: Option<u64>,
    pub(crate) vpix: Option<u64>,
}

pub(crate) async fn run_command(
    channel: TransportChannel,
    spec: CommandSpec,
    stdin_rx: mpsc::Receiver<Vec<u8>>,
    cancel: oneshot::Receiver<()>,
    resize_rx: mpsc::Receiver<WindowSizeMessage>,
) -> io::Result<()> {
    if !spec.pipe_stdin || !spec.pipe_stdout || !spec.pipe_stderr {
        return run_pty_command(channel, spec, stdin_rx, cancel, resize_rx).await;
    }
    run_pipe_command(channel, spec, stdin_rx, cancel).await
}

async fn run_pipe_command(
    channel: TransportChannel,
    spec: CommandSpec,
    mut stdin_rx: mpsc::Receiver<Vec<u8>>,
    mut cancel: oneshot::Receiver<()>,
) -> io::Result<()> {
    let executable = spec
        .command
        .first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "remote command is empty"))?;
    let mut process = Command::new(executable);
    process
        .args(&spec.command[1..])
        .current_dir(&spec.root)
        .env("TERM", spec.term.as_deref().unwrap_or("xterm"));
    if let Some(remote_identity) = spec.remote_identity.as_deref() {
        process.env("RNS_REMOTE_IDENTITY", remote_identity);
    }
    process
        .stdin(if spec.pipe_stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(if spec.pipe_stdout { Stdio::piped() } else { Stdio::null() })
        .stderr(if spec.pipe_stderr { Stdio::piped() } else { Stdio::null() })
        .kill_on_drop(true);

    let mut child = process
        .spawn()
        .map_err(|error| io::Error::other(format!("unable to start remote command: {error}")))?;

    let stdin_task = child.stdin.take().map(|mut stdin| {
        tokio::spawn(async move {
            while let Some(bytes) = stdin_rx.recv().await {
                stdin.write_all(&bytes).await?;
            }
            stdin.shutdown().await
        })
    });

    let stdout_task = child
        .stdout
        .take()
        .filter(|_| spec.pipe_stdout)
        .map(|stdout| tokio::spawn(stream_output(channel.clone(), STREAM_STDOUT, stdout)));
    let stderr_task = child
        .stderr
        .take()
        .filter(|_| spec.pipe_stderr)
        .map(|stderr| tokio::spawn(stream_output(channel.clone(), STREAM_STDERR, stderr)));

    let (status, cancelled) = tokio::select! {
        status = child.wait() => (status?, false),
        _ = &mut cancel => {
            // The session owns the cancellation sender and signals it when the
            // Link closes or its receive loop exits. Kill and reap before
            // returning so neither the command nor its pipes outlive the Link.
            if let Err(error) = child.start_kill() {
                if child.try_wait()?.is_none() {
                    return Err(error);
                }
            }
            (child.wait().await?, true)
        }
    };
    stop_task(stdin_task).await;
    if cancelled {
        stop_task(stdout_task).await;
        stop_task(stderr_task).await;
    } else {
        if let Some(task) = stdout_task {
            task.await.map_err(join_error)??;
        }
        if let Some(task) = stderr_task {
            task.await.map_err(join_error)??;
        }
    }

    let return_code = status.code().map(i64::from).unwrap_or(-1);
    send_typed_with_retry(&channel, &CommandExitedMessage { return_code }).await
}

async fn run_pty_command(
    channel: TransportChannel,
    spec: CommandSpec,
    mut stdin_rx: mpsc::Receiver<Vec<u8>>,
    mut cancel: oneshot::Receiver<()>,
    mut resize_rx: mpsc::Receiver<WindowSizeMessage>,
) -> io::Result<()> {
    let initial_size = window_size(spec.rows, spec.cols, spec.hpix, spec.vpix)?;
    let pty = PtyCommand::spawn(
        &spec.command,
        &spec.root,
        spec.term.as_deref().unwrap_or("xterm"),
        spec.remote_identity.as_deref(),
        initial_size,
    )?;
    let master = Arc::new(Mutex::new(pty.master));
    let output_reader = pty.reader;
    let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(8);
    let reader_task = tokio::task::spawn_blocking(move || {
        let mut reader = output_reader;
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(read) => {
                    if output_tx.blocking_send(buffer[..read].to_vec()).is_err() {
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                // Unix PTY masters report EIO after the slave side closes.
                Err(error) if error.raw_os_error() == Some(5) => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    });
    let output_channel = channel.clone();
    let mut output_task = Some(tokio::spawn(async move {
        let mut writer =
            RnshChannelWriter::new(STREAM_STDOUT, output_channel).map_err(channel_error)?;
        while let Some(bytes) = output_rx.recv().await {
            write_all_with_retry(&mut writer, &bytes).await?;
        }
        writer.close().await.map_err(channel_error)
    }));

    let (input_tx, input_rx) = std_mpsc::sync_channel::<Vec<u8>>(8);
    let input_bridge_tx = input_tx.clone();
    drop(input_tx);
    let input_stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped_for_writer = input_stopped.clone();
    let input_writer_task = tokio::task::spawn_blocking(move || {
        let mut writer = pty.writer;
        loop {
            match input_rx.recv_timeout(StdDuration::from_millis(25)) {
                Ok(bytes) => writer.write_all(&bytes)?,
                Err(std_mpsc::RecvTimeoutError::Timeout)
                    if stopped_for_writer.load(std::sync::atomic::Ordering::Acquire) =>
                {
                    return Ok::<(), io::Error>(());
                }
                Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                    if stopped_for_writer.load(std::sync::atomic::Ordering::Acquire) {
                        return Ok(());
                    }
                    // In canonical mode this is the terminal's EOF character.
                    writer.write_all(&[0x04])?;
                    return Ok(());
                }
            }
        }
    });
    let (child_done_tx, mut child_done_rx) = watch::channel(false);
    let input_bridge_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                bytes = stdin_rx.recv() => match bytes {
                    Some(bytes) => {
                        let sender = input_bridge_tx.clone();
                        tokio::task::spawn_blocking(move || sender.send(bytes))
                            .await
                            .map_err(join_error)?
                            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "PTY input closed"))?;
                    }
                    None => return Ok::<(), io::Error>(()),
                },
                changed = child_done_rx.changed() => {
                    if changed.is_err() || *child_done_rx.borrow() {
                        return Ok(());
                    }
                }
            }
        }
    });
    let killer = Arc::new(Mutex::new(pty.killer));
    let child = pty.child;
    let mut child_wait_task = tokio::task::spawn_blocking(move || {
        let mut child = child;
        child.wait()
    });
    let mut cancelled = false;
    let mut command_error = None;
    let status = loop {
        tokio::select! {
            result = &mut child_wait_task => {
                break result.map_err(join_error)??;
            }
            _ = &mut cancel => {
                cancelled = true;
                kill_pty_child(killer.clone()).await;
                break child_wait_task.await.map_err(join_error)??;
            }
            resize = resize_rx.recv() => {
                if let Some(resize) = resize {
                    let resize_result = window_size(resize.rows, resize.cols, resize.hpix, resize.vpix)
                        .and_then(|size| resize_master(&master, size));
                    if let Err(error) = resize_result {
                        command_error = Some(error);
                        cancelled = true;
                        kill_pty_child(killer.clone()).await;
                        break child_wait_task.await.map_err(join_error)??;
                    }
                }
            }
            result = await_optional_task(&mut output_task) => {
                output_task = None;
                if let Some(result) = result {
                    if let Err(error) = result.map_err(join_error)? {
                        command_error = Some(error);
                        cancelled = true;
                        kill_pty_child(killer.clone()).await;
                        break child_wait_task.await.map_err(join_error)??;
                    }
                }
            }
        }
    };

    input_stopped.store(true, std::sync::atomic::Ordering::Release);
    let _ = child_done_tx.send(true);
    let _ = input_bridge_task.await;
    let result = input_writer_task.await.map_err(join_error)?;
    if let Err(error) = result {
        if !is_closed_pty_error(&error) {
            command_error.get_or_insert(error);
        }
    }

    if cancelled {
        if let Some(task) = output_task.take() {
            task.abort();
            let _ = task.await;
        }
    } else if let Some(task) = output_task.take() {
        task.await.map_err(join_error)??;
    }
    let reader_result = reader_task.await.map_err(join_error)?;
    if !cancelled {
        reader_result?;
    }
    if let Some(error) = command_error {
        return Err(error);
    }
    let return_code = i64::from(status.exit_code());
    send_typed_with_retry(&channel, &CommandExitedMessage { return_code }).await
}

fn is_closed_pty_error(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::UnexpectedEof)
        || is_pty_eio(error)
}

#[cfg(unix)]
fn is_pty_eio(error: &io::Error) -> bool {
    error.raw_os_error() == Some(5)
}

#[cfg(not(unix))]
fn is_pty_eio(_error: &io::Error) -> bool {
    false
}

async fn await_optional_task(
    task: &mut Option<tokio::task::JoinHandle<io::Result<()>>>,
) -> Option<Result<io::Result<()>, tokio::task::JoinError>> {
    match task {
        Some(task) => Some(task.await),
        None => std::future::pending().await,
    }
}

async fn kill_pty_child(killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>) {
    let killer = killer.lock().ok().map(|killer| killer.clone_killer());
    if let Some(mut killer) = killer {
        let _ = tokio::task::spawn_blocking(move || killer.kill()).await;
    }
}

fn resize_master(
    master: &Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    size: portable_pty::PtySize,
) -> io::Result<()> {
    master
        .lock()
        .map_err(|_| io::Error::other("rnsh PTY lock poisoned"))?
        .resize(size)
        .map_err(|error| io::Error::other(format!("rnsh PTY resize failed: {error}")))
}

async fn stop_task(task: Option<tokio::task::JoinHandle<io::Result<()>>>) {
    if let Some(task) = task {
        task.abort();
        let _ = task.await;
    }
}

pub(crate) async fn send_stdin(
    channel: TransportChannel,
    mut input: impl AsyncRead + Unpin,
) -> io::Result<()> {
    let mut writer = RnshChannelWriter::new(STREAM_STDIN, channel).map_err(channel_error)?;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = input.read(&mut buffer).await?;
        if read == 0 {
            sleep(STDIN_EOF_GRACE).await;
            break;
        }
        write_all_with_retry(&mut writer, &buffer[..read]).await?;
    }
    writer.close().await.map_err(channel_error)
}

async fn stream_output<R: AsyncRead + Unpin>(
    channel: TransportChannel,
    stream_id: u16,
    mut input: R,
) -> io::Result<()> {
    let mut writer = RnshChannelWriter::new(stream_id, channel).map_err(channel_error)?;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = input.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        write_all_with_retry(&mut writer, &buffer[..read]).await?;
    }
    writer.close().await.map_err(channel_error)
}

async fn write_all_with_retry(writer: &mut RnshChannelWriter, mut bytes: &[u8]) -> io::Result<()> {
    let deadline = Instant::now() + STREAM_WRITE_TIMEOUT;
    while !bytes.is_empty() {
        let written = match writer.write(bytes).await {
            Ok(written) => written,
            Err(ChannelError::LinkNotReady) if Instant::now() < deadline => {
                sleep(Duration::from_millis(10)).await;
                continue;
            }
            Err(error) => return Err(channel_error(error)),
        };
        if written == 0 {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "remote shell stream was not ready for sending",
                ));
            }
            sleep(Duration::from_millis(10)).await;
            continue;
        }
        bytes = &bytes[written..];
    }
    Ok(())
}

struct RnshChannelWriter {
    stream_id: u16,
    channel: TransportChannel,
    eof_sent: bool,
}

impl RnshChannelWriter {
    fn new(stream_id: u16, channel: TransportChannel) -> Result<Self, ChannelError> {
        StreamDataMessage::new(stream_id, Vec::new(), false, false)?;
        Ok(Self { stream_id, channel, eof_sent: false })
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<usize, ChannelError> {
        if self.eof_sent {
            return Ok(0);
        }
        let stream_mdu = self.channel.mdu().await?.saturating_sub(2);
        if stream_mdu == 0 {
            return Err(ChannelError::PayloadTooLarge);
        }
        let processed = bytes.len().min(stream_mdu).min(16 * 1024);
        let message =
            StreamDataMessage::new(self.stream_id, bytes[..processed].to_vec(), false, false)?;
        self.channel.open().await?;
        self.channel.send_typed(&message).await.map(|_| processed)
    }

    async fn close(&mut self) -> Result<(), ChannelError> {
        if self.eof_sent {
            return Ok(());
        }
        let message = StreamDataMessage::new(self.stream_id, Vec::new(), true, false)?;
        let deadline = Instant::now() + STREAM_WRITE_TIMEOUT;
        loop {
            match self.channel.open().await {
                Ok(()) => match self.channel.send_typed(&message).await {
                    Ok(_) => {
                        self.eof_sent = true;
                        return Ok(());
                    }
                    Err(ChannelError::LinkNotReady) if Instant::now() < deadline => {}
                    Err(error) => return Err(error),
                },
                Err(ChannelError::LinkNotReady) if Instant::now() < deadline => {}
                Err(error) => return Err(error),
            }
            sleep(Duration::from_millis(10)).await;
        }
    }
}

async fn send_typed_with_retry<M: TypedMessage>(
    channel: &TransportChannel,
    message: &M,
) -> io::Result<()> {
    let deadline = Instant::now() + STREAM_WRITE_TIMEOUT;
    loop {
        match channel.send_typed(message).await {
            Ok(_) => return Ok(()),
            Err(ChannelError::LinkNotReady) if Instant::now() < deadline => {
                sleep(Duration::from_millis(10)).await;
            }
            Err(error) => return Err(channel_error(error)),
        }
    }
}

fn channel_error(error: impl std::fmt::Debug) -> io::Error {
    io::Error::other(format!("remote shell channel error: {error:?}"))
}

fn join_error(error: tokio::task::JoinError) -> io::Error {
    io::Error::other(format!("remote shell stream task failed: {error}"))
}

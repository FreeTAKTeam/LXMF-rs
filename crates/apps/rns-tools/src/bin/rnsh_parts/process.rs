use crate::rnsh_parts::protocol::{CommandExitedMessage, StreamDataMessage};
use rns_transport::channel::{ChannelError, TypedMessage};
use rns_transport::transport::TransportChannel;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration, Instant};

const STREAM_STDIN: u16 = 0;
const STREAM_STDOUT: u16 = 1;
const STREAM_STDERR: u16 = 2;
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
}

pub(crate) async fn run_command(
    channel: TransportChannel,
    spec: CommandSpec,
    mut stdin_rx: mpsc::Receiver<Vec<u8>>,
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
        .stderr(if spec.pipe_stderr { Stdio::piped() } else { Stdio::null() });

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

    let status = child.wait().await?;
    if let Some(task) = stdin_task {
        task.abort();
    }
    if let Some(task) = stdout_task {
        task.await.map_err(join_error)??;
    }
    if let Some(task) = stderr_task {
        task.await.map_err(join_error)??;
    }

    let return_code = status.code().map(i64::from).unwrap_or(-1);
    send_typed_with_retry(&channel, &CommandExitedMessage { return_code }).await
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
        let written = writer.write(bytes).await.map_err(channel_error)?;
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

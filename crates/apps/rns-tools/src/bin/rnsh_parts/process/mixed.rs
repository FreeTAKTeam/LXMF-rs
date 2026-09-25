use super::{
    send_typed_with_retry, stop_task, stream_output, CommandSpec, STREAM_STDERR, STREAM_STDOUT,
};
use crate::rnsh_parts::protocol::{CommandExitedMessage, WindowSizeMessage};
use crate::rnsh_parts::pty::window_size;
use rns_transport::transport::TransportChannel;
use std::io;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

pub(super) async fn run_mixed_command(
    channel: TransportChannel,
    spec: CommandSpec,
    mut stdin_rx: mpsc::Receiver<Vec<u8>>,
    mut cancel: oneshot::Receiver<()>,
    mut resize_rx: mpsc::Receiver<WindowSizeMessage>,
) -> io::Result<()> {
    use rustix::fs::{open, Mode, OFlags};
    use rustix::io::{fcntl_setfd, FdFlags};
    use rustix::pty::{grantpt, openpt, ptsname, unlockpt, OpenptFlags};
    use rustix::termios::{tcsetwinsize, Winsize};

    let executable = spec
        .command
        .first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "remote command is empty"))?;
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).map_err(rustix_error)?;
    fcntl_setfd(&master, FdFlags::CLOEXEC).map_err(rustix_error)?;
    grantpt(&master).map_err(rustix_error)?;
    unlockpt(&master).map_err(rustix_error)?;
    let slave_name = ptsname(&master, Vec::new()).map_err(rustix_error)?;
    let slave = open(slave_name.as_c_str(), OFlags::RDWR | OFlags::NOCTTY, Mode::empty())
        .map_err(rustix_error)?;
    let size = window_size(spec.rows, spec.cols, spec.hpix, spec.vpix)?;
    tcsetwinsize(
        &master,
        Winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.pixel_width,
            ws_ypixel: size.pixel_height,
        },
    )
    .map_err(rustix_error)?;

    let mut process = Command::new(executable);
    process
        .args(&spec.command[1..])
        .current_dir(&spec.root)
        .env("TERM", spec.term.as_deref().unwrap_or("xterm"));
    if let Some(remote_identity) = spec.remote_identity.as_deref() {
        process.env("RNS_REMOTE_IDENTITY", remote_identity);
    }
    process
        .stdin(if spec.pipe_stdin { Stdio::piped() } else { Stdio::from(clone_fd(&slave)?) })
        .stdout(if spec.pipe_stdout { Stdio::piped() } else { Stdio::from(clone_fd(&slave)?) })
        .stderr(if spec.pipe_stderr { Stdio::piped() } else { Stdio::from(clone_fd(&slave)?) })
        .kill_on_drop(true);
    let mut child = process
        .spawn()
        .map_err(|error| io::Error::other(format!("unable to start remote command: {error}")))?;
    // Command retains the Stdio handles; release its PTY slave copies so the
    // master can observe EOF after the child closes its terminal descriptors.
    drop(process);
    drop(slave);

    let stdin_task = if spec.pipe_stdin {
        child.stdin.take().map(|mut stdin| {
            tokio::spawn(async move {
                while let Some(bytes) = stdin_rx.recv().await {
                    stdin.write_all(&bytes).await?;
                }
                stdin.shutdown().await
            })
        })
    } else {
        let mut pty_writer = tokio::fs::File::from_std(std::fs::File::from(clone_fd(&master)?));
        Some(tokio::spawn(async move {
            while let Some(bytes) = stdin_rx.recv().await {
                pty_writer.write_all(&bytes).await?;
            }
            pty_writer.write_all(&[0x04]).await?;
            pty_writer.flush().await
        }))
    };

    let mut output_tasks = Vec::new();
    if spec.pipe_stdout {
        if let Some(stdout) = child.stdout.take() {
            output_tasks.push(tokio::spawn(stream_output(channel.clone(), STREAM_STDOUT, stdout)));
        }
    }
    if spec.pipe_stderr {
        if let Some(stderr) = child.stderr.take() {
            output_tasks.push(tokio::spawn(stream_output(channel.clone(), STREAM_STDERR, stderr)));
        }
    }
    if !spec.pipe_stdout || !spec.pipe_stderr {
        let pty_stream = if !spec.pipe_stdout { STREAM_STDOUT } else { STREAM_STDERR };
        let reader = tokio::fs::File::from_std(std::fs::File::from(clone_fd(&master)?));
        output_tasks.push(tokio::spawn(stream_pty_output(channel.clone(), pty_stream, reader)));
    }

    let (status, cancelled) = loop {
        tokio::select! {
            status = child.wait() => {
                let status = status?;
                break (status, false)
            },
            _ = &mut cancel => {
                if let Err(error) = child.start_kill() {
                    if child.try_wait()?.is_none() {
                        return Err(error);
                    }
                }
                break (child.wait().await?, true);
            }
            resize = resize_rx.recv(), if !spec.pipe_stdin || !spec.pipe_stdout || !spec.pipe_stderr => {
                if let Some(resize) = resize {
                    let size = window_size(resize.rows, resize.cols, resize.hpix, resize.vpix)?;
                    tcsetwinsize(&master, Winsize {
                        ws_row: size.rows,
                        ws_col: size.cols,
                        ws_xpixel: size.pixel_width,
                        ws_ypixel: size.pixel_height,
                    }).map_err(rustix_error)?;
                }
            }
        }
    };
    stop_task(stdin_task).await;
    for task in output_tasks {
        if cancelled {
            task.abort();
        } else {
            task.await.map_err(super::join_error)??;
        }
    }
    let return_code = status.code().map(i64::from).unwrap_or(-1);
    send_typed_with_retry(&channel, &CommandExitedMessage { return_code }).await
}

async fn stream_pty_output<R: AsyncRead + Unpin>(
    channel: TransportChannel,
    stream_id: u16,
    mut input: R,
) -> io::Result<()> {
    let mut writer =
        super::RnshChannelWriter::new(stream_id, channel).map_err(super::channel_error)?;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        match input.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => super::write_all_with_retry(&mut writer, &buffer[..read]).await?,
            // Linux PTY masters report EIO after the final slave descriptor closes.
            Err(error) if error.raw_os_error() == Some(5) => break,
            Err(error) => return Err(error),
        }
    }
    writer.close().await.map_err(super::channel_error)
}

fn clone_fd(fd: &impl std::os::fd::AsFd) -> io::Result<std::os::fd::OwnedFd> {
    fd.as_fd().try_clone_to_owned()
}

fn rustix_error(error: rustix::io::Errno) -> io::Error {
    io::Error::from_raw_os_error(error.raw_os_error())
}

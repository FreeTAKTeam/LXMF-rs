const MAX_DYNAMIC_PERMISSION_OUTPUT: usize = 64 * 1024;
const MAX_DYNAMIC_TEMPLATE_OUTPUT: usize = 256 * 1024;
const DYNAMIC_EXECUTABLE_TIMEOUT: Duration = Duration::from_secs(2);

fn is_executable_file(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

#[cfg(not(unix))]
fn read_bounded(
    mut reader: impl Read,
    output_limit: usize,
    context: &'static str,
) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut exceeded = false;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = output_limit.saturating_sub(output.len());
        let copied = read.min(remaining);
        output.extend_from_slice(&buffer[..copied]);
        exceeded |= read > copied;
    }
    if exceeded {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{context} output exceeds {} KiB", output_limit / 1024),
        ))
    } else {
        Ok(output)
    }
}

#[cfg(unix)]
fn set_nonblocking(reader: &impl AsFd) -> io::Result<()> {
    let flags = OFlag::from_bits_truncate(fcntl(reader, FcntlArg::F_GETFL)?);
    let _ = fcntl(reader, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    Ok(())
}

#[cfg(unix)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PipeReadProgress {
    Pending,
    Data,
    Eof,
}

#[cfg(unix)]
fn read_nonblocking(
    reader: &mut impl Read,
    output: &mut Vec<u8>,
    output_limit: usize,
    exceeded: &mut bool,
) -> io::Result<PipeReadProgress> {
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(PipeReadProgress::Eof),
            Ok(read) => {
                let remaining = output_limit.saturating_sub(output.len());
                let copied = read.min(remaining);
                output.extend_from_slice(&buffer[..copied]);
                *exceeded |= read > copied;
                return Ok(PipeReadProgress::Data);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Ok(PipeReadProgress::Pending);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn spawn_bounded_executable(path: &Path) -> io::Result<Box<dyn StdChildWrapper>> {
    let retry_deadline = Instant::now() + Duration::from_millis(250);
    loop {
        let mut command = Command::new(path);
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut command = StdCommandWrap::from(command);
        #[cfg(unix)]
        command.wrap(ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(JobObject);
        let result = command.spawn();
        match result {
            Err(error)
                if error.kind() == io::ErrorKind::ExecutableFileBusy
                    && Instant::now() < retry_deadline =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            result => return result,
        }
    }
}

fn run_permission_resolver(path: &Path) -> io::Result<String> {
    run_bounded_executable(
        path,
        "permission resolver",
        true,
        MAX_DYNAMIC_PERMISSION_OUTPUT,
    )
}

fn run_dynamic_template(path: &Path) -> io::Result<String> {
    run_bounded_executable(path, "dynamic template", false, MAX_DYNAMIC_TEMPLATE_OUTPUT)
}

#[cfg(not(unix))]
fn terminate_bounded_executable(
    child: &mut dyn StdChildWrapper,
    stdout_reader: std::thread::JoinHandle<io::Result<Vec<u8>>>,
    stderr_reader: std::thread::JoinHandle<io::Result<Vec<u8>>>,
    context: &'static str,
) -> io::Error {
    let _ = child.start_kill();
    let _ = child.wait();
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    io::Error::new(io::ErrorKind::TimedOut, format!("{context} exceeded 2 second timeout"))
}

fn run_bounded_executable(
    path: &Path,
    context: &'static str,
    require_success: bool,
    output_limit: usize,
) -> io::Result<String> {
    let mut child = spawn_bounded_executable(path)?;
    let stdout = child
        .stdout()
        .take()
        .ok_or_else(|| io::Error::other(format!("{context} stdout unavailable")))?;
    let stderr = child
        .stderr()
        .take()
        .ok_or_else(|| io::Error::other(format!("{context} stderr unavailable")))?;

    #[cfg(unix)]
    let (mut stdout, mut stderr) = (stdout, stderr);

    #[cfg(unix)]
    {
        if let Err(error) = set_nonblocking(&stdout).and_then(|()| set_nonblocking(&stderr)) {
            let _ = child.start_kill();
            let _ = child.wait();
            return Err(error);
        }

        let deadline = Instant::now() + DYNAMIC_EXECUTABLE_TIMEOUT;
        let mut stdout_output = Vec::new();
        let mut stderr_output = Vec::new();
        let mut stdout_exceeded = false;
        let mut stderr_exceeded = false;
        let mut stdout_eof = false;
        let mut stderr_eof = false;

        loop {
            let mut made_progress = false;
            if !stdout_eof {
                match read_nonblocking(&mut stdout, &mut stdout_output, output_limit, &mut stdout_exceeded) {
                    Ok(PipeReadProgress::Eof) => stdout_eof = true,
                    Ok(PipeReadProgress::Data) => made_progress = true,
                    Ok(PipeReadProgress::Pending) => {}
                    Err(error) => {
                        let _ = child.start_kill();
                        let _ = child.wait();
                        return Err(error);
                    }
                }
            }
            if !stderr_eof {
                match read_nonblocking(&mut stderr, &mut stderr_output, output_limit, &mut stderr_exceeded) {
                    Ok(PipeReadProgress::Eof) => stderr_eof = true,
                    Ok(PipeReadProgress::Data) => made_progress = true,
                    Ok(PipeReadProgress::Pending) => {}
                    Err(error) => {
                        let _ = child.start_kill();
                        let _ = child.wait();
                        return Err(error);
                    }
                }
            }

            let status = match child.try_wait() {
                Ok(status) => status,
                Err(error) => {
                    let _ = child.start_kill();
                    let _ = child.wait();
                    return Err(error);
                }
            };
            if stdout_eof && stderr_eof {
                if let Some(status) = status {
                    if stdout_exceeded || stderr_exceeded {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("{context} output exceeds {} KiB", output_limit / 1024),
                        ));
                    }
                    return finish_bounded_output(stdout_output, stderr_output, status, require_success, context);
                }
            }
            if Instant::now() >= deadline {
                let _ = child.start_kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{context} exceeded 2 second timeout"),
                ));
            }
            if made_progress {
                std::thread::yield_now();
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    #[cfg(not(unix))]
    {
        let stdout_reader = std::thread::spawn(move || read_bounded(stdout, output_limit, context));
        let stderr_reader = std::thread::spawn(move || read_bounded(stderr, output_limit, context));
        let deadline = Instant::now() + DYNAMIC_EXECUTABLE_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                return Err(terminate_bounded_executable(child.as_mut(), stdout_reader, stderr_reader, context));
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        while !stdout_reader.is_finished() || !stderr_reader.is_finished() {
            if Instant::now() >= deadline {
                return Err(terminate_bounded_executable(child.as_mut(), stdout_reader, stderr_reader, context));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let stdout = stdout_reader
            .join()
            .map_err(|_| io::Error::other(format!("{context} stdout reader panicked")))??;
        let stderr = stderr_reader
            .join()
            .map_err(|_| io::Error::other(format!("{context} stderr reader panicked")))??;
        finish_bounded_output(stdout, stderr, status, require_success, context)
    }
}

fn finish_bounded_output(stdout: Vec<u8>, stderr: Vec<u8>, status: ExitStatus, require_success: bool, context: &'static str) -> io::Result<String> {
    if require_success && !status.success() {
        return Err(io::Error::other(format!(
            "{context} exited with {status}: {}",
            String::from_utf8_lossy(&stderr).trim()
        )));
    }
    String::from_utf8(stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{context} output is not UTF-8: {error}"),
        )
    })
}

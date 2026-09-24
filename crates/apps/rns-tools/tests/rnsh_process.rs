#![cfg(unix)]

use std::io::{self, Read};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

static RNSH_PROCESS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn free_port() -> io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn wait_for_port(port: u16, child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            drop(stream);
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!("rnsh listener exited early: {status}")));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "rnsh listener did not open its port"))
}

fn process_is_running(pid: &str) -> io::Result<bool> {
    Ok(Command::new("kill")
        .args(["-0", pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

fn wait_for_output(receiver: &Receiver<Vec<u8>>, needle: &[u8]) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut output = Vec::new();
    while Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => output.extend_from_slice(&bytes),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "local PTY output closed",
                ));
            }
        }
        if output.windows(needle.len()).any(|window| window == needle) {
            return Ok(output);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("timed out waiting for {:?} in local PTY output: {:?}", needle, output),
    ))
}

#[test]
fn rnsh_listener_pty_observes_initial_and_sigwinch_window_sizes() -> io::Result<()> {
    use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

    let _test_guard = RNSH_PROCESS_TEST_LOCK.lock().expect("rnsh process test lock poisoned");
    let temp = tempfile::tempdir()?;
    let server_root = temp.path().join("server-root");
    let server_identity = temp.path().join("server-identity");
    let client_identity = temp.path().join("client-identity");
    std::fs::create_dir_all(&server_root)?;
    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rnsh");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--announce", "0", "--no-auth", "--root"])
        .arg(&server_root)
        .args(["--identity", server_identity.to_str().expect("identity path")])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args([
                "--print-identity",
                "--identity",
                server_identity.to_str().expect("identity path"),
            ])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rnsh server identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rnsh identity query omitted destination"))?
            .to_owned();

        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize { rows: 31, cols: 101, pixel_width: 808, pixel_height: 496 })
            .map_err(|error| io::Error::other(format!("open client PTY: {error}")))?;
        let mut client_command = CommandBuilder::new(binary);
        client_command.args([
            "--connect",
            &format!("127.0.0.1:{port}"),
            "--identity",
            client_identity.to_str().expect("identity path"),
            "--mirror",
            "--timeout",
            "4",
            &destination,
            "--",
            "/bin/sh",
            "-c",
            "test -t 0 && test -t 1 && test -t 2 && stty size; read _; stty size",
        ]);
        client_command.cwd(temp.path());
        let mut client = pair
            .slave
            .spawn_command(client_command)
            .map_err(|error| io::Error::other(format!("spawn rnsh client under PTY: {error}")))?;
        drop(pair.slave);
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|error| io::Error::other(format!("open client PTY writer: {error}")))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| io::Error::other(format!("open client PTY reader: {error}")))?;
        let (output_tx, output_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => return,
                    Ok(read) if output_tx.send(buffer[..read].to_vec()).is_err() => return,
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => return,
                }
            }
        });

        let initial_output = wait_for_output(&output_rx, b"31 101")?;
        assert!(String::from_utf8_lossy(&initial_output).contains("31 101"), "{initial_output:?}");
        pair.master
            .resize(PtySize { rows: 42, cols: 120, pixel_width: 960, pixel_height: 672 })
            .map_err(|error| io::Error::other(format!("resize client PTY: {error}")))?;
        thread::sleep(Duration::from_millis(250));
        writer.write_all(b"continue\n")?;
        writer.flush()?;
        let resized_output = wait_for_output(&output_rx, b"42 120")?;
        assert!(String::from_utf8_lossy(&resized_output).contains("42 120"), "{resized_output:?}");
        // EOF the local terminal input so Tokio's blocking stdin reader can
        // leave its read after the remote command has reported completion.
        writer.write_all(&[0x04])?;
        writer.flush()?;

        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = client.try_wait()? {
                if !status.success() {
                    return Err(io::Error::other(format!(
                        "rnsh client failed under PTY: {status}"
                    )));
                }
                break;
            }
            if Instant::now() >= deadline {
                client.kill()?;
                let _ = client.wait();
                let mut tail = Vec::new();
                for bytes in output_rx.try_iter() {
                    tail.extend_from_slice(&bytes);
                }
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "rnsh PTY client did not exit; terminal tail: {:?}",
                        String::from_utf8_lossy(&tail)
                    ),
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    if result.is_err() {
        let mut stderr = String::new();
        if let Some(mut pipe) = listener.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        eprintln!("rnsh PTY listener stderr: {stderr}");
    }
    result
}

#[test]
fn rnsh_pty_link_timeout_kills_and_reaps_remote_child() -> io::Result<()> {
    use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

    let _test_guard = RNSH_PROCESS_TEST_LOCK.lock().expect("rnsh process test lock poisoned");
    let temp = tempfile::tempdir()?;
    let server_root = temp.path().join("server-root");
    let server_identity = temp.path().join("server-identity");
    let client_identity = temp.path().join("client-identity");
    let child_pid_path = temp.path().join("remote-child.pid");
    std::fs::create_dir_all(&server_root)?;
    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rnsh");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--announce", "0", "--no-auth", "--root"])
        .arg(&server_root)
        .args(["--identity", server_identity.to_str().expect("identity path")])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args([
                "--print-identity",
                "--identity",
                server_identity.to_str().expect("identity path"),
            ])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rnsh server identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rnsh identity query omitted destination"))?
            .to_owned();
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .map_err(|error| io::Error::other(format!("open client PTY: {error}")))?;
        let command = format!("echo $$ > '{}'; exec sleep 30", child_pid_path.display());
        let mut client_command = CommandBuilder::new(binary);
        client_command.args([
            "--connect",
            &format!("127.0.0.1:{port}"),
            "--identity",
            client_identity.to_str().expect("identity path"),
            "--mirror",
            "--timeout",
            "4",
            &destination,
            "--",
            "/bin/sh",
            "-c",
            &command,
        ]);
        client_command.cwd(temp.path());
        let mut client = pair
            .slave
            .spawn_command(client_command)
            .map_err(|error| io::Error::other(format!("spawn rnsh client under PTY: {error}")))?;
        drop(pair.slave);
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|error| io::Error::other(format!("open client PTY writer: {error}")))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| io::Error::other(format!("open client PTY reader: {error}")))?;
        thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        });

        let start_deadline = Instant::now() + Duration::from_secs(5);
        let pid = loop {
            if let Ok(contents) = std::fs::read_to_string(&child_pid_path) {
                let pid = contents.trim();
                if !pid.is_empty() && process_is_running(pid)? {
                    break pid.to_owned();
                }
            }
            if Instant::now() >= start_deadline {
                let _ = client.kill();
                let _ = client.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "remote PTY command did not become live before client timeout",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        };
        // Release Tokio's local blocking stdin reader; the remote command
        // remains alive because it is sleeping rather than reading its PTY.
        writer.write_all(&[0x04])?;
        writer.flush()?;

        let client_deadline = Instant::now() + Duration::from_secs(8);
        let mut client_status = None;
        while Instant::now() < client_deadline {
            if let Some(status) = client.try_wait()? {
                client_status = Some(status);
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        if client_status.is_none() {
            client.kill()?;
            let _ = client.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "rnsh PTY client did not time out",
            ));
        }
        assert!(!client_status.expect("status checked above").success());

        let reap_deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < reap_deadline {
            if !process_is_running(&pid)? {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("remote PTY command child {pid} remained alive after Link teardown"),
        ))
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    if result.is_err() {
        let mut stderr = String::new();
        if let Some(mut pipe) = listener.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        eprintln!("rnsh PTY cancellation listener stderr: {stderr}");
    }
    result
}

#[test]
fn rnsh_executes_a_remote_command_and_forwards_output_cross_process() -> io::Result<()> {
    let _test_guard = RNSH_PROCESS_TEST_LOCK.lock().expect("rnsh process test lock poisoned");
    let temp = tempfile::tempdir()?;
    let server_root = temp.path().join("server-root");
    let server_identity = temp.path().join("server-identity");
    let client_identity = temp.path().join("client-identity");
    std::fs::create_dir_all(&server_root)?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rnsh");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--announce", "0", "--no-auth", "--root"])
        .arg(&server_root)
        .args(["--identity", server_identity.to_str().expect("identity path")])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args([
                "--print-identity",
                "--identity",
                server_identity.to_str().expect("identity path"),
            ])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other(format!(
                "rnsh identity query failed: {}",
                String::from_utf8_lossy(&destination_output.stderr)
            )));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rnsh identity query omitted destination"))?
            .to_owned();

        let client = Command::new(binary)
            .args([
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--identity",
                client_identity.to_str().expect("identity path"),
                "--mirror",
                "--timeout",
                "15",
                &destination,
                "--",
                "/bin/echo",
                "rnsh-cross-process",
            ])
            .current_dir(temp.path())
            .output()?;
        if !client.status.success() {
            return Err(io::Error::other(format!(
                "rnsh client failed: {}\nstdout:\n{}\nstderr:\n{}",
                client.status,
                String::from_utf8_lossy(&client.stdout),
                String::from_utf8_lossy(&client.stderr)
            )));
        }
        assert!(
            String::from_utf8_lossy(&client.stdout).contains("rnsh-cross-process\n"),
            "rnsh client did not preserve command output: {}",
            String::from_utf8_lossy(&client.stdout)
        );

        let large_output = Command::new(binary)
            .args([
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--identity",
                client_identity.to_str().expect("identity path"),
                "--mirror",
                "--timeout",
                "30",
                &destination,
                "--",
                "head",
                "-c",
                "131072",
                "/dev/zero",
            ])
            .current_dir(temp.path())
            .output()?;
        if !large_output.status.success() {
            return Err(io::Error::other(format!(
                "rnsh large-output client failed: {}\nstdout length: {}\nstderr:\n{}",
                large_output.status,
                large_output.stdout.len(),
                String::from_utf8_lossy(&large_output.stderr)
            )));
        }
        assert!(
            large_output.stdout.len() >= 131_072,
            "rnsh large-output client returned too little output: {}",
            large_output.stdout.len()
        );
        Ok(())
    })();

    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rnsh_authenticated_listener_accepts_allowlisted_identity_and_rejects_another() -> io::Result<()>
{
    let _test_guard = RNSH_PROCESS_TEST_LOCK.lock().expect("rnsh process test lock poisoned");
    let temp = tempfile::tempdir()?;
    let server_root = temp.path().join("server-root");
    let server_identity = temp.path().join("server-identity");
    let client_identity = temp.path().join("client-identity");
    let denied_identity = temp.path().join("denied-identity");
    std::fs::create_dir_all(&server_root)?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rnsh");
    let client_identity_output = Command::new(binary)
        .args(["--print-identity", "--identity", client_identity.to_str().expect("identity path")])
        .output()?;
    if !client_identity_output.status.success() {
        return Err(io::Error::other("rnsh client identity query failed"));
    }
    let allowed_identity = String::from_utf8_lossy(&client_identity_output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Identity     : "))
        .ok_or_else(|| io::Error::other("rnsh identity query omitted identity hash"))?
        .to_owned();

    let mut listener = Command::new(binary)
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--announce",
            "0",
            "--allow",
            &allowed_identity,
            "--root",
        ])
        .arg(&server_root)
        .args(["--identity", server_identity.to_str().expect("identity path")])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args([
                "--print-identity",
                "--identity",
                server_identity.to_str().expect("identity path"),
            ])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rnsh server identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rnsh server identity query omitted destination"))?
            .to_owned();

        let allowed = Command::new(binary)
            .args([
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--identity",
                client_identity.to_str().expect("identity path"),
                "--mirror",
                "--timeout",
                "15",
                &destination,
                "--",
                "/bin/echo",
                "allowlisted",
            ])
            .current_dir(temp.path())
            .output()?;
        if !allowed.status.success() {
            return Err(io::Error::other(format!(
                "allowlisted rnsh client failed: {}\nstdout:\n{}\nstderr:\n{}",
                allowed.status,
                String::from_utf8_lossy(&allowed.stdout),
                String::from_utf8_lossy(&allowed.stderr)
            )));
        }
        assert!(String::from_utf8_lossy(&allowed.stdout).contains("allowlisted\n"));

        let denied = Command::new(binary)
            .args([
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--identity",
                denied_identity.to_str().expect("identity path"),
                "--mirror",
                "--timeout",
                "3",
                &destination,
                "--",
                "/bin/echo",
                "denied",
            ])
            .current_dir(temp.path())
            .output()?;
        assert!(!denied.status.success(), "non-allowlisted rnsh client succeeded");
        Ok(())
    })();

    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rnsh_client_timeout_closes_session_and_reaps_remote_command() -> io::Result<()> {
    let _test_guard = RNSH_PROCESS_TEST_LOCK.lock().expect("rnsh process test lock poisoned");
    let temp = tempfile::tempdir()?;
    let server_root = temp.path().join("server-root");
    let server_identity = temp.path().join("server-identity");
    let client_identity = temp.path().join("client-identity");
    let child_pid_path = temp.path().join("remote-child.pid");
    std::fs::create_dir_all(&server_root)?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rnsh");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--announce", "0", "--no-auth", "--root"])
        .arg(&server_root)
        .args(["--identity", server_identity.to_str().expect("identity path")])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        if let Err(error) = wait_for_port(port, &mut listener) {
            let mut stderr = String::new();
            if let Some(mut pipe) = listener.stderr.take() {
                pipe.read_to_string(&mut stderr)?;
            }
            return Err(io::Error::other(format!("{error}; listener stderr: {}", stderr)));
        }
        let destination_output = Command::new(binary)
            .args([
                "--print-identity",
                "--identity",
                server_identity.to_str().expect("identity path"),
            ])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rnsh server identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rnsh identity query omitted destination"))?
            .to_owned();

        let command = format!("echo $$ > '{}'; exec sleep 30", child_pid_path.display());
        let mut client = Command::new(binary)
            .args([
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--identity",
                client_identity.to_str().expect("identity path"),
                "--mirror",
                "--timeout",
                "4",
                &destination,
                "--",
                "sh",
                "-c",
                &command,
            ])
            .current_dir(temp.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Observe the remote command while its client is still connected so
        // the later PID check proves this exact child was alive before timeout.
        let start_deadline = Instant::now() + Duration::from_secs(3);
        let started_pid = loop {
            if let Ok(contents) = std::fs::read_to_string(&child_pid_path) {
                let pid = contents.trim();
                if !pid.is_empty() && process_is_running(pid)? {
                    break Ok(pid.to_owned());
                }
            }
            if client.try_wait()?.is_some() {
                break Err(io::Error::other(
                    "rnsh client exited before remote command became live",
                ));
            }
            if Instant::now() >= start_deadline {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "remote command did not become live before the client timeout",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        };
        let pid = match started_pid {
            Ok(pid) => pid,
            Err(error) => {
                let _ = client.kill();
                let output = client.wait_with_output()?;
                return Err(io::Error::other(format!(
                    "{error}; timeout client status: {}",
                    output.status
                )));
            }
        };

        let client_output = client.wait_with_output()?;
        assert!(!client_output.status.success(), "timeout client unexpectedly succeeded");

        let reap_deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < reap_deadline {
            if !process_is_running(&pid)? {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("remote command child {pid} remained alive after Link teardown"),
        ))
    })();

    let _ = listener.kill();
    let _ = listener.wait();
    result
}

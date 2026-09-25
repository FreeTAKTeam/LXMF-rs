use std::fs;
use std::io::{self, BufRead, BufReader};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[path = "support/slow_tcp_proxy.rs"]
mod slow_tcp_proxy;

use slow_tcp_proxy::{SlowTcpProxy, SLOW_PATH_FIRST_RESPONSE_DELAY};

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
            return Err(io::Error::other(format!("rncp listener exited early: {status}")));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "rncp listener did not open its port"))
}

fn run_client_output(
    source: &Path,
    destination: &str,
    port: u16,
    cwd: &Path,
    identity_seed: &str,
    silent: bool,
) -> io::Result<std::process::Output> {
    let binary = env!("CARGO_BIN_EXE_rncp");
    let source = source.to_string_lossy();
    let mut command = Command::new(binary);
    command
        .arg(source.as_ref())
        .arg(destination)
        .args(["--connect", &format!("127.0.0.1:{port}"), "--no-compress"])
        .arg("--identity-seed")
        .arg(identity_seed)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if silent {
        command.arg("--silent");
    }
    command.output()
}

fn run_client(source: &Path, destination: &str, port: u16, cwd: &Path) -> io::Result<()> {
    let output = run_client_output(source, destination, port, cwd, "rncp-process-client", true)?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "rncp client failed: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

#[test]
fn rncp_send_and_fetch_cross_process_with_binary_data() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("client");
    let fetch_root = temp.path().join("fetch");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    fs::create_dir_all(&fetch_root)?;
    let source = client_root.join("payload.bin");
    let payload = (0..8192).map(|index| (index as u8).wrapping_mul(37)).collect::<Vec<_>>();
    fs::write(&source, &payload)?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let mut listener = Command::new(binary)
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--allow-fetch",
            "--no-auth",
            "--no-compress",
            "--save",
        ])
        .arg(&listener_root)
        .args(["--identity-seed", "rncp-process-server", "--silent"])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args(["--print-identity", "--identity-seed", "rncp-process-server"])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rncp identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rncp identity query omitted destination"))?
            .to_owned();

        run_client(&source, &destination, port, &client_root)?;
        let received = listener_root.join("payload.bin");
        assert_eq!(fs::read(&received)?, payload);

        let fetched = Command::new(binary)
            .arg(&received)
            .arg(&destination)
            .args(["--fetch", "--connect", &format!("127.0.0.1:{port}"), "--no-compress", "--save"])
            .arg(&fetch_root)
            .args(["--identity-seed", "rncp-process-fetch-client", "--silent"])
            .current_dir(&client_root)
            .output()?;
        if !fetched.status.success() {
            return Err(io::Error::other(format!(
                "rncp fetch failed: {}\nstdout:\n{}\nstderr:\n{}",
                fetched.status,
                String::from_utf8_lossy(&fetched.stdout),
                String::from_utf8_lossy(&fetched.stderr)
            )));
        }
        assert_eq!(fs::read(fetch_root.join("payload.bin"))?, payload);

        let missing = Command::new(binary)
            .arg("missing.bin")
            .arg(&destination)
            .args([
                "--fetch",
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--silent",
                "--timeout",
                "5",
                "--save",
            ])
            .arg(&fetch_root)
            .args(["--identity-seed", "rncp-process-missing-fetch"])
            .current_dir(&client_root)
            .output()?;
        assert!(!missing.status.success(), "missing fetch unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&missing.stderr).contains("remote file was not found"),
            "missing fetch stderr did not preserve the not-found category: {}",
            String::from_utf8_lossy(&missing.stderr)
        );
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rncp_denied_sender_reports_nonzero_status() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("client");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    let source = client_root.join("payload.bin");
    fs::write(&source, b"denied sender payload")?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--save"])
        .arg(&listener_root)
        .args(["--identity-seed", "rncp-process-denied-server", "--silent", "--timeout", "5"])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args(["--print-identity", "--identity-seed", "rncp-process-denied-server"])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rncp identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rncp identity query omitted destination"))?
            .to_owned();
        let output = run_client_output(
            &source,
            &destination,
            port,
            &client_root,
            "rncp-process-denied-client",
            true,
        )?;
        assert!(!output.status.success(), "unauthorised sender unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Resource transfer failed"),
            "denied sender stderr did not preserve the terminal failure: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!listener_root.join("payload.bin").exists());
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rncp_fetch_jail_escape_reports_denial_without_creating_output() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let jail_root = temp.path().join("jail");
    let client_root = temp.path().join("client");
    let fetch_root = temp.path().join("fetch");
    fs::create_dir_all(&jail_root)?;
    fs::create_dir_all(&client_root)?;
    fs::create_dir_all(&fetch_root)?;
    fs::write(temp.path().join("outside-secret.bin"), b"must not escape fetch jail")?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--allow-fetch", "--no-auth", "--jail"])
        .arg(&jail_root)
        .args(["--identity-seed", "rncp-process-fetch-jail", "--silent"])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args(["--print-identity", "--identity-seed", "rncp-process-fetch-jail"])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rncp identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rncp identity query omitted destination"))?
            .to_owned();
        let fetched = Command::new(binary)
            .arg("../outside-secret.bin")
            .arg(destination)
            .args(["--fetch", "--connect", &format!("127.0.0.1:{port}"), "--save"])
            .arg(&fetch_root)
            .args([
                "--no-compress",
                "--silent",
                "--identity-seed",
                "rncp-process-fetch-jail-client",
            ])
            .current_dir(&client_root)
            .output()?;

        assert!(!fetched.status.success(), "fetch outside the jail unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&fetched.stderr).contains("remote fetch was not allowed"),
            "jail denial category missing from stderr: {}",
            String::from_utf8_lossy(&fetched.stderr)
        );
        assert_eq!(
            fs::read(temp.path().join("outside-secret.bin"))?,
            b"must not escape fetch jail"
        );
        assert_eq!(fs::read_dir(&fetch_root)?.count(), 0, "denied fetch created an output file");
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rncp_rejects_invalid_identity_and_unusable_save_path() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let invalid_identity = Command::new(binary)
        .args([
            "--connect",
            "127.0.0.1:1",
            "--identity-seed",
            "rncp-process-invalid-identity",
            "--allowed-identity",
            "not-a-reticulum-hash",
        ])
        .output()?;
    assert!(!invalid_identity.status.success());
    assert!(
        String::from_utf8_lossy(&invalid_identity.stderr)
            .contains("destination must be a 32-character hex hash"),
        "invalid identity stderr: {}",
        String::from_utf8_lossy(&invalid_identity.stderr)
    );

    let save_file = temp.path().join("save-file");
    fs::write(&save_file, b"not a directory")?;
    let unusable_save = Command::new(binary)
        .args(["--listen", "127.0.0.1:0", "--identity-seed", "rncp-process-save-file", "--save"])
        .arg(&save_file)
        .output()?;
    assert_eq!(unusable_save.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&unusable_save.stderr).contains("Output directory not found"),
        "unusable save stderr: {}",
        String::from_utf8_lossy(&unusable_save.stderr)
    );
    Ok(())
}

#[test]
fn rncp_missing_save_directory_uses_reference_failure_status() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let missing_save = temp.path().join("missing-save-directory");
    let output = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .args(["--listen", "127.0.0.1:0", "--save"])
        .arg(&missing_save)
        .output()?;

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "rncp: Output directory not found\n");
    assert!(!missing_save.exists());
    Ok(())
}

#[test]
fn rncp_reports_path_discovery_timeout() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("payload.bin");
    fs::write(&source, b"path discovery timeout payload")?;
    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let destination = "00000000000000000000000000000000";
    let output = Command::new(binary)
        .arg(&source)
        .arg(destination)
        .args([
            "--connect",
            &format!("127.0.0.1:{port}"),
            "--no-compress",
            "--timeout",
            "1",
            "--identity-seed",
            "rncp-process-timeout",
        ])
        .current_dir(temp.path())
        .output()?;

    assert_eq!(output.status.code(), Some(1), "path discovery status");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("Path to {destination} requested\n"),
        "unexpected path-discovery progress output"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "rncp: path discovery timed out\n",
        "unexpected path-discovery failure output"
    );
    Ok(())
}

#[test]
fn rncp_missing_destination_reports_cli_failure() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("payload.bin");
    fs::write(&source, b"missing destination fixture")?;
    let acceptor = TcpListener::bind("127.0.0.1:0")?;
    acceptor.set_nonblocking(true)?;
    let address = acceptor.local_addr()?;

    let mut child = Command::new(env!("CARGO_BIN_EXE_rncp"))
        .arg(&source)
        .args(["--connect", &address.to_string(), "--identity-seed", "rncp-missing-destination"])
        .current_dir(temp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let accept_deadline = Instant::now() + Duration::from_secs(5);
    let stream = loop {
        match acceptor.accept() {
            Ok((stream, _)) => break Some(stream),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if child.try_wait()?.is_some() {
                    break None;
                }
                if Instant::now() >= accept_deadline {
                    if let Err(error) = child.kill() {
                        if error.kind() != io::ErrorKind::InvalidInput {
                            return Err(error);
                        }
                    }
                    child.wait()?;
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "rncp did not connect to its local TCP interface",
                    ));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    };
    let output = child.wait_with_output()?;
    drop(stream);

    assert_eq!(output.status.code(), Some(1), "missing destination exit status");
    assert!(
        output.stdout.is_empty(),
        "unexpected stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "rncp: missing destination hash\n");
    assert_eq!(fs::read(&source)?, b"missing destination fixture");
    Ok(())
}

#[test]
fn rncp_listener_restart_preserves_identity_and_transfer() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("client");
    let identity = temp.path().join("listener.identity");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    let before = client_root.join("before-restart.bin");
    let after = client_root.join("after-restart.bin");
    fs::write(&before, (0..4096).map(|index| (index as u8).wrapping_mul(11)).collect::<Vec<_>>())?;
    fs::write(&after, (0..4096).map(|index| (index as u8).wrapping_mul(29)).collect::<Vec<_>>())?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let start_listener = || {
        Command::new(binary)
            .args(["--listen", &format!("127.0.0.1:{port}"), "--no-auth", "--save"])
            .arg(&listener_root)
            .args(["--identity", identity.to_str().expect("identity path is UTF-8"), "--silent"])
            .current_dir(temp.path())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
    };
    let destination_for = || -> io::Result<String> {
        let output = Command::new(binary)
            .args(["--print-identity", "--identity"])
            .arg(&identity)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other("rncp identity query failed"));
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .map(str::to_owned)
            .ok_or_else(|| io::Error::other("rncp identity query omitted destination"))
    };

    let mut listener = start_listener()?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination = destination_for()?;
        run_client(&before, &destination, port, &client_root)?;
        assert_eq!(fs::read(listener_root.join("before-restart.bin"))?, fs::read(&before)?);

        listener.kill()?;
        listener.wait()?;

        let mut restarted = start_listener()?;
        let restarted_result = (|| {
            wait_for_port(port, &mut restarted)?;
            assert_eq!(destination_for()?, destination);
            run_client(&after, &destination, port, &client_root)?;
            assert_eq!(fs::read(listener_root.join("after-restart.bin"))?, fs::read(&after)?);
            Ok(())
        })();
        let _ = restarted.kill();
        let _ = restarted.wait();
        restarted_result
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rncp_fetch_reports_destination_disk_error() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("client");
    let fetch_root = temp.path().join("fetch");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    fs::create_dir_all(&fetch_root)?;
    let remote = listener_root.join("disk-error.bin");
    fs::write(&remote, b"disk error payload")?;
    fs::create_dir(fetch_root.join("disk-error.bin"))?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--allow-fetch", "--no-auth", "--save"])
        .arg(&listener_root)
        .args(["--identity-seed", "rncp-process-disk-error", "--silent"])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args(["--print-identity", "--identity-seed", "rncp-process-disk-error"])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rncp identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .map(str::to_owned)
            .ok_or_else(|| io::Error::other("rncp identity query omitted destination"))?;
        let fetched = Command::new(binary)
            .arg(&remote)
            .arg(destination)
            .args(["--fetch", "--connect", &format!("127.0.0.1:{port}"), "--save"])
            .arg(&fetch_root)
            .args([
                "--overwrite",
                "--no-compress",
                "--silent",
                "--identity-seed",
                "rncp-process-disk-error-client",
            ])
            .current_dir(&client_root)
            .output()?;
        assert!(!fetched.status.success(), "disk-error fetch unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&fetched.stderr).contains("Is a directory"),
            "disk-error fetch stderr did not preserve the OS failure: {}",
            String::from_utf8_lossy(&fetched.stderr)
        );
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rncp_listener_reports_received_file_disk_error() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("client");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    let source = client_root.join("receiver-disk-error.bin");
    fs::write(&source, b"receiver disk error payload")?;
    // A directory at the incoming filename forces the listener's completed
    // Resource save callback to fail without relying on permission semantics.
    fs::create_dir(listener_root.join("receiver-disk-error.bin"))?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--no-auth", "--overwrite", "--save"])
        .arg(&listener_root)
        .args(["--identity-seed", "rncp-process-receiver-disk-error"])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = listener
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("rncp listener stderr was not captured"))?;
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if stderr_tx.send(line).is_err() {
                break;
            }
        }
    });

    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args(["--print-identity", "--identity-seed", "rncp-process-receiver-disk-error"])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rncp identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("rncp identity query omitted destination"))?
            .to_owned();

        // Resource delivery can succeed even though the application-level
        // save fails; the listener must surface that distinct outcome.
        let sender = run_client_output(
            &source,
            &destination,
            port,
            &client_root,
            "rncp-process-receiver-disk-error-client",
            false,
        )?;
        assert!(
            sender.status.success(),
            "Resource transfer failed: {}",
            String::from_utf8_lossy(&sender.stderr)
        );
        let sender_stdout = String::from_utf8_lossy(&sender.stdout);
        assert!(
            sender_stdout.contains("sent to"),
            "sender overstated remote persistence: {sender_stdout}"
        );
        assert!(
            !sender_stdout.contains("copied to"),
            "sender claimed a remote save it cannot verify: {sender_stdout}"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "listener did not report the received-file save failure",
                ));
            }
            match stderr_rx.recv_timeout(remaining) {
                Ok(line) if line.contains("rncp: could not save received file:") => break,
                Ok(_) => {}
                Err(error) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("listener save-failure diagnostic was not observed: {error}"),
                    ));
                }
            }
        }
        assert!(listener_root.join("receiver-disk-error.bin").is_dir());
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[cfg(unix)]
#[test]
fn rncp_ctrl_c_reports_cancellation() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("cancel.bin");
    fs::write(&source, b"cancel payload")?;
    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let client = Command::new(binary)
        .arg(&source)
        .arg("00000000000000000000000000000000")
        .args([
            "--connect",
            &format!("127.0.0.1:{port}"),
            "--timeout",
            "30",
            "--silent",
            "--identity-seed",
            "rncp-process-cancel",
        ])
        .current_dir(temp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    thread::sleep(Duration::from_millis(100));
    let signal = Command::new("kill").args(["-INT", &client.id().to_string()]).status()?;
    assert!(signal.success(), "failed to send SIGINT to rncp client");
    let output = client.wait_with_output()?;
    assert!(!output.status.success(), "cancelled rncp client unexpectedly succeeded");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("operation cancelled by user"),
        "cancelled rncp stderr did not preserve the cancellation category: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn rncp_listener_accepts_multiple_concurrent_clients() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("clients");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--no-auth", "--no-compress", "--save"])
        .arg(&listener_root)
        .args(["--identity-seed", "rncp-process-multi-server", "--silent"])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args(["--print-identity", "--identity-seed", "rncp-process-multi-server"])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rncp identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .map(str::to_owned)
            .ok_or_else(|| io::Error::other("rncp identity query omitted destination"))?;

        let mut clients = Vec::new();
        let mut payloads = Vec::new();
        for index in 0..3u8 {
            let source = client_root.join(format!("concurrent-{index}.bin"));
            let payload = (0..8192)
                .map(|offset| (offset as u8).wrapping_mul(index.wrapping_add(3)))
                .collect::<Vec<_>>();
            fs::write(&source, &payload)?;
            payloads.push((source.clone(), payload));
            clients.push(
                Command::new(binary)
                    .arg(&source)
                    .arg(&destination)
                    .args([
                        "--connect",
                        &format!("127.0.0.1:{port}"),
                        "--no-compress",
                        "--silent",
                        "--identity-seed",
                    ])
                    .arg(format!("rncp-process-multi-client-{index}"))
                    .current_dir(&client_root)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()?,
            );
        }

        for (client, (source, payload)) in clients.into_iter().zip(payloads) {
            let output = client.wait_with_output()?;
            if !output.status.success() {
                return Err(io::Error::other(format!(
                    "concurrent rncp client for {} failed: {}\nstdout:\n{}\nstderr:\n{}",
                    source.display(),
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            assert_eq!(
                fs::read(listener_root.join(source.file_name().expect("source name")))?,
                payload
            );
        }
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rncp_interrupted_link_reports_resource_failure() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("client");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    let source = client_root.join("interrupted.bin");
    let payload = (0..(16 * 1024 * 1024))
        .map(|index| (index as u8).wrapping_mul(53).wrapping_add(17))
        .collect::<Vec<_>>();
    fs::write(&source, &payload)?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--no-auth", "--no-compress", "--save"])
        .arg(&listener_root)
        .args(["--identity-seed", "rncp-process-interrupted-server", "--silent"])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args(["--print-identity", "--identity-seed", "rncp-process-interrupted-server"])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rncp identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .map(str::to_owned)
            .ok_or_else(|| io::Error::other("rncp identity query omitted destination"))?;

        let mut client = Command::new(binary)
            .arg(&source)
            .arg(&destination)
            .args([
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--no-compress",
                "--timeout",
                "30",
                "--identity-seed",
                "rncp-process-interrupted-client",
            ])
            .current_dir(&client_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = client
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("rncp client stdout was not captured"))?;
        let (phase_tx, phase_rx) = mpsc::channel();
        let stdout_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
            let mut reader = BufReader::new(stdout);
            let mut captured = Vec::new();
            loop {
                let mut line = String::new();
                let read = reader.read_line(&mut line)?;
                if read == 0 {
                    break;
                }
                if line.contains("Transferring file...") {
                    let _ = phase_tx.send(());
                }
                captured.extend_from_slice(line.as_bytes());
            }
            Ok(captured)
        });
        if phase_rx.recv_timeout(Duration::from_secs(20)).is_err() {
            let _ = client.kill();
            let _ = client.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "rncp client did not reach the transferring phase",
            ));
        }

        listener.kill()?;
        listener.wait()?;
        let output = client.wait_with_output()?;
        let stdout =
            stdout_reader.join().map_err(|_| io::Error::other("rncp stdout reader panicked"))??;
        assert!(!output.status.success(), "interrupted rncp client unexpectedly succeeded");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Resource transfer failed") || stderr.contains("Resource transfer timed out"),
            "interrupted rncp stderr did not preserve a resource failure category: {stderr}\nstdout:\n{}",
            String::from_utf8_lossy(&stdout)
        );
        assert!(!listener_root.join("interrupted.bin").exists());
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[cfg(unix)]
#[test]
fn rncp_ctrl_c_during_resource_transfer_reports_cancellation() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("client");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    let source = client_root.join("cancel-during-transfer.bin");
    let payload = (0..(16 * 1024 * 1024))
        .map(|index| (index as u8).wrapping_mul(61).wrapping_add(23))
        .collect::<Vec<_>>();
    fs::write(&source, payload)?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--no-auth", "--no-compress", "--save"])
        .arg(&listener_root)
        .args(["--identity-seed", "rncp-process-cancel-transfer-server", "--silent"])
        .current_dir(temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let destination_output = Command::new(binary)
            .args(["--print-identity", "--identity-seed", "rncp-process-cancel-transfer-server"])
            .output()?;
        if !destination_output.status.success() {
            return Err(io::Error::other("rncp transfer-cancellation identity query failed"));
        }
        let destination = String::from_utf8_lossy(&destination_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .map(str::to_owned)
            .ok_or_else(|| io::Error::other("rncp transfer-cancellation identity was omitted"))?;
        let mut client = Command::new(binary)
            .arg(&source)
            .arg(&destination)
            .args([
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--no-compress",
                "--timeout",
                "30",
                "--identity-seed",
                "rncp-process-cancel-transfer-client",
            ])
            .current_dir(&client_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = client
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("rncp cancellation client stdout was not captured"))?;
        let (phase_tx, phase_rx) = mpsc::channel();
        let stdout_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
            let mut reader = BufReader::new(stdout);
            let mut captured = Vec::new();
            loop {
                let mut line = String::new();
                let read = reader.read_line(&mut line)?;
                if read == 0 {
                    break;
                }
                if line.contains("Transferring file...") {
                    let _ = phase_tx.send(());
                }
                captured.extend_from_slice(line.as_bytes());
            }
            Ok(captured)
        });
        if phase_rx.recv_timeout(Duration::from_secs(20)).is_err() {
            let _ = client.kill();
            let _ = client.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "rncp client did not begin the Resource transfer before cancellation",
            ));
        }

        let signal = Command::new("kill").args(["-INT", &client.id().to_string()]).status()?;
        if !signal.success() {
            let _ = client.kill();
            let _ = client.wait();
            return Err(io::Error::other("failed to send SIGINT during rncp Resource transfer"));
        }
        let output = client.wait_with_output()?;
        let stdout = stdout_reader
            .join()
            .map_err(|_| io::Error::other("rncp cancellation stdout reader panicked"))??;
        assert!(!output.status.success(), "cancelled rncp transfer unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("operation cancelled by user"),
            "active-transfer cancellation did not preserve its error category: {}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&stdout)
        );
        assert!(
            !listener_root.join("cancel-during-transfer.bin").exists(),
            "receiver exposed a file for the cancelled Resource"
        );
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rncp_uses_medium_timeout_after_interface_activation() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("client");
    let source = client_root.join("adaptive-timeout.bin");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    fs::write(&source, b"adaptive timeout payload")?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--no-auth"])
        .arg("--identity-seed")
        .arg("rncp-process-adaptive-timeout-server")
        .args(["--silent", "--timeout", "5"])
        .current_dir(&listener_root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let started = Instant::now();
        let client = Command::new(binary)
            .arg(&source)
            .arg("00000000000000000000000000000000")
            .args([
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--timeout",
                "1",
                "--silent",
                "--identity-seed",
                "rncp-process-adaptive-timeout-client",
            ])
            .current_dir(&client_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let output = client.wait_with_output()?;
        assert!(!output.status.success(), "adaptive-timeout rncp unexpectedly succeeded");
        assert!(
            started.elapsed() >= Duration::from_secs(5),
            "active interface medium timeout was not applied: elapsed {:?}",
            started.elapsed()
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("path discovery timed out"),
            "adaptive-timeout stderr did not preserve path discovery failure: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

#[test]
fn rncp_completes_after_delayed_first_hop_on_a_rate_limited_tcp_path() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let listener_root = temp.path().join("listener");
    let client_root = temp.path().join("client");
    fs::create_dir_all(&listener_root)?;
    fs::create_dir_all(&client_root)?;
    let source = client_root.join("slow-path.bin");
    let payload = (0..8_192)
        .map(|index| (index as u8).wrapping_mul(71).wrapping_add((index >> 3) as u8))
        .collect::<Vec<_>>();
    fs::write(&source, &payload)?;

    let port = free_port()?;
    let binary = env!("CARGO_BIN_EXE_rncp");
    let identity_seed = "rncp-process-slow-path-server";
    let mut listener = Command::new(binary)
        .args(["--listen", &format!("127.0.0.1:{port}"), "--no-auth"])
        .args(["--identity-seed", identity_seed, "--silent", "--timeout", "5"])
        .current_dir(&listener_root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut listener)?;
        let identity_output = Command::new(binary)
            .args(["--print-identity", "--identity-seed", identity_seed])
            .output()?;
        if !identity_output.status.success() {
            return Err(io::Error::other(format!(
                "slow-path listener identity query failed: {}\n{}",
                identity_output.status,
                String::from_utf8_lossy(&identity_output.stderr)
            )));
        }
        let destination = String::from_utf8_lossy(&identity_output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Listening on : "))
            .ok_or_else(|| io::Error::other("slow-path listener identity omitted destination"))?
            .to_owned();
        let upstream =
            format!("127.0.0.1:{port}").parse::<SocketAddr>().map_err(io::Error::other)?;
        let proxy = SlowTcpProxy::start(upstream)?;
        let proxy_address = format!("127.0.0.1:{}", proxy.port);
        let started = Instant::now();
        let output = Command::new(binary)
            .arg(&source)
            .arg(&destination)
            .args([
                "--connect",
                &proxy_address,
                "--no-compress",
                "--silent",
                "--timeout",
                "1",
                "--identity-seed",
                "rncp-process-slow-path-client",
            ])
            .current_dir(&client_root)
            .output()?;
        let elapsed = started.elapsed();
        let (client_to_server_bytes, server_to_client_bytes) = proxy.join()?;

        if !output.status.success() {
            return Err(io::Error::other(format!(
                "rncp failed over the delayed, rate-limited path: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        if elapsed < SLOW_PATH_FIRST_RESPONSE_DELAY {
            return Err(io::Error::other(format!(
                "test path did not apply its first-response delay: {elapsed:?}"
            )));
        }
        assert!(client_to_server_bytes > 0, "slow proxy did not forward client traffic");
        assert!(server_to_client_bytes > 0, "slow proxy did not forward server traffic");
        assert_eq!(fs::read(listener_root.join("slow-path.bin"))?, payload);
        Ok(())
    })();
    let _ = listener.kill();
    let _ = listener.wait();
    result
}

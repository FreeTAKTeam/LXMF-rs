#![cfg(unix)]

use std::io::{self, Read};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
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

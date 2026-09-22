#![cfg(unix)]

use std::io;
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

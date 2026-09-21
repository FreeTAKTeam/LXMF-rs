use std::fs;
use std::io;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
) -> io::Result<std::process::Output> {
    let binary = env!("CARGO_BIN_EXE_rncp");
    let source = source.to_string_lossy();
    let mut command = Command::new(binary);
    command
        .arg(source.as_ref())
        .arg(destination)
        .args(["--connect", &format!("127.0.0.1:{port}"), "--no-compress", "--silent"])
        .arg("--identity-seed")
        .arg(identity_seed)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.output()
}

fn run_client(source: &Path, destination: &str, port: u16, cwd: &Path) -> io::Result<()> {
    let output = run_client_output(source, destination, port, cwd, "rncp-process-client")?;
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

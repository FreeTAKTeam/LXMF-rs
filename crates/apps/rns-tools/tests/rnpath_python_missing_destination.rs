use std::fs;
use std::io;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

static PYTHON_INTEROP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const MISSING_DESTINATION: &str = "00112233445566778899aabbccddeeff";

fn free_port() -> io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn python_repo() -> PathBuf {
    let configured = std::env::var_os("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".tmp/python-refs/Reticulum"));
    if configured.is_absolute() {
        configured
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").join(configured)
    }
}

fn python_bin() -> String {
    std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string())
}

fn wait_for_port(port: u16, child: &mut Child, label: &str) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "{label} exited before opening its port: {status}"
            )));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, format!("{label} did not open port {port}")))
}

fn wait_for_output(mut child: Child, timeout: Duration, label: &str) -> io::Result<Output> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let output = child.wait_with_output()?;
    Err(io::Error::other(format!(
        "{label} timed out\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )))
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn assert_python_reports_missing_path(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "pinned Python rnpath status; stdout={:?}; stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "unexpected Python stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Path not found"),
        "Python rnpath did not report a missing path: {stdout:?}"
    );
    assert!(
        !stdout.contains("Path found"),
        "Python rnpath claimed the missing path was found: {stdout:?}"
    );
}

#[test]
#[ignore = "requires local pinned Python Reticulum checkout and built reticulumd binary"]
fn rnpath_missing_destination_matches_pinned_python_failure_over_live_tcp() -> io::Result<()> {
    let _guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let python_repo = python_repo();
    let python_script = python_repo.join("RNS/Utilities/rnpath.py");
    if !python_script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rnpath script not found: {}", python_script.display()),
        ));
    }

    let network_port = free_port()?;
    let rpc_port = free_port()?;
    let reference_config = temp.path().join("python-network-config");
    fs::create_dir_all(&reference_config)?;
    fs::write(
        reference_config.join("config"),
        format!(
            "[reticulum]\n\
             enable_transport = no\n\
             share_instance = no\n\
             \n\
             [logging]\n\
             loglevel = 0\n\
             \n\
             [interfaces]\n\
             [[TCP Server Interface]]\n\
             type = TCPServerInterface\n\
             enabled = yes\n\
             listen_ip = 127.0.0.1\n\
             listen_port = {network_port}\n"
        ),
    )?;
    let python_client_config = temp.path().join("python-client-config");
    fs::create_dir_all(&python_client_config)?;
    fs::write(
        python_client_config.join("config"),
        format!(
            "[reticulum]\n\
             enable_transport = no\n\
             share_instance = no\n\
             \n\
             [logging]\n\
             loglevel = 0\n\
             \n\
             [interfaces]\n\
             [[TCP Client Interface]]\n\
             type = TCPClientInterface\n\
             enabled = yes\n\
             target_host = 127.0.0.1\n\
             target_port = {network_port}\n"
        ),
    )?;

    let server_script = r#"
import sys
import time
import RNS
RNS.Reticulum(configdir=sys.argv[1], loglevel=0)
while True:
    time.sleep(1)
"#;
    let mut network = Command::new(python_bin())
        .arg("-c")
        .arg(server_script)
        .arg(&reference_config)
        .env("PYTHONPATH", &python_repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(network_port, &mut network, "isolated Python Reticulum TCP network")?;

        let rust_config = temp.path().join("rust-client.toml");
        fs::write(
            &rust_config,
            format!(
                "[reticulum]\n\
                 enable_transport = true\n\
                 share_instance = false\n\
                 respond_to_probes = false\n\
                 \n\
                 [[interfaces]]\n\
                 type = \"tcp_client\"\n\
                 enabled = true\n\
                 name = \"rnpath-negative-peer\"\n\
                 host = \"127.0.0.1\"\n\
                 port = {network_port}\n"
            ),
        )?;
        let rpc = format!("127.0.0.1:{rpc_port}");
        let daemon_log = fs::File::create(temp.path().join("reticulumd.log"))?;
        let daemon_stderr = daemon_log.try_clone()?;
        let mut daemon = Command::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/debug/reticulumd"),
        )
        .args(["--rpc", &rpc, "--db"])
        .arg(temp.path().join("rust-client.db"))
        .args(["--config"])
        .arg(&rust_config)
        .stdout(daemon_log)
        .stderr(daemon_stderr)
        .spawn()?;

        let daemon_result = (|| {
            wait_for_port(rpc_port, &mut daemon, "Rust daemon RPC")?;
            thread::sleep(Duration::from_millis(500));

            let rust_output = wait_for_output(
                Command::new(env!("CARGO_BIN_EXE_rnpath-rs"))
                    .args([MISSING_DESTINATION, "--rpc", &rpc, "--timeout", "2"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()?,
                Duration::from_secs(45),
                "Rust rnpath missing-destination request",
            )?;

            let reference_output = wait_for_output(
                Command::new(python_bin())
                    .arg(&python_script)
                    .args(["--config"])
                    .arg(&python_client_config)
                    .args(["-w", "2", MISSING_DESTINATION])
                    .env("PYTHONPATH", &python_repo)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()?,
                Duration::from_secs(45),
                "pinned Python rnpath missing-destination request",
            )?;

            assert_python_reports_missing_path(&reference_output);
            assert_eq!(
                rust_output.status.code(),
                Some(1),
                "Rust rnpath must fail for an absent path"
            );
            assert!(
                rust_output.stdout.is_empty(),
                "Rust rnpath emitted apparent success: {}",
                String::from_utf8_lossy(&rust_output.stdout)
            );
            let rust_stderr = String::from_utf8_lossy(&rust_output.stderr);
            assert!(
                rust_stderr.contains("path discovery for"),
                "Rust rnpath omitted failure context: {rust_stderr}"
            );
            assert!(
                rust_stderr.contains(MISSING_DESTINATION),
                "Rust rnpath omitted destination hash: {rust_stderr}"
            );
            assert!(
                rust_stderr.contains("timeout"),
                "Rust rnpath did not report timeout/no path: {rust_stderr}"
            );
            assert!(
                !rust_stderr.contains("Path found"),
                "Rust rnpath claimed the missing path was found: {rust_stderr}"
            );
            Ok(())
        })();
        stop(&mut daemon);
        daemon_result
    })();
    stop(&mut network);
    result
}

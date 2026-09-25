use std::fs;
use std::io;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

static PYTHON_INTEROP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

fn wait_for_value(path: &Path, child: &mut Child, label: &str) -> io::Result<String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(value) = fs::read_to_string(path) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value.to_owned());
            }
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "{label} exited before publishing its hash: {status}"
            )));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("{label} did not publish a destination hash"),
    ))
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

#[test]
#[ignore = "requires local Python Reticulum checkout and built reticulumd binary"]
fn rnpath_discovers_late_pinned_python_announce_through_live_daemon_rpc() -> io::Result<()> {
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

    let python_interface_port = free_port()?;
    let rpc_port = free_port()?;
    let python_config = temp.path().join("python-config");
    fs::create_dir_all(&python_config)?;
    fs::write(
        python_config.join("config"),
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
             listen_port = {python_interface_port}\n"
        ),
    )?;

    let destination_path = temp.path().join("python-destination.hash");
    let announce_trigger = temp.path().join("announce-now");
    let responder = r#"
import pathlib
import sys
import time
import RNS

config_dir, hash_path, announce_trigger = sys.argv[1:4]
RNS.Reticulum(configdir=config_dir, loglevel=0)
identity = RNS.Identity()
destination = RNS.Destination(
    identity, RNS.Destination.IN, RNS.Destination.SINGLE, "rnpath", "discovery"
)
pathlib.Path(hash_path).write_text(destination.hash.hex(), encoding="ascii")
while not pathlib.Path(announce_trigger).exists():
    time.sleep(0.02)
destination.announce()
while True:
    time.sleep(1)
"#;
    let mut python = Command::new(python_bin())
        .arg("-c")
        .arg(responder)
        .arg(&python_config)
        .arg(&destination_path)
        .arg(&announce_trigger)
        .env("PYTHONPATH", &python_repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(python_interface_port, &mut python, "pinned Python TCP listener")?;
        let destination_hash =
            wait_for_value(&destination_path, &mut python, "pinned Python responder")?;
        let config = temp.path().join("reticulumd.toml");
        fs::write(
            &config,
            format!(
                "[reticulum]\n\
                 enable_transport = true\n\
                 share_instance = false\n\
                 respond_to_probes = false\n\
                 \n\
                 [[interfaces]]\n\
                 type = \"tcp_client\"\n\
                 enabled = true\n\
                 name = \"python-rnpath-peer\"\n\
                 host = \"127.0.0.1\"\n\
                 port = {python_interface_port}\n"
            ),
        )?;
        let rpc = format!("127.0.0.1:{rpc_port}");
        let rpc_unix = temp.path().join("reticulumd.sock");
        let db = temp.path().join("reticulum.db");
        let log_path = temp.path().join("reticulumd.log");
        let log = fs::File::create(&log_path)?;
        let stderr = log.try_clone()?;
        let mut daemon = Command::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/debug/reticulumd"),
        )
        .args(["--rpc", &rpc, "--rpc-unix"])
        .arg(&rpc_unix)
        .args(["--db"])
        .arg(&db)
        .args(["--config"])
        .arg(&config)
        .stdout(log)
        .stderr(stderr)
        .spawn()?;

        let daemon_result = (|| {
            wait_for_port(rpc_port, &mut daemon, "Rust daemon RPC")?;
            // Allow the daemon's TCP client interface to establish before discovery begins.
            thread::sleep(Duration::from_millis(500));
            let rnpath = PathBuf::from(env!("CARGO_BIN_EXE_rnpath-rs"));
            let client = Command::new(rnpath)
                .args(["--rpc", &rpc, "--timeout", "12", "--json", &destination_hash])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            thread::sleep(Duration::from_millis(300));
            fs::write(&announce_trigger, "announce")?;
            let output = wait_for_output(client, Duration::from_secs(20), "Rust rnpath client")?;
            if !output.status.success() {
                let daemon_log = fs::read_to_string(&log_path).unwrap_or_default();
                return Err(io::Error::other(format!(
                    "Rust rnpath failed: {}\nstdout:\n{}\nstderr:\n{}\ndaemon log:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                    daemon_log
                )));
            }
            let response: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            assert_eq!(response["path_found"], true, "rnpath JSON: {response}");
            assert_eq!(response["destination_hash"], destination_hash);
            assert_eq!(response["hops"], 1, "rnpath JSON: {response}");
            Ok(())
        })();
        stop(&mut daemon);
        daemon_result
    })();
    stop(&mut python);
    result
}

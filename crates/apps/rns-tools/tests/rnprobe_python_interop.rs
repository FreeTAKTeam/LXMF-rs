use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

static PYTHON_INTEROP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn free_port() -> io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
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

fn write_python_config(dir: &Path, port: u16) -> io::Result<()> {
    fs::write(
        dir.join("config"),
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
             target_port = {port}\n"
        ),
    )
}

fn write_python_server_config(dir: &Path, port: u16) -> io::Result<()> {
    fs::write(
        dir.join("config"),
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
             listen_port = {port}\n"
        ),
    )
}

fn wait_for_port(port: u16, child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "daemon exited before opening RPC port: {status}"
            )));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "daemon did not open its RPC port"))
}

fn wait_for_log_marker(path: &Path, child: &mut Child, marker: &str) -> io::Result<String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(path) {
            if let Some(value) =
                contents.split(marker).nth(1).and_then(|rest| rest.split_whitespace().next())
            {
                return Ok(value.to_string());
            }
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "daemon exited before logging probe hash: {status}"
            )));
        }
        thread::sleep(Duration::from_millis(50));
    }
    let log = fs::read_to_string(path).unwrap_or_else(|error| format!("<unreadable: {error}>"));
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("daemon did not log marker {marker:?}; log:\n{log}"),
    ))
}

fn wait_for_file_value(path: &Path, child: &mut Child, label: &str) -> io::Result<String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(value) = fs::read_to_string(path) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value.to_string());
            }
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "{label} exited before publishing its value: {status}"
            )));
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("{label} did not publish a value at {}", path.display()),
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

#[test]
#[ignore = "requires the pinned Python Reticulum checkout"]
fn rnprobe_invalid_probe_count_matches_pinned_python_process_failure() -> io::Result<()> {
    let repo = python_repo();
    let script = repo.join("RNS/Utilities/rnprobe.py");
    if !script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rnprobe script not found: {}", script.display()),
        ));
    }

    let args =
        ["rnstransport.probe", "00112233445566778899aabbccddeeff", "--probes", "not-an-integer"];
    let python =
        Command::new(python_bin()).arg(script).args(args).env("PYTHONPATH", &repo).output()?;
    let rust = Command::new(env!("CARGO_BIN_EXE_rnprobe")).args(args).output()?;

    assert_eq!(
        python.status.code(),
        Some(2),
        "pinned Python stderr: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    assert_eq!(
        rust.status.code(),
        Some(2),
        "Rust stderr: {}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let python_error = String::from_utf8_lossy(&python.stderr);
    let rust_error = String::from_utf8_lossy(&rust.stderr);
    assert!(python_error.contains("argument -n/--probes: invalid int value"), "{python_error}");
    assert!(rust_error.contains("invalid value 'not-an-integer' for '--probes"), "{rust_error}");
    Ok(())
}

fn spawn_rust_daemon(
    config: &Path,
    db: &Path,
    rpc: &str,
    rpc_unix: &Path,
    log: &Path,
) -> io::Result<Child> {
    let stdout = OpenOptions::new().create(true).append(true).open(log)?;
    let stderr = stdout.try_clone()?;
    Command::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/debug/reticulumd"))
        .args(["--rpc", rpc, "--rpc-unix"])
        .arg(rpc_unix)
        .args(["--db"])
        .arg(db)
        .args(["--config"])
        .arg(config)
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
}

fn spawn_python_probe_responder(
    repo: &Path,
    config: &Path,
    identity: &Path,
    hash_path: &Path,
    announce_trigger: &Path,
) -> io::Result<Child> {
    let script = r#"
import pathlib
import sys
import time
import RNS

config_dir, identity_path, hash_path, announce_trigger = sys.argv[1:5]
identity_file = pathlib.Path(identity_path)
identity = RNS.Identity.from_file(identity_path) if identity_file.is_file() else RNS.Identity()
identity.to_file(identity_path)
RNS.Reticulum(configdir=config_dir, loglevel=0)
destination = RNS.Destination(
    identity,
    RNS.Destination.IN,
    RNS.Destination.SINGLE,
    "rnstransport",
    "probe",
)
destination.set_proof_strategy(RNS.Destination.PROVE_ALL)
pathlib.Path(hash_path).write_text(destination.hash.hex(), encoding="ascii")
while not pathlib.Path(announce_trigger).exists():
    time.sleep(0.025)
destination.announce()
while True:
    time.sleep(1)
"#;
    Command::new(python_bin())
        .arg("-c")
        .arg(script)
        .arg(config)
        .arg(identity)
        .arg(hash_path)
        .arg(announce_trigger)
        .env("PYTHONPATH", repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
}

#[test]
#[ignore = "requires local Python Reticulum checkout and a built reticulumd binary"]
fn pinned_python_rnprobe_reaches_rust_daemon_responder() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let rust_interface_port = free_port()?;
    let rpc_port = free_port()?;
    let rust_config = temp.path().join("reticulumd.toml");
    fs::write(
        &rust_config,
        format!(
            "[reticulum]\n\
             enable_transport = true\n\
             share_instance = false\n\
             respond_to_probes = true\n\
             \n\
             [[interfaces]]\n\
             type = \"tcp_server\"\n\
             enabled = true\n\
             name = \"python-rnprobe\"\n\
             host = \"127.0.0.1\"\n\
             port = {rust_interface_port}\n"
        ),
    )?;
    let python_config = temp.path().join("python-config");
    fs::create_dir_all(&python_config)?;
    write_python_config(&python_config, rust_interface_port)?;
    let log = temp.path().join("reticulumd.log");
    let rpc_unix = temp.path().join("rpc.sock");
    let rpc = format!("127.0.0.1:{rpc_port}");
    let mut daemon =
        spawn_rust_daemon(&rust_config, &temp.path().join("reticulum.db"), &rpc, &rpc_unix, &log)?;
    let result = (|| {
        wait_for_port(rpc_port, &mut daemon)?;
        let destination_hash = wait_for_log_marker(&log, &mut daemon, "probe destination hash=")?;
        let repo = python_repo();
        let script = repo.join("RNS/Utilities/rnprobe.py");
        if !script.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("pinned Python rnprobe script not found: {}", script.display()),
            ));
        }
        let child = Command::new(python_bin())
            .arg(script)
            .args([
                "--config",
                python_config.to_string_lossy().as_ref(),
                "--size",
                "32",
                "--probes",
                "2",
                "--timeout",
                "8",
                "--wait",
                "0.1",
                "rnstransport.probe",
                &destination_hash,
            ])
            .env("PYTHONPATH", repo)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let output = wait_for_output(child, Duration::from_secs(30), "Python rnprobe")?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python rnprobe failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Sent 2, received 2"), "Python rnprobe output: {stdout}");
        assert!(stdout.contains("packet loss 0.0%"), "Python rnprobe output: {stdout}");
        Ok(())
    })();
    let _ = daemon.kill();
    let _ = daemon.wait();
    result
}

#[test]
#[ignore = "requires local Python Reticulum checkout and a built reticulumd and rnprobe binary"]
fn native_rnprobe_discovers_late_pinned_python_announce() -> io::Result<()> {
    let _test_guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let python_interface_port = free_port()?;
    let rpc_port = free_port()?;
    let repo = python_repo();
    let python_config = temp.path().join("python-config");
    fs::create_dir_all(&python_config)?;
    write_python_server_config(&python_config, python_interface_port)?;
    let hash_path = temp.path().join("destination.hash");
    let announce_trigger = temp.path().join("announce.trigger");
    let mut python = spawn_python_probe_responder(
        &repo,
        &python_config,
        &python_config.join("identity"),
        &hash_path,
        &announce_trigger,
    )?;
    let result = (|| {
        let pin =
            Command::new("git").args(["-C"]).arg(&repo).args(["rev-parse", "HEAD"]).output()?;
        if !pin.status.success() {
            return Err(io::Error::other(format!(
                "could not identify pinned Python Reticulum checkout: {}",
                String::from_utf8_lossy(&pin.stderr)
            )));
        }
        let actual_pin = String::from_utf8_lossy(&pin.stdout).trim().to_owned();
        if actual_pin != "99de23c040d507e3fefca19e87b182302902725d" {
            return Err(io::Error::other(format!(
                "rnprobe interop requires pinned Reticulum 99de23c040d507e3fefca19e87b182302902725d, found {actual_pin}"
            )));
        }
        wait_for_port(python_interface_port, &mut python)?;
        let destination_hash =
            wait_for_file_value(&hash_path, &mut python, "Python probe responder")?;
        if announce_trigger.exists() {
            return Err(io::Error::other(
                "Python responder announced before the Rust probe started",
            ));
        }
        let rust_config = temp.path().join("reticulumd.toml");
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
                 name = \"python-rnprobe\"\n\
                 host = \"127.0.0.1\"\n\
                 port = {python_interface_port}\n"
            ),
        )?;
        let log = temp.path().join("reticulumd.log");
        let rpc_unix = temp.path().join("rpc.sock");
        let rpc = format!("127.0.0.1:{rpc_port}");
        let mut daemon = spawn_rust_daemon(
            &rust_config,
            &temp.path().join("reticulum.db"),
            &rpc,
            &rpc_unix,
            &log,
        )?;
        let daemon_result = (|| {
            wait_for_port(rpc_port, &mut daemon)?;
            let rnprobe =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/debug/rnprobe");
            let child = Command::new(rnprobe)
                .args([
                    "--rpc",
                    &rpc,
                    "--size",
                    "24",
                    "--probes",
                    "2",
                    "--timeout",
                    "8",
                    "--wait",
                    "0.1",
                    "--json",
                    "rnstransport.probe",
                    &destination_hash,
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            // Start the real CLI with no known announce/path; release the peer only after
            // its daemon-side probe request has begun waiting for discovery.
            thread::sleep(Duration::from_millis(300));
            fs::write(&announce_trigger, "announce")?;
            let output = wait_for_output(child, Duration::from_secs(30), "native rnprobe")?;
            if !output.status.success() {
                return Err(io::Error::other(format!(
                    "native rnprobe failed: {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let response: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            assert_eq!(response["destination"], destination_hash);
            assert_eq!(response["probes"], 2);
            assert_eq!(response["sent"], 2);
            assert_eq!(response["replies"], 2);
            assert_eq!(response["results"][0]["status"], "delivered");
            assert_eq!(response["results"][1]["status"], "delivered");
            assert!(stdout.contains("\"replies\": 2"), "native rnprobe output: {stdout}");
            assert!(
                stdout.contains("\"packet_loss_percent\": 0.0"),
                "native rnprobe output: {stdout}"
            );
            Ok(())
        })();
        let _ = daemon.kill();
        let _ = daemon.wait();
        daemon_result
    })();
    let _ = python.kill();
    let _ = python.wait();
    result
}

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
    Err(io::Error::new(io::ErrorKind::TimedOut, format!("{label} did not publish its hash")))
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

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn take(&mut self) -> io::Result<Child> {
        self.0.take().ok_or_else(|| io::Error::other("child process already consumed"))
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            stop(child);
        }
    }
}

#[test]
#[ignore = "requires local pinned Python Reticulum checkout and built reticulumd binary"]
fn rnpath_table_matches_frozen_python_route_fields_over_live_tcp() -> io::Result<()> {
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
    let python_peer_config = temp.path().join("python-peer-config");
    fs::create_dir_all(&python_peer_config)?;
    fs::write(
        python_peer_config.join("config"),
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

    let destination_path = temp.path().join("python-destination.hash");
    let announce_trigger = temp.path().join("announce-now");
    let responder = r#"
import pathlib, sys, time, RNS
config_dir, hash_path, trigger = sys.argv[1:4]
RNS.Reticulum(configdir=config_dir, loglevel=0)
identity = RNS.Identity()
destination = RNS.Destination(identity, RNS.Destination.IN, RNS.Destination.SINGLE, "rnpath", "table")
pathlib.Path(hash_path).write_text(destination.hash.hex(), encoding="ascii")
while not pathlib.Path(trigger).exists(): time.sleep(0.02)
destination.announce()
while True: time.sleep(1)
"#;
    let mut python_peer = Command::new(python_bin())
        .arg("-c")
        .arg(responder)
        .arg(&python_peer_config)
        .arg(&destination_path)
        .arg(&announce_trigger)
        .env("PYTHONPATH", &python_repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(network_port, &mut python_peer, "pinned Python TCP listener")?;
        let destination_hash =
            wait_for_value(&destination_path, &mut python_peer, "Python responder")?;

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
                 name = \"python-rnpath-table-peer\"\n\
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
        .arg(temp.path().join("reticulum.db"))
        .args(["--config"])
        .arg(&rust_config)
        .stdout(daemon_log)
        .stderr(daemon_stderr)
        .spawn()?;

        let daemon_result = (|| {
            wait_for_port(rpc_port, &mut daemon, "Rust daemon RPC")?;
            thread::sleep(Duration::from_millis(500));

            let observer_config = temp.path().join("python-observer-config");
            fs::create_dir_all(&observer_config)?;
            fs::write(
                observer_config.join("config"),
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
            let observer = r#"
import json, sys, time, RNS
config_dir, destination_hex = sys.argv[1:3]
reticulum = RNS.Reticulum(configdir=config_dir, loglevel=0)
destination = bytes.fromhex(destination_hex)
deadline = time.time() + 15
while not RNS.Transport.has_path(destination) and time.time() < deadline: time.sleep(0.05)
if not RNS.Transport.has_path(destination): raise SystemExit("Python observer did not learn announced path")
table = sorted(reticulum.get_path_table(), key=lambda row: (row["interface"], row["hops"]))
for row in table:
    for key, value in list(row.items()):
        if isinstance(value, bytes): row[key] = RNS.hexrep(value, delimit=False)
print(json.dumps(table))
"#;
            let mut python_observer = ChildGuard::new(
                Command::new(python_bin())
                    .arg("-c")
                    .arg(observer)
                    .arg(&observer_config)
                    .arg(&destination_hash)
                    .env("PYTHONPATH", &python_repo)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()?,
            );

            thread::sleep(Duration::from_millis(500));
            fs::write(&announce_trigger, "announce")?;
            thread::sleep(Duration::from_millis(750));

            let rust_output = wait_for_output(
                Command::new(env!("CARGO_BIN_EXE_rnpath-rs"))
                    .args(["--rpc", &rpc, "--table", "--json"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()?,
                Duration::from_secs(20),
                "Rust rnpath --table",
            )?;
            if !rust_output.status.success() {
                return Err(io::Error::other(format!(
                    "Rust rnpath --table failed: {}\nstdout: {}\nstderr: {}",
                    rust_output.status,
                    String::from_utf8_lossy(&rust_output.stdout),
                    String::from_utf8_lossy(&rust_output.stderr)
                )));
            }

            let python_output = wait_for_output(
                python_observer.take()?,
                Duration::from_secs(20),
                "pinned Python path-table observer",
            )?;
            if !python_output.status.success() {
                return Err(io::Error::other(format!(
                    "pinned Python path-table observer failed: {}\nstdout: {}\nstderr: {}",
                    python_output.status,
                    String::from_utf8_lossy(&python_output.stdout),
                    String::from_utf8_lossy(&python_output.stderr)
                )));
            }

            let rust_rows: serde_json::Value = serde_json::from_slice(&rust_output.stdout)?;
            let python_rows: serde_json::Value = serde_json::from_slice(&python_output.stdout)?;
            let rust_row = rust_rows
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["hash"] == destination_hash))
                .ok_or_else(|| {
                    io::Error::other(format!(
                        "Rust path table omitted announced destination: {rust_rows}"
                    ))
                })?;
            let python_row = python_rows
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["hash"] == destination_hash))
                .ok_or_else(|| {
                    io::Error::other(format!(
                        "Python path table omitted announced destination: {python_rows}"
                    ))
                })?;

            assert_eq!(rust_row["hash"], python_row["hash"]);
            assert_eq!(rust_row["via"], python_row["via"]);
            assert_eq!(rust_row["hops"], python_row["hops"]);
            assert_eq!(rust_row["hops"], 1);
            assert!(rust_row["expires"].as_f64().is_some(), "Rust expiry field: {rust_row}");
            assert!(python_row["expires"].as_f64().is_some(), "Python expiry field: {python_row}");
            assert!(rust_row["interface"].is_string(), "Rust interface field: {rust_row}");
            assert!(python_row["interface"].is_string(), "Python interface field: {python_row}");
            eprintln!(
                "path-table fields matched: hash={}, via={}, hops={}; expires are numeric; interface labels differ by representation (Rust={:?}, Python={:?})",
                rust_row["hash"], rust_row["via"], rust_row["hops"], rust_row["interface"], python_row["interface"]
            );
            Ok(())
        })();
        stop(&mut daemon);
        daemon_result
    })();
    stop(&mut python_peer);
    result
}

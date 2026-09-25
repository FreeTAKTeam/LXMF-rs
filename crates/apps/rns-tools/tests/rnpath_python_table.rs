use std::fs;
use std::io;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

static PYTHON_INTEROP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
const PINNED_RETICULUM_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";

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

fn assert_pinned_python_checkout(repo: &Path) -> io::Result<()> {
    let output = Command::new("git").arg("-C").arg(repo).args(["rev-parse", "HEAD"]).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "could not verify pinned Python Reticulum checkout at {}: {}",
            repo.display(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let revision = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if revision != PINNED_RETICULUM_REVISION {
        return Err(io::Error::other(format!(
            "rnpath parity requires Reticulum {PINNED_RETICULUM_REVISION}, found {revision} at {}",
            repo.display()
        )));
    }
    Ok(())
}

fn wait_for_port(port: u16, child: &mut Child, label: &str) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "{label} exited before opening port {port}: {status}"
            )));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, format!("{label} did not open port {port}")))
}

fn wait_for_file(path: &Path, child: &mut Child, label: &str) -> io::Result<String> {
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
                "{label} exited before publishing evidence: {status}"
            )));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, format!("{label} did not publish evidence")))
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
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            stop(child);
        }
    }
}

fn run_rust_table(rpc: &str, max_hops: Option<u8>) -> io::Result<serde_json::Value> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rnpath-rs"));
    command.args(["--rpc", rpc, "--table", "--json"]);
    if let Some(max_hops) = max_hops {
        command.args(["--max", &max_hops.to_string()]);
    }
    let output = wait_for_output(
        command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?,
        Duration::from_secs(20),
        "Rust rnpath --table",
    )?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Rust rnpath table command failed: {}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(io::Error::other)
}

fn run_python_table(
    repo: &Path,
    config: &Path,
    max_hops: Option<u8>,
) -> io::Result<serde_json::Value> {
    let mut command = Command::new(python_bin());
    command
        .arg(repo.join("RNS/Utilities/rnpath.py"))
        .args(["--config"])
        .arg(config)
        .args(["--table", "--json"]);
    if let Some(max_hops) = max_hops {
        command.args(["--max", &max_hops.to_string()]);
    }
    let output = wait_for_output(
        command.env("PYTHONPATH", repo).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?,
        Duration::from_secs(20),
        "pinned Python rnpath --table",
    )?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "pinned Python rnpath table command failed: {}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(io::Error::other)
}

fn route<'a>(
    table: &'a serde_json::Value,
    destination_hash: &str,
) -> Option<&'a serde_json::Value> {
    table.as_array()?.iter().find(|row| {
        row["hash"].as_str().is_some_and(|hash| hash.eq_ignore_ascii_case(destination_hash))
    })
}

#[test]
#[ignore = "requires the pinned Python Reticulum checkout and built reticulumd binary"]
fn rnpath_max_hops_matches_frozen_python_over_a_live_two_hop_route() -> io::Result<()> {
    let _guard = PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let python_repo = python_repo();
    assert_pinned_python_checkout(&python_repo)?;
    let python_script = python_repo.join("RNS/Utilities/rnpath.py");
    if !python_script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rnpath script not found: {}", python_script.display()),
        ));
    }

    let relay_ingress = free_port()?;
    let relay_egress = free_port()?;
    let rpc_port = free_port()?;
    let relay_config = temp.path().join("python-relay-config");
    let destination_config = temp.path().join("python-destination-config");
    let observer_config = temp.path().join("python-observer-config");
    for directory in [&relay_config, &destination_config, &observer_config] {
        fs::create_dir_all(directory)?;
    }
    fs::write(
        relay_config.join("config"),
        format!(
            "[reticulum]\n\
             enable_transport = yes\n\
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
             listen_port = {relay_ingress}\n\
             \n\
             [[TCP Destination Interface]]\n\
             type = TCPServerInterface\n\
             enabled = yes\n\
             listen_ip = 127.0.0.1\n\
             listen_port = {relay_egress}\n"
        ),
    )?;
    let client_config = |directory: &Path, share_instance: &str, port: u16| -> io::Result<()> {
        fs::write(
            directory.join("config"),
            format!(
                "[reticulum]\n\
                 enable_transport = no\n\
                 share_instance = {share_instance}\n\
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
    };
    client_config(&destination_config, "no", relay_egress)?;
    client_config(&observer_config, "yes", relay_ingress)?;

    let relay_ready = temp.path().join("relay-ready");
    let relay = r#"
import pathlib, sys, time, RNS
config_dir, ready = sys.argv[1:3]
RNS.Reticulum(configdir=config_dir, loglevel=0)
pathlib.Path(ready).write_text("ready", encoding="ascii")
while True: time.sleep(1)
"#;
    let mut python_relay = Command::new(python_bin())
        .arg("-c")
        .arg(relay)
        .arg(&relay_config)
        .arg(&relay_ready)
        .env("PYTHONPATH", &python_repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(relay_ingress, &mut python_relay, "pinned Python transport relay")?;
        wait_for_port(relay_egress, &mut python_relay, "pinned Python destination-side listener")?;
        let _ = wait_for_file(&relay_ready, &mut python_relay, "pinned Python transport relay")?;

        let destination_path = temp.path().join("destination.hash");
        let announce_trigger = temp.path().join("announce-now");
        let destination = r#"
import pathlib, sys, time, RNS
config_dir, hash_path, trigger = sys.argv[1:4]
RNS.Reticulum(configdir=config_dir, loglevel=0)
identity = RNS.Identity()
destination = RNS.Destination(identity, RNS.Destination.IN, RNS.Destination.SINGLE, "rnpath", "two-hop")
pathlib.Path(hash_path).write_text(destination.hash.hex(), encoding="ascii")
while not pathlib.Path(trigger).exists(): time.sleep(0.02)
destination.announce()
while True: time.sleep(1)
"#;
        let mut python_destination = ChildGuard::new(
            Command::new(python_bin())
                .arg("-c")
                .arg(destination)
                .arg(&destination_config)
                .arg(&destination_path)
                .arg(&announce_trigger)
                .env("PYTHONPATH", &python_repo)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?,
        );
        let destination_hash = wait_for_file(
            &destination_path,
            python_destination.0.as_mut().expect("child guard is populated"),
            "pinned Python destination",
        )?;

        let rpc = format!("127.0.0.1:{rpc_port}");
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
                 name = \"pinned-python-two-hop-relay\"\n\
                 host = \"127.0.0.1\"\n\
                 port = {relay_ingress}\n"
            ),
        )?;
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

            let observer_ready = temp.path().join("observer-ready");
            let observer_trigger = temp.path().join("observer-trigger");
            let observer_script = r#"
import pathlib, sys, time, RNS
config_dir, ready, trigger = sys.argv[1:4]
reticulum = RNS.Reticulum(configdir=config_dir, loglevel=0)
pathlib.Path(ready).write_text("ready", encoding="ascii")
while not pathlib.Path(trigger).exists(): time.sleep(0.02)
while True: time.sleep(1)
"#;
            let mut python_observer = ChildGuard::new(
                Command::new(python_bin())
                    .arg("-c")
                    .arg(observer_script)
                    .arg(&observer_config)
                    .arg(&observer_ready)
                    .arg(&observer_trigger)
                    .env("PYTHONPATH", &python_repo)
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()?,
            );
            let _ = wait_for_file(
                &observer_ready,
                python_observer.0.as_mut().expect("child guard is populated"),
                "pinned Python observer",
            )?;

            // Rust and Python observers are connected before the only route is announced.
            fs::write(&announce_trigger, "announce")?;
            fs::write(&observer_trigger, "observe")?;
            thread::sleep(Duration::from_secs(2));

            let rust_unbounded = run_rust_table(&rpc, None)?;
            let python_unbounded = run_python_table(&python_repo, &observer_config, None)?;
            let rust_route = route(&rust_unbounded, &destination_hash);
            let python_route = route(&python_unbounded, &destination_hash);
            if rust_route.is_none() || python_route.is_none() {
                return Err(io::Error::other(format!(
                    "two-hop route absent from unbounded tables: destination={destination_hash}, Rust={rust_unbounded}, Python={python_unbounded}"
                )));
            }
            let rust_route = rust_route.expect("checked above");
            let python_route = python_route.expect("checked above");
            if rust_route["hops"].as_u64() != Some(2) || python_route["hops"].as_u64() != Some(2) {
                return Err(io::Error::other(format!(
                    "single-route topology did not produce the required two-hop route: destination={destination_hash}, Rust route={rust_route}, Python route={python_route}"
                )));
            }
            assert_eq!(
                rust_route["hash"].as_str().map(str::to_ascii_lowercase),
                Some(destination_hash.to_ascii_lowercase())
            );
            assert_eq!(
                python_route["hash"].as_str().map(str::to_ascii_lowercase),
                Some(destination_hash.to_ascii_lowercase())
            );
            eprintln!("unbounded route confirmed: destination={destination_hash}, Rust={rust_route}, Python={python_route}");

            for max_hops in [1, 2] {
                let rust_limited = run_rust_table(&rpc, Some(max_hops))?;
                let python_limited =
                    run_python_table(&python_repo, &observer_config, Some(max_hops))?;
                let rust_includes = route(&rust_limited, &destination_hash).is_some();
                let python_includes = route(&python_limited, &destination_hash).is_some();
                let expected = max_hops == 2;
                if rust_includes != expected || python_includes != expected {
                    return Err(io::Error::other(format!(
                        "--max {max_hops} disagrees with expected route inclusion={expected}: destination={destination_hash}, Rust={rust_limited}, Python={python_limited}"
                    )));
                }
                assert_eq!(
                    rust_includes, python_includes,
                    "Rust and Python --max {max_hops} route inclusion differs"
                );
                eprintln!("--max {max_hops}: destination inclusion matched for Rust and pinned Python ({rust_includes})");
            }
            Ok(())
        })();
        stop(&mut daemon);
        daemon_result
    })();
    stop(&mut python_relay);
    result
}

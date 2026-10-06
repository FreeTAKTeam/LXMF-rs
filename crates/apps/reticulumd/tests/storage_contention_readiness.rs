#![cfg(all(unix, feature = "zmq-pipeline-rpc"))]

use rns_rpc::rpc::{codec, zmq, RpcRequest, RpcResponse};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use zeromq::{Socket, SocketRecv, SocketSend};

#[test]
fn two_worker_daemon_keeps_readiness_and_polling_during_announce_storage_contention() {
    let root = tempfile::tempdir().expect("disposable daemon storage");
    let target = root.path().join("target");
    let peer = root.path().join("peer");
    std::fs::create_dir(&target).expect("create target storage");
    std::fs::create_dir(&peer).expect("create peer storage");
    let ports = reserve_ports(6);
    std::fs::write(target.join("reticulum.toml"), "").expect("target configuration");
    std::fs::write(
        peer.join("reticulum.toml"),
        format!(
            "[[interfaces]]\ntype = \"tcp_client\"\nenabled = true\nhost = \"127.0.0.1\"\nport = {}\n",
            ports[0]
        ),
    )
    .expect("real TCP peer configuration");
    let mut daemon = start_daemon(&target, ports[0], ports[1], Some(ports[2]));
    wait_ready(ports[1], &mut daemon);
    let mut peer_daemon = start_daemon(&peer, ports[3], ports[4], Some(ports[5]));
    wait_ready(ports[4], &mut peer_daemon);
    let database = target.join("reticulum.db");
    wait_until("real peer announce persisted", || announce_count(&database) > 0);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("independent client runtime");
    let (mut command, mut responses, endpoint) = runtime.block_on(async {
        let mut responses = zeromq::PullSocket::new();
        let endpoint = responses.bind("tcp://127.0.0.1:0").await.expect("response listener");
        let mut command = zeromq::PushSocket::new();
        command
            .connect(&format!("tcp://127.0.0.1:{}", ports[2]))
            .await
            .expect("daemon command endpoint");
        let endpoint = endpoint.to_string();
        let response = rpc(
            &mut command,
            &mut responses,
            &endpoint,
            1,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": ["sdk.capability.async_events", "sdk.capability.cursor_replay"],
                "config": {"profile": "desktop-local-runtime"}
            }),
            Duration::from_secs(3),
        )
        .await
        .expect("negotiate before contention");
        assert!(response.error.is_none(), "negotiation failed: {:?}", response.error);
        (command, responses, endpoint)
    });

    let before = announce_count(&database);
    let identity_before = identity_updated_at(&database);
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let lock_database = database.clone();
    let holder = thread::spawn(move || {
        let connection = rusqlite::Connection::open(lock_database).expect("external writer");
        connection.execute_batch("BEGIN IMMEDIATE").expect("hold disposable database write lock");
        locked_tx.send(()).expect("notify lock acquired");
        // A native watchdog releases even if the daemon's two reactor workers stall.
        let _ = release_rx.recv_timeout(Duration::from_secs(4));
        connection.execute_batch("ROLLBACK").expect("release external writer");
    });
    locked_rx.recv_timeout(Duration::from_secs(3)).expect("external lock acquired");
    // Both periodic maintenance and a real peer announce become due under the lock.
    thread::sleep(Duration::from_millis(1400));
    let readiness = ready(ports[1], Duration::from_millis(500));
    let polled = runtime.block_on(rpc(
        &mut command,
        &mut responses,
        &endpoint,
        2,
        "sdk_poll_events_v2",
        json!({"cursor": null, "max": 1}),
        Duration::from_millis(700),
    ));
    release_tx.send(()).expect("release database contention");
    holder.join().expect("external writer joined");
    wait_ready(ports[1], &mut daemon);
    wait_until("announce and identity persistence resume", || {
        announce_count(&database) > before && identity_updated_at(&database) > identity_before
    });
    // Assert after release so failure does not leave owned work blocked.
    assert!(readiness.is_ok(), "readiness stalled during SQLite contention: {readiness:?}");
    let response =
        polled.expect("correlated ZeroMQ poll stays responsive during SQLite contention");
    assert!(response.error.is_none(), "poll failed: {:?}", response.error);
    stop_gracefully(&mut peer_daemon);
    stop_gracefully(&mut daemon);
}

async fn rpc(
    command: &mut zeromq::PushSocket,
    responses: &mut zeromq::PullSocket,
    endpoint: &str,
    id: u64,
    method: &str,
    params: Value,
    deadline: Duration,
) -> Result<RpcResponse, String> {
    tokio::time::timeout(deadline, async {
        let request = RpcRequest { id, method: method.into(), params: Some(params) };
        let envelope = zmq::ZmqRpcEnvelope::request(
            "storage-contention-regression",
            id,
            endpoint,
            codec::encode_frame(&request).map_err(|error| error.to_string())?,
            None,
        );
        command
            .send(zmq::encode_envelope(&envelope).map_err(|error| error.to_string())?.into())
            .await
            .map_err(|error| error.to_string())?;
        let message = responses.recv().await.map_err(|error| error.to_string())?;
        let bytes = Vec::<u8>::try_from(message).map_err(|error| error.to_string())?;
        let envelope = zmq::decode_envelope(&bytes).map_err(|error| error.to_string())?;
        if envelope.request_id != id {
            return Err(format!("unexpected response correlation: {}", envelope.request_id));
        }
        codec::decode_frame(&envelope.payload).map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| format!("{method} deadline elapsed"))?
}

fn ready(port: u16, deadline: Duration) -> std::io::Result<()> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, deadline)?;
    stream.set_read_timeout(Some(deadline))?;
    stream.set_write_timeout(Some(deadline))?;
    stream.write_all(b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut status = String::new();
    BufReader::new(stream).read_line(&mut status)?;
    if !status.starts_with("HTTP/1.1 200") {
        return Err(std::io::Error::other(format!("unexpected readiness status: {status}")));
    }
    Ok(())
}

fn announce_count(database: &Path) -> i64 {
    rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("read disposable announce database")
        .query_row("SELECT COUNT(*) FROM announces", [], |row| row.get(0))
        .expect("count persisted announces")
}

fn identity_updated_at(database: &Path) -> i64 {
    rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("read disposable identity database")
        .query_row("SELECT COALESCE(MAX(updated_at), 0) FROM announce_identities", [], |row| {
            row.get(0)
        })
        .expect("read persisted identity timestamp")
}

fn wait_until(label: &str, mut predicate: impl FnMut() -> bool) {
    let started = Instant::now();
    while !predicate() {
        assert!(started.elapsed() < Duration::from_secs(10), "timed out: {label}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_ready(port: u16, daemon: &mut Daemon) {
    wait_until("daemon readiness", || {
        assert!(daemon.0.try_wait().expect("owned daemon status").is_none(), "daemon exited");
        ready(port, Duration::from_millis(500)).is_ok()
    });
}

fn reserve_ports(count: usize) -> Vec<u16> {
    let listeners: Vec<_> = (0..count)
        .map(|_| TcpListener::bind(("127.0.0.1", 0)).expect("reserve distinct loopback port"))
        .collect();
    listeners
        .iter()
        .map(|listener| listener.local_addr().expect("listener address").port())
        .collect()
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        // Best-effort cleanup on a test assertion failure.
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn start_daemon(root: &Path, transport: u16, http: u16, zmq: Option<u16>) -> Daemon {
    let mut command = Command::new(env!("CARGO_BIN_EXE_reticulumd"));
    command
        .args([
            "--rpc",
            &format!("127.0.0.1:{http}"),
            "--transport",
            &format!("127.0.0.1:{transport}"),
        ])
        .arg("--db")
        .arg(root.join("reticulum.db"))
        .arg("--identity")
        .arg(root.join("identity"))
        .arg("--config")
        .arg(root.join("reticulum.toml"))
        .arg("--rpc-unix")
        .arg(root.join("rpc.sock"))
        .args(["--announce-interval-secs", "1"])
        .env("TOKIO_WORKER_THREADS", "2")
        .env("LXMD_PROPAGATION_NODE", "true")
        .env("LXMD_PROPAGATION_STORAGE_MAINTENANCE_INTERVAL_SECS", "1")
        .env("RUST_LOG", "error")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if let Some(port) = zmq {
        command.args(["--zmq-rpc-command", &format!("tcp://127.0.0.1:{port}")]);
    }
    Daemon(command.spawn().expect("start owned daemon"))
}

fn stop_gracefully(daemon: &mut Daemon) {
    assert!(Command::new("kill")
        .args(["-INT", &daemon.0.id().to_string()])
        .status()
        .expect("signal owned daemon")
        .success());
    wait_until("owned daemon shutdown", || {
        daemon
            .0
            .try_wait()
            .expect("owned daemon status")
            .map(|status| {
                assert!(status.success(), "daemon shutdown failed: {status}");
                true
            })
            .unwrap_or(false)
    });
}

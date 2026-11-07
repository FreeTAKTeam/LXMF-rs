use rns_rpc::RpcDaemon;
use std::ffi::OsStr;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

pub(super) fn spawn_readiness(
    daemon: Arc<RpcDaemon>,
    expected_rpc: usize,
    bound_rpc: watch::Receiver<usize>,
    shutdown: watch::Receiver<bool>,
) -> Option<ReadinessTask> {
    let socket = std::env::var_os("NOTIFY_SOCKET")?;
    Some(spawn_readiness_to(daemon, expected_rpc, bound_rpc, shutdown, socket))
}

pub(super) struct ReadinessTask {
    task: tokio::task::JoinHandle<()>,
    active: Arc<Mutex<bool>>,
}

impl Drop for ReadinessTask {
    fn drop(&mut self) {
        // Serialize with the nonblocking send so owner cancellation finishes
        // before main sends STOPPING, even if the notifier runs on another thread.
        *self.active.lock().expect("notification lifecycle mutex poisoned") = false;
        self.task.abort();
    }
}

fn spawn_readiness_to(
    daemon: Arc<RpcDaemon>,
    expected_rpc: usize,
    bound_rpc: watch::Receiver<usize>,
    shutdown: watch::Receiver<bool>,
    socket: std::ffi::OsString,
) -> ReadinessTask {
    let active = Arc::new(Mutex::new(true));
    let task = tokio::spawn(notify_when_ready(
        daemon,
        expected_rpc,
        bound_rpc,
        shutdown,
        socket,
        active.clone(),
    ));
    ReadinessTask { task, active }
}

pub(super) struct RpcListenerBound(watch::Sender<usize>);

impl RpcListenerBound {
    pub(super) fn new(sender: watch::Sender<usize>) -> Self {
        sender.send_modify(|count| *count += 1);
        Self(sender)
    }
}

impl Drop for RpcListenerBound {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}

async fn notify_when_ready(
    daemon: Arc<RpcDaemon>,
    expected_rpc: usize,
    mut bound_rpc: watch::Receiver<usize>,
    mut shutdown: watch::Receiver<bool>,
    socket: std::ffi::OsString,
    active: Arc<Mutex<bool>>,
) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        if *bound_rpc.borrow() >= expected_rpc && daemon.is_ready() {
            let active = active.lock().expect("notification lifecycle mutex poisoned");
            if !*active {
                return;
            }
            match send(&socket, "READY=1") {
                Ok(()) => return,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    log::error!("[daemon] systemd readiness notification failed: {error}");
                    return;
                }
            }
        }
        tokio::select! {
            biased;
            result = shutdown.changed() => if result.is_err() { return; },
            result = bound_rpc.changed() => if result.is_err() { return; },
            _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {},
        }
    }
}

pub(super) fn stopping() {
    if let Some(socket) = std::env::var_os("NOTIFY_SOCKET") {
        if let Err(error) = send(&socket, "STOPPING=1") {
            log::warn!("[daemon] systemd shutdown notification failed: {error}");
        }
    }
}

#[cfg(unix)]
fn send(socket: &OsStr, state: &str) -> std::io::Result<()> {
    use std::os::unix::net::UnixDatagram;
    let sender = UnixDatagram::unbound()?;
    sender.set_nonblocking(true)?;
    #[cfg(target_os = "linux")]
    if let Some(name) = {
        use std::os::unix::ffi::OsStrExt;
        socket.as_bytes().strip_prefix(b"@")
    } {
        use std::os::linux::net::SocketAddrExt;
        let address = std::os::unix::net::SocketAddr::from_abstract_name(name)?;
        sender.send_to_addr(state.as_bytes(), &address)?;
        return Ok(());
    }
    sender.send_to(state.as_bytes(), socket)?;
    Ok(())
}

#[cfg(not(unix))]
fn send(_socket: &OsStr, _state: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "systemd notification requires Unix sockets",
    ))
}

pub(super) async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn readiness_waits_for_rpc_and_zmq_and_shutdown_cancels_it() {
        let root = tempfile::tempdir().expect("notify directory");
        let path = root.path().join("notify.sock");
        let receiver = tokio::net::UnixDatagram::bind(&path).expect("notification receiver");
        let daemon = Arc::new(RpcDaemon::test_instance());
        daemon.configure_zmq_readiness(1);
        let (bound_tx, bound_rx) = watch::channel(0);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(notify_when_ready(
            daemon.clone(),
            1,
            bound_rx.clone(),
            shutdown_rx.clone(),
            path.clone().into_os_string(),
            Arc::new(Mutex::new(true)),
        ));
        let mut bytes = [0; 64];
        assert!(timeout(Duration::from_millis(50), receiver.recv(&mut bytes)).await.is_err());
        bound_tx.send_replace(1);
        assert!(timeout(Duration::from_millis(50), receiver.recv(&mut bytes)).await.is_err());
        let _listener = daemon.zmq_listener_bound();
        let length = timeout(Duration::from_secs(1), receiver.recv(&mut bytes))
            .await
            .expect("ready deadline")
            .expect("ready datagram");
        assert_eq!(&bytes[..length], b"READY=1");
        task.await.expect("ready task");
        send(path.as_os_str(), "STOPPING=1").expect("stopping notification");
        let length = receiver.recv(&mut bytes).await.expect("stop datagram");
        assert_eq!(&bytes[..length], b"STOPPING=1");
        shutdown_tx.send_replace(true);
        notify_when_ready(
            daemon.clone(),
            1,
            bound_rx,
            shutdown_rx,
            path.into_os_string(),
            Arc::new(Mutex::new(true)),
        )
        .await;
        assert!(timeout(Duration::from_millis(50), receiver.recv(&mut bytes)).await.is_err());
    }

    #[tokio::test]
    async fn dropped_owner_cancels_readiness_and_dead_rpc_listener_is_not_ready() {
        let root = tempfile::tempdir().expect("notify directory");
        let path = root.path().join("notify.sock");
        let receiver = tokio::net::UnixDatagram::bind(&path).expect("receiver");
        let daemon = Arc::new(RpcDaemon::test_instance());
        daemon.configure_zmq_readiness(1);
        let (bound_tx, bound_rx) = watch::channel(0);
        let listener = RpcListenerBound::new(bound_tx);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let owner = spawn_readiness_to(
            daemon.clone(),
            1,
            bound_rx.clone(),
            shutdown_rx,
            path.into_os_string(),
        );
        drop(listener);
        let _zmq = daemon.zmq_listener_bound();
        let mut bytes = [0; 64];
        assert!(timeout(Duration::from_millis(50), receiver.recv(&mut bytes)).await.is_err());
        drop(owner);
        let (bound_tx, bound_rx) = watch::channel(0);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let cancelled = spawn_readiness_to(
            daemon.clone(),
            1,
            bound_rx,
            shutdown_rx,
            receiver
                .local_addr()
                .expect("receiver address")
                .as_pathname()
                .expect("path")
                .as_os_str()
                .to_owned(),
        );
        drop(cancelled);
        bound_tx.send_replace(1);
        assert!(timeout(Duration::from_millis(50), receiver.recv(&mut bytes)).await.is_err());
    }

    #[tokio::test]
    async fn full_notification_queue_retries_until_delivery() {
        let root = tempfile::tempdir().expect("notify directory");
        let path = root.path().join("notify.sock");
        let receiver = tokio::net::UnixDatagram::bind(&path).expect("receiver");
        let sender = std::os::unix::net::UnixDatagram::unbound().expect("sender");
        sender.set_nonblocking(true).expect("nonblocking sender");
        let mut queued = 0;
        loop {
            match sender.send_to(b"filler", &path) {
                Ok(_) => queued += 1,
                Err(error) => {
                    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
                    break;
                }
            }
        }
        let daemon = Arc::new(RpcDaemon::test_instance());
        let (_bound_tx, bound_rx) = watch::channel(1);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let _owner = spawn_readiness_to(daemon, 1, bound_rx, shutdown_rx, path.into_os_string());
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut bytes = [0; 64];
        for _ in 0..queued {
            receiver.recv(&mut bytes).await.expect("drain filler");
        }
        let length = timeout(Duration::from_secs(1), receiver.recv(&mut bytes))
            .await
            .expect("retry deadline")
            .expect("ready");
        assert_eq!(&bytes[..length], b"READY=1");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn abstract_notify_socket_receives_datagram() {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::{SocketAddr, UnixDatagram};
        let name = format!("lxmf-notify-test-{}", std::process::id());
        let address = SocketAddr::from_abstract_name(name.as_bytes()).expect("abstract address");
        let receiver = UnixDatagram::bind_addr(&address).expect("abstract receiver");
        receiver.set_read_timeout(Some(std::time::Duration::from_secs(1))).expect("read timeout");
        send(OsStr::new(&format!("@{name}")), "READY=1").expect("notify abstract socket");
        let mut bytes = [0; 64];
        let length = receiver.recv(&mut bytes).expect("abstract datagram");
        assert_eq!(&bytes[..length], b"READY=1");
    }
}

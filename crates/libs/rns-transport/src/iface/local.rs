use alloc::string::String;
#[cfg(any(target_os = "linux", target_os = "android"))]
use std::ffi::OsString;
#[cfg(any(target_os = "linux", target_os = "android"))]
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{UnixListener, UnixStream};

use super::tcp_client::{
    run_hdlc_stream, run_hdlc_stream_with_runtime, HdlcStreamRuntime, TcpClient,
};
use super::{Interface, InterfaceContext, InterfaceManager};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalUnixEndpoint {
    Filesystem(PathBuf),
    Abstract(String),
}

impl LocalUnixEndpoint {
    pub fn filesystem<P: Into<PathBuf>>(path: P) -> Self {
        Self::Filesystem(path.into())
    }

    pub fn abstract_name<T: Into<String>>(name: T) -> Self {
        let mut name = name.into();
        if let Some(stripped) = name.strip_prefix('@') {
            name = stripped.to_string();
        }
        if let Some(stripped) = name.strip_prefix('\0') {
            name = stripped.to_string();
        }
        Self::Abstract(name)
    }

    #[must_use]
    pub fn from_config_value(value: &str) -> Self {
        if let Some(stripped) = value.strip_prefix('@') {
            Self::abstract_name(stripped)
        } else if let Some(stripped) = value.strip_prefix('\0') {
            Self::abstract_name(stripped)
        } else {
            Self::filesystem(value)
        }
    }

    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Filesystem(path) => path.display().to_string(),
            Self::Abstract(name) => format!("@{name}"),
        }
    }

    #[must_use]
    pub fn is_abstract(&self) -> bool {
        matches!(self, Self::Abstract(_))
    }

    fn filesystem_path(&self) -> Option<&Path> {
        match self {
            Self::Filesystem(path) => Some(path.as_path()),
            Self::Abstract(_) => None,
        }
    }

    fn tokio_path(&self) -> PathBuf {
        match self {
            Self::Filesystem(path) => path.clone(),
            Self::Abstract(name) => abstract_socket_path(name),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn abstract_socket_path(name: &str) -> PathBuf {
    let mut bytes = Vec::with_capacity(name.len() + 1);
    bytes.push(0);
    bytes.extend_from_slice(name.as_bytes());
    PathBuf::from(OsString::from_vec(bytes))
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn abstract_socket_path(name: &str) -> PathBuf {
    PathBuf::from(format!("@{name}"))
}

pub struct LocalUnixServer {
    endpoint: LocalUnixEndpoint,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    client_mtu: usize,
    client_forced_bitrate_bps: Option<u64>,
}

impl LocalUnixServer {
    pub const DEFAULT_CLIENT_MTU: usize = TcpClient::DEFAULT_MTU;

    pub fn new<P: Into<PathBuf>>(
        path: P,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        Self {
            endpoint: LocalUnixEndpoint::filesystem(path),
            iface_manager,
            client_mtu: Self::DEFAULT_CLIENT_MTU,
            client_forced_bitrate_bps: None,
        }
    }

    pub fn new_endpoint(
        endpoint: LocalUnixEndpoint,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        Self {
            endpoint,
            iface_manager,
            client_mtu: Self::DEFAULT_CLIENT_MTU,
            client_forced_bitrate_bps: None,
        }
    }

    #[must_use]
    pub fn with_client_mtu(mut self, client_mtu: usize) -> Self {
        self.client_mtu = client_mtu.max(256);
        self
    }

    #[must_use]
    pub fn with_client_forced_bitrate(mut self, bitrate_bps: u64) -> Self {
        self.client_forced_bitrate_bps = (bitrate_bps > 0).then_some(bitrate_bps);
        self
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let parent_iface = context.channel.address;
        let (endpoint, client_mtu, client_forced_bitrate_bps, iface_manager) = {
            let guard = context.inner.lock().unwrap();
            (
                guard.endpoint.clone(),
                guard.client_mtu,
                guard.client_forced_bitrate_bps,
                guard.iface_manager.clone(),
            )
        };

        let (_, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));
        let endpoint_label = endpoint.label();

        let tx_task = {
            let cancel = context.cancel.clone();
            let iface_stop = iface_stop.clone();
            let tx_channel = tx_channel.clone();

            tokio::spawn(async move {
                loop {
                    if cancel.is_cancelled() || iface_stop.is_cancelled() {
                        break;
                    }

                    let mut tx_channel = tx_channel.lock().await;

                    tokio::select! {
                        _ = cancel.cancelled() => {
                            break;
                        }
                        _ = iface_stop.cancelled() => {
                            break;
                        }
                        // Listener interfaces do not transmit packets directly.
                        message = tx_channel.recv() => {
                            if message.is_none() {
                                break;
                            }
                        }
                    }
                }
            })
        };

        loop {
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            if let Some(path) = endpoint.filesystem_path() {
                if let Err(err) = prepare_socket_path(path) {
                    log::warn!("couldn't prepare local unix socket <{}>: {}", endpoint_label, err);
                    tokio::select! {
                        _ = context.cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                    }
                    continue;
                }
            }

            let listener = match UnixListener::bind(endpoint.tokio_path()) {
                Ok(listener) => listener,
                Err(err) => {
                    log::warn!("couldn't bind local unix socket <{}>: {}", endpoint_label, err);
                    tokio::select! {
                        _ = context.cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                    }
                    continue;
                }
            };

            log::info!("listen on local unix socket <{}>", endpoint_label);
            let mut counter = 0usize;

            loop {
                if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = context.cancel.cancelled() => {
                        break;
                    }
                    _ = iface_stop.cancelled() => {
                        break;
                    }
                    client = listener.accept() => {
                        match client {
                            Ok((stream, _addr)) => {
                                counter = counter.saturating_add(1);
                                let peer_label = format!("unix:{}#{}", endpoint_label, counter);
                                log::info!("new local unix client <{}> connected", peer_label);

                                let mut iface_manager = iface_manager.lock().await;
                                let mut client =
                                    LocalUnixClient::new_from_stream(peer_label, stream)
                                        .with_mtu(client_mtu);
                                if let Some(bitrate_bps) = client_forced_bitrate_bps {
                                    client = client.with_forced_bitrate(bitrate_bps);
                                }
                                let child_iface = iface_manager.spawn(
                                    client,
                                    LocalUnixClient::spawn,
                                );
                                iface_manager.inherit_runtime_config(parent_iface, child_iface);
                            }
                            Err(err) => {
                                log::warn!(
                                    "local unix socket accept error <{}>: {}",
                                    endpoint_label,
                                    err
                                );
                                break;
                            }
                        }
                    }
                }
            }

            if let Some(path) = endpoint.filesystem_path() {
                let _ = remove_socket_file(path);
            }
        }

        if let Some(path) = endpoint.filesystem_path() {
            let _ = remove_socket_file(path);
        }
        iface_stop.cancel();
        let _ = tokio::join!(tx_task);
    }

    pub async fn preflight_bind_available(endpoint: &LocalUnixEndpoint) -> std::io::Result<()> {
        if let Some(path) = endpoint.filesystem_path() {
            prepare_socket_path(path)?;
        }
        let listener = UnixListener::bind(endpoint.tokio_path())?;
        drop(listener);
        if let Some(path) = endpoint.filesystem_path() {
            remove_socket_file(path)?;
        }
        Ok(())
    }
}

impl Interface for LocalUnixServer {
    fn mtu() -> usize {
        2048
    }
}

pub struct LocalUnixClient {
    addr: String,
    stream: Option<UnixStream>,
    connect_endpoint: Option<LocalUnixEndpoint>,
    mtu: usize,
    forced_bitrate_bps: Option<u64>,
    reconnect_wait: Duration,
    reconnect_events: Option<tokio::sync::mpsc::Sender<crate::hash::AddressHash>>,
}

impl LocalUnixClient {
    pub const DEFAULT_RECONNECT_WAIT: Duration = Duration::from_secs(5);

    pub fn new_from_stream<T: Into<String>>(addr: T, stream: UnixStream) -> Self {
        Self {
            addr: addr.into(),
            stream: Some(stream),
            connect_endpoint: None,
            mtu: TcpClient::DEFAULT_MTU,
            forced_bitrate_bps: None,
            reconnect_wait: Self::DEFAULT_RECONNECT_WAIT,
            reconnect_events: None,
        }
    }

    pub fn new_connect(endpoint: LocalUnixEndpoint) -> Self {
        let addr = endpoint.label();
        Self {
            addr,
            stream: None,
            connect_endpoint: Some(endpoint),
            mtu: TcpClient::DEFAULT_MTU,
            forced_bitrate_bps: None,
            reconnect_wait: Self::DEFAULT_RECONNECT_WAIT,
            reconnect_events: None,
        }
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(256);
        self
    }

    #[must_use]
    pub fn with_forced_bitrate(mut self, bitrate_bps: u64) -> Self {
        self.forced_bitrate_bps = (bitrate_bps > 0).then_some(bitrate_bps);
        self
    }

    #[must_use]
    pub fn with_reconnect_wait(mut self, reconnect_wait: Duration) -> Self {
        self.reconnect_wait = reconnect_wait;
        self
    }

    #[must_use]
    pub fn with_reconnect_events(
        mut self,
        events: tokio::sync::mpsc::Sender<crate::hash::AddressHash>,
    ) -> Self {
        self.reconnect_events = Some(events);
        self
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (
            addr,
            mtu,
            forced_bitrate_bps,
            mut stream,
            connect_endpoint,
            reconnect_wait,
            reconnect_events,
        ) = {
            let mut guard = context.inner.lock().unwrap();
            (
                guard.addr.clone(),
                guard.mtu,
                guard.forced_bitrate_bps,
                guard.stream.take(),
                guard.connect_endpoint.clone(),
                guard.reconnect_wait,
                guard.reconnect_events.clone(),
            )
        };

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        let mut running = true;
        let mut has_connected = false;
        while running && !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
            let stream = match stream.take() {
                Some(stream) => {
                    running = false;
                    Ok(stream)
                }
                None => {
                    let Some(endpoint) = connect_endpoint.as_ref() else {
                        break;
                    };
                    UnixStream::connect(endpoint.tokio_path()).await
                }
            };

            let stream = match stream {
                Ok(stream) => stream,
                Err(err) => {
                    log::warn!("couldn't connect local unix client <{}>: {}", addr, err);
                    tokio::select! {
                        _ = context.cancel.cancelled() => break,
                        _ = iface_stop.cancelled() => break,
                        _ = tokio::time::sleep(reconnect_wait) => {}
                    }
                    continue;
                }
            };

            let (read_stream, write_stream) = stream.into_split();

            log::info!("connected to local unix client <{}>", addr);
            if has_connected {
                if let Some(events) = reconnect_events.as_ref() {
                    if let Err(err) = events.try_send(iface_address) {
                        log::debug!(
                            "dropped local unix reconnect event iface={} endpoint={} err={}",
                            iface_address,
                            addr,
                            err
                        );
                    }
                }
            } else {
                has_connected = true;
            }

            if let Some(bitrate_bps) = forced_bitrate_bps {
                run_hdlc_stream_with_runtime(
                    "local_unix".to_string(),
                    iface_address,
                    mtu,
                    context.cancel.clone(),
                    iface_stop.clone(),
                    rx_channel.clone(),
                    tx_channel.clone(),
                    read_stream,
                    write_stream,
                    HdlcStreamRuntime::new().with_forced_bitrate(bitrate_bps),
                )
                .await;
            } else {
                run_hdlc_stream(
                    "local_unix".to_string(),
                    iface_address,
                    mtu,
                    context.cancel.clone(),
                    iface_stop.clone(),
                    rx_channel.clone(),
                    tx_channel.clone(),
                    read_stream,
                    write_stream,
                )
                .await;
            }

            log::info!("disconnected from local unix client <{}>", addr);
        }

        iface_stop.cancel();
    }
}

pub async fn preflight_unix_connect(endpoint: &LocalUnixEndpoint) -> Result<(), String> {
    UnixStream::connect(endpoint.tokio_path()).await.map(|_| ()).map_err(|err| {
        format!("local unix preflight connect failed endpoint={} err={}", endpoint.label(), err)
    })
}

impl Interface for LocalUnixClient {
    fn mtu() -> usize {
        TcpClient::DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

fn prepare_socket_path(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    remove_socket_file(path)
}

fn remove_socket_file(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_socket() {
                std::fs::remove_file(path)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "path exists and is not a unix socket",
                ))
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_abstract_config_labels() {
        let endpoint = LocalUnixEndpoint::from_config_value("@rns/default");
        assert_eq!(endpoint, LocalUnixEndpoint::Abstract("rns/default".to_string()));
        assert_eq!(endpoint.label(), "@rns/default");
        assert!(endpoint.is_abstract());
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[tokio::test]
    async fn abstract_unix_endpoint_accepts_stream_without_filesystem_socket() {
        let name = format!(
            "rns/test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        );
        let endpoint = LocalUnixEndpoint::Abstract(name.clone());
        let listener = UnixListener::bind(endpoint.tokio_path()).expect("bind abstract socket");

        let connect_path = endpoint.tokio_path();
        let accept_task = tokio::spawn(async move { listener.accept().await });
        let _client = UnixStream::connect(connect_path).await.expect("connect abstract socket");
        let (_server, _addr) =
            accept_task.await.expect("accept task").expect("accept abstract socket");

        assert!(!Path::new(format!("@{name}").as_str()).exists());
    }

    #[tokio::test]
    async fn local_unix_server_stops_on_interface_stop_and_releases_socket() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("reticulum-local-server.sock");
        let endpoint = LocalUnixEndpoint::filesystem(path.clone());
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let context = {
            let mut manager_guard = manager.lock().await;
            manager_guard.new_context(LocalUnixServer::new(path.clone(), manager.clone()))
        };
        let iface_address = context.channel.address;
        let task = tokio::spawn(LocalUnixServer::spawn(context));

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if path.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("local unix server socket was not created");

        assert!(manager.lock().await.stop_interface(iface_address));
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("local unix server task timed out")
            .expect("local unix server task");

        assert!(!path.exists());
        LocalUnixServer::preflight_bind_available(&endpoint)
            .await
            .expect("socket path can be rebound after stop");
    }

    #[tokio::test]
    async fn local_unix_connect_client_retries_initial_connect_and_reconnects_after_close() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("reticulum-local.sock");
        let endpoint = LocalUnixEndpoint::filesystem(path.clone());
        let mut manager = InterfaceManager::new(8);
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::channel(32);
        let context = manager.new_context(
            LocalUnixClient::new_connect(endpoint.clone())
                .with_reconnect_wait(Duration::from_millis(20))
                .with_reconnect_events(reconnect_tx),
        );
        let iface_address = context.channel.address;
        let cancel = context.cancel.clone();
        let iface_stop = context.channel.stop.clone();
        let task = tokio::spawn(LocalUnixClient::spawn(context));

        let first_listener = UnixListener::bind(endpoint.tokio_path()).expect("bind first socket");
        let (first_stream, _) =
            tokio::time::timeout(Duration::from_secs(2), first_listener.accept())
                .await
                .expect("first reconnect timed out")
                .expect("first accept");
        drop(first_stream);
        drop(first_listener);
        remove_socket_file(&path).expect("remove first socket");

        let second_listener =
            UnixListener::bind(endpoint.tokio_path()).expect("bind second socket");
        let (second_stream, _) =
            tokio::time::timeout(Duration::from_secs(2), second_listener.accept())
                .await
                .expect("second reconnect timed out")
                .expect("second accept");
        let reconnected_iface = tokio::time::timeout(Duration::from_secs(2), reconnect_rx.recv())
            .await
            .expect("reconnect event timed out")
            .expect("reconnect event");

        cancel.cancel();
        drop(second_stream);
        drop(second_listener);
        let _ = remove_socket_file(&path);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("client task timed out")
            .expect("client task");

        assert!(iface_stop.is_cancelled());
        assert_eq!(reconnected_iface, iface_address);
    }
}

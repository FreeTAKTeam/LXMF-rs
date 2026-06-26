use alloc::string::String;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::{lookup_host, TcpListener};

use crate::error::RnsError;

use super::tcp_client::{
    backbone_hdlc_watchdog, prefer_ipv6_socket_addrs, HdlcStreamWatchdog, TcpClient,
    TcpSocketTuning,
};
use super::{Interface, InterfaceContext, InterfaceManager};

pub struct TcpServer {
    addr: String,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    client_mtu: usize,
    client_socket_tuning: TcpSocketTuning,
    client_hdlc_watchdog: Option<HdlcStreamWatchdog>,
    client_forced_bitrate_bps: Option<u64>,
    prefer_ipv6: bool,
}

impl TcpServer {
    pub const DEFAULT_CLIENT_MTU: usize = TcpClient::DEFAULT_MTU;

    pub fn new<T: Into<String>>(
        addr: T,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        Self {
            addr: addr.into(),
            iface_manager,
            client_mtu: Self::DEFAULT_CLIENT_MTU,
            client_socket_tuning: TcpSocketTuning::default(),
            client_hdlc_watchdog: None,
            client_forced_bitrate_bps: None,
            prefer_ipv6: false,
        }
    }

    #[must_use]
    pub fn with_client_mtu(mut self, client_mtu: usize) -> Self {
        self.client_mtu = client_mtu.max(256);
        self
    }

    #[must_use]
    pub fn with_client_socket_tuning(mut self, client_socket_tuning: TcpSocketTuning) -> Self {
        self.client_socket_tuning = client_socket_tuning;
        self
    }

    #[must_use]
    pub fn with_backbone_client_liveness(mut self) -> Self {
        self.client_hdlc_watchdog = Some(backbone_hdlc_watchdog());
        self
    }

    #[must_use]
    pub fn with_client_forced_bitrate(mut self, bitrate_bps: u64) -> Self {
        self.client_forced_bitrate_bps = (bitrate_bps > 0).then_some(bitrate_bps);
        self
    }

    #[must_use]
    pub fn with_prefer_ipv6(mut self, prefer_ipv6: bool) -> Self {
        self.prefer_ipv6 = prefer_ipv6;
        self
    }

    #[must_use]
    pub fn client_socket_tuning(&self) -> TcpSocketTuning {
        self.client_socket_tuning
    }

    #[must_use]
    pub fn client_hdlc_liveness_enabled(&self) -> bool {
        self.client_hdlc_watchdog.is_some()
    }

    #[must_use]
    pub fn client_forced_bitrate_bps(&self) -> Option<u64> {
        self.client_forced_bitrate_bps
    }

    #[must_use]
    pub fn prefer_ipv6(&self) -> bool {
        self.prefer_ipv6
    }

    fn accepted_client(
        addr: String,
        stream: tokio::net::TcpStream,
        client_mtu: usize,
        client_socket_tuning: TcpSocketTuning,
        client_hdlc_watchdog: Option<HdlcStreamWatchdog>,
        client_forced_bitrate_bps: Option<u64>,
    ) -> TcpClient {
        let mut client = TcpClient::new_from_stream(addr, stream)
            .with_mtu(client_mtu)
            .with_socket_tuning(client_socket_tuning);
        if let Some(bitrate_bps) = client_forced_bitrate_bps {
            client = client.with_forced_bitrate(bitrate_bps);
        }
        if let Some(watchdog) = client_hdlc_watchdog {
            client.with_hdlc_watchdog(watchdog)
        } else {
            client
        }
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let parent_iface = context.channel.address;
        let (
            addr,
            client_mtu,
            client_socket_tuning,
            client_hdlc_watchdog,
            client_forced_bitrate_bps,
            prefer_ipv6,
        ) = {
            let guard = context.inner.lock().unwrap();
            (
                guard.addr.clone(),
                guard.client_mtu,
                guard.client_socket_tuning,
                guard.client_hdlc_watchdog.clone(),
                guard.client_forced_bitrate_bps,
                guard.prefer_ipv6,
            )
        };

        let iface_manager = { context.inner.lock().unwrap().iface_manager.clone() };

        let (_, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            let listener = bind_tcp_listener(addr.clone(), prefer_ipv6)
                .await
                .map_err(|_| RnsError::ConnectionError);

            if listener.is_err() {
                log::warn!("couldn't bind to <{}>", addr);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            log::info!("listen on <{}>", addr);

            let listener = listener.unwrap();

            let tx_task = {
                let cancel = context.cancel.clone();
                let tx_channel = tx_channel.clone();

                tokio::spawn(async move {
                    loop {
                        if cancel.is_cancelled() {
                            break;
                        }

                        let mut tx_channel = tx_channel.lock().await;

                        tokio::select! {
                            _ = cancel.cancelled() => {
                                break;
                            }
                            // Skip all tx messages
                            _ = tx_channel.recv() => {}
                        }
                    }
                })
            };

            let cancel = context.cancel.clone();

            loop {
                if cancel.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    }

                    client = listener.accept() => {
                        if let Ok(client) = client {
                            log::info!(
                                "new client <{}> connected to <{}>",
                                client.1,
                                addr
                            );

                            let mut iface_manager = iface_manager.lock().await;

                            let accepted_client = TcpServer::accepted_client(
                                client.1.to_string(),
                                client.0,
                                client_mtu,
                                client_socket_tuning,
                                client_hdlc_watchdog.clone(),
                                client_forced_bitrate_bps,
                            );
                            let child_iface =
                                iface_manager.spawn(accepted_client, TcpClient::spawn);
                            iface_manager.inherit_runtime_config(parent_iface, child_iface);
                        }
                    }
                }
            }

            let _ = tokio::join!(tx_task);
        }
    }
}

impl Interface for TcpServer {
    fn mtu() -> usize {
        2048
    }
}

async fn bind_tcp_listener(addr: String, prefer_ipv6: bool) -> io::Result<TcpListener> {
    let addrs = prefer_ipv6_socket_addrs(lookup_host(addr.as_str()).await?, prefer_ipv6);
    let mut last_err = None;
    for addr in addrs {
        match bind_tcp_socket(addr) {
            Ok(listener) => return Ok(listener),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "TCP listener resolved to no addresses")
    }))
}

fn bind_tcp_socket(addr: SocketAddr) -> io::Result<TcpListener> {
    let domain = if addr.is_ipv6() { Domain::IPV6 } else { Domain::IPV4 };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    let std_listener: std::net::TcpListener = socket.into();
    TcpListener::from_std(std_listener)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{backbone_hdlc_watchdog, bind_tcp_listener, TcpClient, TcpServer, TcpSocketTuning};
    use crate::iface::InterfaceManager;
    use tokio::net::{TcpListener, TcpStream};

    #[test]
    fn tcp_server_exposes_client_socket_tuning() {
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let server = TcpServer::new("127.0.0.1:0", manager.clone());
        assert!(server.client_socket_tuning().is_empty());
        assert!(!server.client_hdlc_liveness_enabled());
        assert!(!server.prefer_ipv6());

        let tuned = TcpServer::new("127.0.0.1:0", manager)
            .with_client_socket_tuning(TcpSocketTuning::backbone())
            .with_prefer_ipv6(true);
        assert_eq!(tuned.client_socket_tuning().nodelay, Some(true));
        assert_eq!(tuned.client_socket_tuning().keepalive, Some(true));
        assert_eq!(tuned.client_socket_tuning().tcp_keepalive_idle, Some(Duration::from_secs(5)));
        assert_eq!(
            tuned.client_socket_tuning().tcp_keepalive_interval,
            Some(Duration::from_secs(2))
        );
        assert_eq!(tuned.client_socket_tuning().tcp_keepalive_retries, Some(12));
        assert_eq!(tuned.client_socket_tuning().tcp_user_timeout, Some(Duration::from_secs(24)));
        assert!(!tuned.client_hdlc_liveness_enabled());
        assert!(tuned.prefer_ipv6());
    }

    #[test]
    fn tcp_server_backbone_client_liveness_is_exposed() {
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let server = TcpServer::new("127.0.0.1:0", manager).with_backbone_client_liveness();

        assert!(server.client_hdlc_liveness_enabled());
    }

    #[tokio::test]
    async fn tcp_server_forwards_configured_liveness_to_accepted_clients() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let _peer = TcpStream::connect(addr).await.expect("connect peer");
        let (stream, peer_addr) = listener.accept().await.expect("accept stream");

        let ordinary = TcpServer::accepted_client(
            peer_addr.to_string(),
            stream,
            TcpClient::DEFAULT_MTU,
            TcpSocketTuning::default(),
            None,
            None,
        );
        assert!(!ordinary.hdlc_liveness_enabled());
        assert_eq!(ordinary.forced_bitrate_bps(), None);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let _peer = TcpStream::connect(addr).await.expect("connect peer");
        let (stream, peer_addr) = listener.accept().await.expect("accept stream");

        let backbone = TcpServer::accepted_client(
            peer_addr.to_string(),
            stream,
            1_048_576,
            TcpSocketTuning::backbone(),
            Some(backbone_hdlc_watchdog()),
            Some(9_600),
        );

        assert_eq!(backbone.mtu_value(), 1_048_576);
        assert_eq!(backbone.socket_tuning().nodelay, Some(true));
        assert!(backbone.hdlc_liveness_enabled());
        assert_eq!(backbone.forced_bitrate_bps(), Some(9_600));
    }

    #[tokio::test]
    async fn tcp_listener_sets_reuse_address_for_ipv4() {
        let listener =
            bind_tcp_listener("127.0.0.1:0".to_string(), false).await.expect("bind listener");
        let std_listener = listener.into_std().expect("std listener");
        let socket: socket2::Socket = std_listener.into();

        assert!(socket.reuse_address().expect("reuse_address"));
    }

    #[tokio::test]
    async fn tcp_listener_sets_reuse_address_for_ipv6() {
        let listener = match bind_tcp_listener("[::1]:0".to_string(), true).await {
            Ok(listener) => listener,
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
                ) =>
            {
                return;
            }
            Err(err) => panic!("bind IPv6 listener: {err}"),
        };
        let std_listener = listener.into_std().expect("std listener");
        let socket: socket2::Socket = std_listener.into();

        assert!(socket.reuse_address().expect("reuse_address"));
    }
}

use super::*;
use crate::iface::{IfaceRole, InterfaceManager, InterfaceSharedConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

const CREDENTIAL: &str = "lora-supervisor-test-credential";

struct FailedEndpoint {
    manager: InterfaceManager,
    address: crate::hash::AddressHash,
    endpoint: String,
    status: LoraRuntimeStatusHandle,
    cancel: CancellationToken,
    stop: CancellationToken,
    task: JoinHandle<()>,
    _root: tempfile::TempDir,
    _reserved: Option<TcpSocket>,
}

impl FailedEndpoint {
    fn start(tcp: bool) -> Self {
        let root = tempfile::tempdir().expect("owned missing-device directory");
        let reserved = tcp.then(|| {
            let socket = TcpSocket::new_v4().expect("owned unlistened socket");
            socket.bind("127.0.0.1:0".parse().expect("loopback")).expect("reserve refusal port");
            socket
        });
        let interface = if let Some(socket) = reserved.as_ref() {
            LoraInterface::new_tcp(
                socket.local_addr().expect("refused endpoint").to_string(),
                LoraConfig::us915_default(),
            )
        } else {
            LoraInterface::new(
                root.path().join("missing-owned-lora").to_str().expect("owned device").to_owned(),
                115_200,
                LoraConfig::us915_default(),
            )
        }
        .with_reconnect_backoff(Duration::from_secs(60))
        .with_max_reconnect_backoff(Duration::from_secs(60));
        let endpoint = interface.endpoint().to_owned();
        let mut manager = InterfaceManager::new(1);
        let mut context = manager.new_context_with_role(interface, IfaceRole::Unicast);
        let address = *context.channel.address();
        assert!(manager
            .try_set_shared_config(
                address,
                InterfaceSharedConfig {
                    network_name: Some("lora-supervisor-network".into()),
                    passphrase: Some(CREDENTIAL.into()),
                    ..Default::default()
                }
            )
            .expect("prepared native eight-byte-default IFAC"));
        let status = LoraRuntimeStatusHandle::new(context.inner.clone());
        let stop = context.channel.stop.clone();
        // Public contexts can supply independent global and interface stops.
        let cancel = CancellationToken::new();
        context.cancel = cancel.clone();
        let task = tokio::spawn(LoraInterface::spawn(context));
        Self {
            manager,
            address,
            endpoint,
            status,
            cancel,
            stop,
            task,
            _root: root,
            _reserved: reserved,
        }
    }

    async fn failure(&self) -> serde_json::Value {
        loop {
            let status = self.status.to_json();
            if status["last_command_error"].is_string() {
                return status;
            }
            sleep(Duration::from_millis(5)).await;
        }
    }

    async fn abort_and_reap(mut self) {
        self.cancel.cancel();
        self.stop.cancel();
        self.task.abort();
        let result = self.task.await;
        assert!(result.is_ok() || result.is_err_and(|error| error.is_cancelled()));
        self.manager.stop_interface(self.address);
        assert!(self.manager.interface_hashes().is_empty());
    }
}

async fn opening_failure_is_published(tcp: bool) {
    let fixture = FailedEndpoint::start(tcp);
    let result = timeout(Duration::from_secs(2), fixture.failure()).await;
    let endpoint = fixture.endpoint.clone();
    fixture.abort_and_reap().await;
    let status = result.expect("real native open/connect failure must publish before retry");
    assert_eq!(status["online"], false);
    let error = status["last_command_error"].as_str().expect("published native opening error");
    assert!(error.contains(&endpoint), "opening error must identify the native endpoint");
    assert!(error.contains(if tcp { "connect" } else { "open" }));
    assert!(!status.to_string().contains(CREDENTIAL));
}

#[tokio::test]
async fn lora_serial_open_failure_is_published_before_retry() {
    opening_failure_is_published(false).await;
}

#[tokio::test]
async fn lora_tcp_connect_failure_is_published_before_retry() {
    opening_failure_is_published(true).await;
}

async fn retry_backoff_stops(tcp: bool, interface_stop: bool) {
    let mut fixture = FailedEndpoint::start(tcp);
    let failure = timeout(Duration::from_secs(2), fixture.failure()).await;
    if failure.is_err() {
        fixture.abort_and_reap().await;
        panic!("native opening failure must establish the retry barrier");
    }
    assert!(!fixture.cancel.is_cancelled());
    assert!(!fixture.stop.is_cancelled());
    if interface_stop {
        fixture.stop.cancel();
        assert!(!fixture.cancel.is_cancelled(), "independent interface stop");
    } else {
        fixture.cancel.cancel();
        assert!(!fixture.stop.is_cancelled(), "independent global cancellation");
    }
    let joined = timeout(Duration::from_secs(2), &mut fixture.task).await;
    if joined.is_err() {
        fixture.abort_and_reap().await;
        panic!("native retry sleep must observe stop without waiting sixty seconds");
    }
    let completion = joined.expect("native retry join deadline");
    let status = fixture.status.to_json();
    assert_eq!(status["online"], false);
    assert!(status["last_command_error"].is_string(), "retain the native failure context");
    assert!(fixture.stop.is_cancelled(), "native worker retires its channel on completion");
    assert_eq!(fixture.manager.online(&fixture.address), Some(false));
    assert_eq!(
        fixture
            .manager
            .shared_config(&fixture.address)
            .expect("prepared policy remains")
            .passphrase
            .as_deref(),
        Some(CREDENTIAL)
    );
    fixture.manager.stop_interface(fixture.address);
    assert!(fixture.manager.interface_hashes().is_empty());
    completion.expect("native retry worker joined normally");
}

#[tokio::test]
async fn lora_serial_retry_backoff_stops_on_global_cancellation() {
    retry_backoff_stops(false, false).await;
}

#[tokio::test]
async fn lora_serial_retry_backoff_stops_on_interface_stop() {
    retry_backoff_stops(false, true).await;
}

#[tokio::test]
async fn lora_tcp_retry_backoff_stops_on_global_cancellation() {
    retry_backoff_stops(true, false).await;
}

#[tokio::test]
async fn lora_tcp_retry_backoff_stops_on_interface_stop() {
    retry_backoff_stops(true, true).await;
}

async fn active_stream_stops(interface_stop: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("owned native RNode peer");
    let config = LoraConfig::us915_default();
    // A nonresponsive fake cannot let the normal startup deadline mask the stop under test.
    let interface =
        LoraInterface::new_tcp(listener.local_addr().expect("owned endpoint").to_string(), config)
            .with_startup_response_timeout(Duration::from_secs(60));
    let mut manager = InterfaceManager::new(1);
    let mut context = manager.new_context_with_role(interface, IfaceRole::Unicast);
    let address = *context.channel.address();
    let stop = context.channel.stop.clone();
    let cancel = CancellationToken::new();
    context.cancel = cancel.clone();
    let weak_interface = Arc::downgrade(&context.inner);
    let mut task = tokio::spawn(LoraInterface::spawn(context));
    let (mut peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("native active connection deadline")
        .expect("native active connection");
    // These bytes are an entry barrier, not independent protocol-conformance evidence.
    let expected = config.command_frames().concat();
    let mut received = vec![0; expected.len()];
    timeout(Duration::from_secs(2), peer.read_exact(&mut received))
        .await
        .expect("native initial I/O barrier deadline")
        .expect("native initial I/O barrier");
    assert_eq!(received, expected);
    assert!(weak_interface.upgrade().is_some(), "native stream/monitor owner is live");
    if interface_stop {
        stop.cancel();
        assert!(!cancel.is_cancelled(), "independent interface stop");
    } else {
        cancel.cancel();
        assert!(!stop.is_cancelled(), "independent global cancellation");
    }
    let joined = timeout(Duration::from_secs(2), &mut task).await;
    let within_bound = joined.is_ok();
    let completion = match joined {
        Ok(result) => result,
        Err(_) => {
            // Settle the old implementation's stream and probe monitor before reporting red.
            cancel.cancel();
            stop.cancel();
            match timeout(Duration::from_secs(2), &mut task).await {
                Ok(result) => result,
                Err(_) => {
                    task.abort();
                    task.await
                }
            }
        }
    };
    let mut trailing = Vec::new();
    timeout(Duration::from_secs(2), peer.read_to_end(&mut trailing))
        .await
        .expect("native active peer EOF deadline")
        .expect("native active peer EOF");
    peer.shutdown().await.expect("close owned peer");
    manager.stop_interface(address);
    assert!(manager.interface_hashes().is_empty());
    completion.expect("native active owner joined without abandoning probe monitor");
    assert!(weak_interface.upgrade().is_none(), "stream and probe monitor released their owner");
    assert!(within_bound, "active native LoRa stream must observe either stop input");
}

#[tokio::test]
async fn lora_active_stream_stops_on_global_cancellation() {
    active_stream_stops(false).await;
}

#[tokio::test]
async fn lora_active_stream_stops_on_interface_stop() {
    active_stream_stops(true).await;
}

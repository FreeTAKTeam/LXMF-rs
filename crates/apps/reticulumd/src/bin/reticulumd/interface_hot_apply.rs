use rns_rpc::{InterfaceMutationBridge, InterfaceRecord};
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::InterfaceManager;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, error::TrySendError, Receiver, Sender};

use crate::bootstrap::{
    mark_interface_runtime_fields, mark_interface_runtime_managed, mark_interface_startup_status,
};

#[derive(Clone)]
pub(super) struct TcpInterfaceMutationBridge {
    tx: Sender<TcpInterfaceCommand>,
}

const TCP_INTERFACE_MUTATION_QUEUE_CAPACITY: usize = 64;

impl TcpInterfaceMutationBridge {
    pub(super) fn spawn(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
    ) -> Self {
        let (tx, rx) = channel(TCP_INTERFACE_MUTATION_QUEUE_CAPACITY);
        tokio::spawn(run_tcp_interface_mutation_worker(iface_manager, rx, seeded));
        Self { tx }
    }
}

impl InterfaceMutationBridge for TcpInterfaceMutationBridge {
    fn apply_interfaces(
        &self,
        interfaces: Vec<InterfaceRecord>,
    ) -> Result<Vec<InterfaceRecord>, io::Error> {
        let effective = interfaces
            .iter()
            .cloned()
            .map(|mut record| {
                if record.kind == "tcp_client" && record.enabled {
                    mark_interface_startup_status(&mut record, "spawned", None, None);
                    mark_interface_runtime_managed(&mut record, "daemon_transport");
                    mark_interface_runtime_fields(&mut record, "running", 0);
                }
                record
            })
            .collect::<Vec<_>>();
        self.tx.try_send(TcpInterfaceCommand::Apply { interfaces }).map_err(
            |error| match error {
                TrySendError::Full(_) => {
                    io::Error::new(io::ErrorKind::WouldBlock, "interface mutation queue is full")
                }
                TrySendError::Closed(_) => io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "interface mutation worker is not running",
                ),
            },
        )?;
        Ok(effective)
    }
}

enum TcpInterfaceCommand {
    Apply { interfaces: Vec<InterfaceRecord> },
}

#[derive(Clone)]
struct ManagedTcpInterface {
    record: InterfaceRecord,
    address: AddressHash,
}

async fn run_tcp_interface_mutation_worker(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    mut rx: Receiver<TcpInterfaceCommand>,
    seeded: Vec<(String, InterfaceRecord, AddressHash)>,
) {
    let mut managed = seeded
        .into_iter()
        .map(|(key, record, address)| (key, ManagedTcpInterface { record, address }))
        .collect::<HashMap<_, _>>();

    while let Some(command) = rx.recv().await {
        match command {
            TcpInterfaceCommand::Apply { interfaces } => {
                apply_tcp_interface_records(&iface_manager, &mut managed, interfaces).await;
            }
        }
    }
}

async fn apply_tcp_interface_records(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    managed: &mut HashMap<String, ManagedTcpInterface>,
    interfaces: Vec<InterfaceRecord>,
) {
    let desired = interfaces
        .into_iter()
        .filter_map(|record| {
            let key = tcp_interface_key(&record)?;
            Some((key, record))
        })
        .collect::<HashMap<_, _>>();

    let current_keys = managed.keys().cloned().collect::<Vec<_>>();
    for key in current_keys {
        let should_remove = match (managed.get(&key), desired.get(&key)) {
            (Some(current), Some(next)) => {
                !next.enabled || tcp_interface_record_changed(&current.record, next)
            }
            (Some(_), None) => true,
            (None, _) => false,
        };
        if should_remove {
            if let Some(current) = managed.remove(&key) {
                let mut guard = iface_manager.lock().await;
                let _ = guard.stop_interface(current.address);
            }
        }
    }

    for (key, record) in desired {
        if !record.enabled || managed.contains_key(&key) {
            continue;
        }
        let Some(endpoint) = tcp_endpoint(&record) else {
            continue;
        };
        let address = {
            let mut guard = iface_manager.lock().await;
            guard.spawn(TcpClient::new(endpoint), TcpClient::spawn)
        };
        managed.insert(key, ManagedTcpInterface { record, address });
    }
}

pub(super) fn tcp_interface_key(record: &InterfaceRecord) -> Option<String> {
    if record.kind != "tcp_client" {
        return None;
    }
    if let Some(name) =
        record.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
    {
        return Some(name.to_string());
    }
    let host = record.host.as_ref()?.trim();
    let port = record.port?;
    Some(format!("{host}:{port}"))
}

fn tcp_interface_record_changed(current: &InterfaceRecord, next: &InterfaceRecord) -> bool {
    current.enabled != next.enabled || current.host != next.host || current.port != next.port
}

fn tcp_endpoint(record: &InterfaceRecord) -> Option<String> {
    Some(format!("{}:{}", record.host.as_ref()?, record.port?))
}

#[cfg(test)]
mod tests {
    use super::{
        InterfaceManager, InterfaceMutationBridge, InterfaceRecord, TcpInterfaceMutationBridge,
    };
    use std::io;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::time::{timeout, Duration};

    fn tcp_record(name: &str, host: &str, port: u16) -> InterfaceRecord {
        InterfaceRecord {
            kind: "tcp_client".to_string(),
            enabled: true,
            host: Some(host.to_string()),
            port: Some(port),
            name: Some(name.to_string()),
            settings: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn hot_apply_spawns_tcp_client_connections() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let bridge = TcpInterfaceMutationBridge::spawn(iface_manager, Vec::new());

        let applied = bridge
            .apply_interfaces(vec![tcp_record("loopback", "127.0.0.1", addr.port())])
            .expect("apply interfaces");
        assert_eq!(applied.len(), 1);
        let runtime = applied[0]
            .settings
            .as_ref()
            .and_then(|value| value.get("_runtime"))
            .expect("runtime metadata");
        assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
        assert_eq!(runtime.get("runtime_status").and_then(|value| value.as_str()), Some("running"));

        let accept = timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("tcp client should connect");
        let (_stream, peer_addr) = accept.expect("accept connection");
        assert!(peer_addr.ip().is_loopback());
    }

    #[test]
    fn hot_apply_queue_is_bounded_and_reports_pressure() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let bridge = TcpInterfaceMutationBridge { tx };

        bridge
            .apply_interfaces(vec![tcp_record("first", "127.0.0.1", 1)])
            .expect("first command fits bounded queue");
        let err = bridge
            .apply_interfaces(vec![tcp_record("second", "127.0.0.1", 2)])
            .expect_err("second command should hit queue capacity");

        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
        assert!(err.to_string().contains("interface mutation queue is full"));
    }
}

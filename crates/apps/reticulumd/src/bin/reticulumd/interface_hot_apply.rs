use rns_rpc::{InterfaceMutationBridge, InterfaceRecord};
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::{IfaceRole, InterfaceManager, InterfaceMode, InterfaceSharedConfig};
use serde_json::Value as JsonValue;
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
        if !record.enabled {
            continue;
        }
        if let Some(current) = managed.get_mut(&key) {
            let mut guard = iface_manager.lock().await;
            apply_record_runtime_config(&mut guard, current.address, &record);
            current.record = record;
            continue;
        }
        let Some(endpoint) = tcp_endpoint(&record) else {
            continue;
        };
        let address = {
            let mut guard = iface_manager.lock().await;
            let mode = interface_record_mode(&record);
            let address = guard.spawn_as_with_mode(
                TcpClient::new(endpoint),
                TcpClient::spawn,
                IfaceRole::Unicast,
                mode,
            );
            apply_record_runtime_config(&mut guard, address, &record);
            address
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

fn apply_record_runtime_config(
    manager: &mut InterfaceManager,
    address: AddressHash,
    record: &InterfaceRecord,
) {
    manager.set_mode(address, interface_record_mode(record));
    manager.set_outgoing(address, setting_bool(record, "outgoing").unwrap_or(true));
    manager.set_announce_pacing(
        address,
        setting_u64(record, "bitrate").unwrap_or(62_500),
        setting_u64(record, "announce_cap").unwrap_or(2),
    );
    manager.set_shared_config(address, interface_record_shared_config(record));
}

fn interface_record_mode(record: &InterfaceRecord) -> InterfaceMode {
    setting_str(record, "interface_mode")
        .or_else(|| setting_str(record, "mode"))
        .and_then(|value| InterfaceMode::parse(value).ok().flatten())
        .unwrap_or(InterfaceMode::Full)
}

fn interface_record_shared_config(record: &InterfaceRecord) -> InterfaceSharedConfig {
    InterfaceSharedConfig {
        announce_rate_target: setting_u64(record, "announce_rate_target"),
        announce_rate_grace: setting_u64(record, "announce_rate_grace"),
        announce_rate_penalty: setting_u64(record, "announce_rate_penalty"),
        bootstrap_only: setting_bool(record, "bootstrap_only"),
        ifac_size: setting_u64(record, "ifac_size"),
        network_name: setting_string(record, "network_name")
            .or_else(|| setting_string(record, "networkname")),
        passphrase: setting_string(record, "passphrase")
            .or_else(|| setting_string(record, "pass_phrase")),
        ingress_control: setting_bool(record, "ingress_control"),
        egress_control: setting_bool(record, "egress_control"),
        ic_max_held_announces: setting_u64(record, "ic_max_held_announces"),
        ic_burst_hold: setting_f64(record, "ic_burst_hold"),
        ic_burst_freq_new: setting_f64(record, "ic_burst_freq_new"),
        ic_burst_freq: setting_f64(record, "ic_burst_freq"),
        ic_pr_burst_freq_new: setting_f64(record, "ic_pr_burst_freq_new"),
        ic_pr_burst_freq: setting_f64(record, "ic_pr_burst_freq"),
        ec_pr_freq: setting_f64(record, "ec_pr_freq"),
        ic_new_time: setting_f64(record, "ic_new_time"),
        ic_burst_penalty: setting_f64(record, "ic_burst_penalty"),
        ic_held_release_interval: setting_f64(record, "ic_held_release_interval"),
        discoverable: setting_bool(record, "discoverable"),
        announce_interval: setting_u64(record, "announce_interval"),
        discovery_stamp_value: setting_u64(record, "discovery_stamp_value"),
        discovery_name: setting_string(record, "discovery_name"),
        discovery_encrypt: setting_bool(record, "discovery_encrypt"),
        reachable_on: setting_string(record, "reachable_on"),
        publish_ifac: setting_bool(record, "publish_ifac"),
        latitude: setting_f64(record, "latitude"),
        longitude: setting_f64(record, "longitude"),
        height: setting_f64(record, "height"),
        discovery_frequency: setting_u64(record, "discovery_frequency"),
        discovery_bandwidth: setting_u64(record, "discovery_bandwidth"),
        discovery_modulation: setting_u64(record, "discovery_modulation"),
    }
}

fn setting<'a>(record: &'a InterfaceRecord, key: &str) -> Option<&'a JsonValue> {
    record.settings.as_ref()?.as_object()?.get(key)
}

fn setting_str<'a>(record: &'a InterfaceRecord, key: &str) -> Option<&'a str> {
    setting(record, key)?.as_str()
}

fn setting_string(record: &InterfaceRecord, key: &str) -> Option<String> {
    setting_str(record, key).map(ToOwned::to_owned)
}

fn setting_bool(record: &InterfaceRecord, key: &str) -> Option<bool> {
    setting(record, key)?.as_bool()
}

fn setting_u64(record: &InterfaceRecord, key: &str) -> Option<u64> {
    setting(record, key)?.as_u64()
}

fn setting_f64(record: &InterfaceRecord, key: &str) -> Option<f64> {
    setting(record, key)?.as_f64()
}

#[cfg(test)]
mod tests {
    use super::{
        apply_tcp_interface_records, InterfaceManager, InterfaceMutationBridge, InterfaceRecord,
        ManagedTcpInterface, TcpInterfaceMutationBridge,
    };
    use rns_transport::iface::{InterfaceMode, InterfaceSharedConfig};
    use serde_json::json;
    use std::collections::HashMap;
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

    #[tokio::test(flavor = "current_thread")]
    async fn hot_apply_spawns_tcp_client_with_record_runtime_settings() {
        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let mut managed = HashMap::new();
        let mut record = tcp_record("loopback", "127.0.0.1", 1);
        record.settings = Some(json!({
            "interface_mode": "gateway",
            "outgoing": false,
            "bitrate": 1200,
            "announce_cap": 5,
            "announce_rate_target": 120,
            "announce_rate_grace": 2,
            "announce_rate_penalty": 30,
            "network_name": "field-net",
            "discoverable": true,
            "announce_interval": 21600
        }));

        apply_tcp_interface_records(&iface_manager, &mut managed, vec![record]).await;

        let address = managed.get("loopback").expect("managed tcp client").address;
        let manager = iface_manager.lock().await;
        assert_eq!(manager.mode(&address), Some(InterfaceMode::Gateway));
        assert_eq!(manager.outgoing(&address), Some(false));
        assert_eq!(manager.announce_pacing(&address), Some((1200, 5)));
        assert_eq!(
            manager.shared_config(&address),
            Some(&InterfaceSharedConfig {
                announce_rate_target: Some(120),
                announce_rate_grace: Some(2),
                announce_rate_penalty: Some(30),
                network_name: Some("field-net".to_string()),
                discoverable: Some(true),
                announce_interval: Some(21_600),
                ..InterfaceSharedConfig::default()
            })
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn hot_apply_updates_existing_tcp_client_runtime_settings() {
        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let address = {
            let mut manager = iface_manager.lock().await;
            *manager.new_channel(8).address()
        };
        let mut managed = HashMap::from([(
            "loopback".to_string(),
            ManagedTcpInterface { record: tcp_record("loopback", "127.0.0.1", 1), address },
        )]);
        let mut record = tcp_record("loopback", "127.0.0.1", 1);
        record.settings = Some(json!({
            "interface_mode": "access_point",
            "outgoing": false,
            "passphrase": "shared-secret",
            "publish_ifac": true
        }));

        apply_tcp_interface_records(&iface_manager, &mut managed, vec![record]).await;

        let manager = iface_manager.lock().await;
        assert_eq!(manager.mode(&address), Some(InterfaceMode::AccessPoint));
        assert_eq!(manager.outgoing(&address), Some(false));
        assert_eq!(
            manager.shared_config(&address),
            Some(&InterfaceSharedConfig {
                passphrase: Some("shared-secret".to_string()),
                publish_ifac: Some(true),
                ..InterfaceSharedConfig::default()
            })
        );
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

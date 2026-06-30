use rns_rpc::{InterfaceMutationBridge, InterfaceRecord};
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::udp::UdpInterface;
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
pub(super) struct InterfaceHotApplyBridge {
    tx: Sender<InterfaceHotApplyCommand>,
}

const INTERFACE_HOT_APPLY_QUEUE_CAPACITY: usize = 64;

impl InterfaceHotApplyBridge {
    pub(super) fn spawn(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
    ) -> Self {
        let (tx, rx) = channel(INTERFACE_HOT_APPLY_QUEUE_CAPACITY);
        tokio::spawn(run_interface_mutation_worker(iface_manager, rx, seeded));
        Self { tx }
    }
}

impl InterfaceMutationBridge for InterfaceHotApplyBridge {
    fn apply_interfaces(
        &self,
        interfaces: Vec<InterfaceRecord>,
    ) -> Result<Vec<InterfaceRecord>, io::Error> {
        validate_hot_apply_uniqueness(&interfaces)?;
        let effective = interfaces
            .iter()
            .cloned()
            .map(|mut record| {
                if matches!(record.kind.as_str(), "tcp_client" | "udp") && record.enabled {
                    mark_interface_startup_status(&mut record, "spawned", None, None);
                    mark_interface_runtime_managed(&mut record, "daemon_transport");
                    mark_interface_runtime_fields(&mut record, "running", 0);
                    if record.kind == "udp" {
                        mark_udp_record_runtime_status(&mut record, None);
                    }
                }
                record
            })
            .collect::<Vec<_>>();
        self.tx.try_send(InterfaceHotApplyCommand::Apply { interfaces }).map_err(|error| {
            match error {
                TrySendError::Full(_) => {
                    io::Error::new(io::ErrorKind::WouldBlock, "interface mutation queue is full")
                }
                TrySendError::Closed(_) => io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "interface mutation worker is not running",
                ),
            }
        })?;
        Ok(effective)
    }
}

enum InterfaceHotApplyCommand {
    Apply { interfaces: Vec<InterfaceRecord> },
}

#[derive(Clone)]
struct ManagedHotApplyInterface {
    record: InterfaceRecord,
    address: AddressHash,
}

async fn run_interface_mutation_worker(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    mut rx: Receiver<InterfaceHotApplyCommand>,
    seeded: Vec<(String, InterfaceRecord, AddressHash)>,
) {
    let mut managed = seeded
        .into_iter()
        .map(|(key, record, address)| (key, ManagedHotApplyInterface { record, address }))
        .collect::<HashMap<_, _>>();

    while let Some(command) = rx.recv().await {
        match command {
            InterfaceHotApplyCommand::Apply { interfaces } => {
                apply_hot_apply_interface_records(&iface_manager, &mut managed, interfaces).await;
            }
        }
    }
}

async fn apply_hot_apply_interface_records(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    managed: &mut HashMap<String, ManagedHotApplyInterface>,
    interfaces: Vec<InterfaceRecord>,
) {
    let desired = interfaces
        .into_iter()
        .filter_map(|record| {
            let key = hot_apply_interface_key(&record)?;
            Some((key, record))
        })
        .collect::<HashMap<_, _>>();

    let current_keys = managed.keys().cloned().collect::<Vec<_>>();
    for key in current_keys {
        let should_remove = match (managed.get(&key), desired.get(&key)) {
            (Some(current), Some(next)) => {
                !next.enabled || hot_apply_interface_record_changed(&current.record, next)
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
        if let Some(address) = spawn_hot_apply_interface(iface_manager, &record).await {
            managed.insert(key, ManagedHotApplyInterface { record, address });
        }
    }
}

fn validate_hot_apply_uniqueness(interfaces: &[InterfaceRecord]) -> Result<(), io::Error> {
    let mut seen = std::collections::HashSet::new();
    let mut seen_udp_bind_addresses = std::collections::HashSet::new();
    for record in interfaces {
        if record.kind == "udp" {
            let (bind_addr, _) = udp_bind_and_forward_addr(record)?;
            if record.enabled && !seen_udp_bind_addresses.insert(bind_addr.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("duplicate udp bind address '{bind_addr}'"),
                ));
            }
        }
        let Some(key) = hot_apply_interface_key(record) else {
            continue;
        };
        if !seen.insert(key.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate hot-apply interface key '{key}'"),
            ));
        }
    }
    Ok(())
}

async fn spawn_hot_apply_interface(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    record: &InterfaceRecord,
) -> Option<AddressHash> {
    let mut guard = iface_manager.lock().await;
    let mode = interface_record_mode(record);
    let address = match record.kind.as_str() {
        "tcp_client" => guard.spawn_as_with_mode(
            TcpClient::new(tcp_endpoint(record)?),
            TcpClient::spawn,
            IfaceRole::Unicast,
            mode,
        ),
        "udp" => {
            let (bind_addr, forward_addr) = udp_bind_and_forward_addr(record).ok()?;
            let adapter = UdpInterface::new(bind_addr, forward_addr);
            if adapter.is_multicast() {
                return None;
            }
            guard.spawn_as_with_mode(adapter, UdpInterface::spawn, IfaceRole::Unicast, mode)
        }
        _ => return None,
    };
    apply_record_runtime_config(&mut guard, address, record);
    Some(address)
}

fn hot_apply_interface_key(record: &InterfaceRecord) -> Option<String> {
    match record.kind.as_str() {
        "tcp_client" => tcp_interface_key(record),
        "udp" => udp_interface_key(record),
        _ => None,
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

pub(super) fn hot_apply_interface_seed_key(record: &InterfaceRecord) -> Option<String> {
    match record.kind.as_str() {
        "tcp_client" => tcp_interface_key(record),
        "udp" => {
            udp_bind_and_forward_addr(record).ok()?;
            udp_interface_key(record)
        }
        _ => None,
    }
}

fn udp_interface_key(record: &InterfaceRecord) -> Option<String> {
    if record.kind != "udp" {
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

fn hot_apply_interface_record_changed(current: &InterfaceRecord, next: &InterfaceRecord) -> bool {
    current.kind != next.kind
        || current.enabled != next.enabled
        || current.host != next.host
        || current.port != next.port
        || (current.kind == "udp" && udp_forward_addr(current) != udp_forward_addr(next))
}

fn tcp_endpoint(record: &InterfaceRecord) -> Option<String> {
    Some(format!("{}:{}", record.host.as_ref()?, record.port?))
}

fn udp_bind_and_forward_addr(
    record: &InterfaceRecord,
) -> Result<(String, Option<String>), io::Error> {
    let host = record.host.as_deref().map(str::trim).filter(|value| !value.is_empty()).ok_or_else(
        || io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply requires host"),
    )?;
    if setting_str(record, "device").is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "udp hot-apply does not support device-bound records",
        ));
    }
    if host_is_multicast(host) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "udp hot-apply does not support multicast",
        ));
    }
    let port = record.port.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply requires port")
    })?;
    Ok((format!("{host}:{port}"), udp_forward_addr_result(record)?))
}

fn udp_forward_addr(record: &InterfaceRecord) -> Option<String> {
    udp_forward_addr_result(record).ok().flatten()
}

fn udp_forward_addr_result(record: &InterfaceRecord) -> Result<Option<String>, io::Error> {
    let host = setting_str(record, "target_host").or_else(|| setting_str(record, "forward_ip"));
    let port = setting_u64(record, "target_port").or_else(|| setting_u64(record, "forward_port"));
    let (host, port) = match (host, port) {
        (Some(host), Some(port)) => (host, port),
        (None, None) => return Ok(None),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "udp hot-apply target_host and target_port must be provided together",
            ))
        }
    };
    if host_is_multicast(host) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "udp hot-apply does not support multicast targets",
        ));
    }
    let port = u16::try_from(port).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply target_port is out of range")
    })?;
    Ok(Some(format!("{host}:{port}")))
}

fn mark_udp_record_runtime_status(
    record: &mut InterfaceRecord,
    runtime_iface: Option<AddressHash>,
) {
    if let Ok((bind_addr, forward_addr)) = udp_bind_and_forward_addr(record) {
        let role = if forward_addr.is_some() { "peer" } else { "listener" };
        let iface = runtime_iface.map(|value| value.to_string());
        crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
            runtime.insert(
                "udp".to_string(),
                serde_json::json!({
                    "status": {
                        "link_state": "configured",
                        "role": role,
                        "bind_addr": bind_addr,
                        "forward_addr": forward_addr,
                        "iface": iface,
                    }
                }),
            );
        });
    }
}

fn host_is_multicast(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_multicast())
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
#[path = "tests/interface_hot_apply.rs"]
mod tests;

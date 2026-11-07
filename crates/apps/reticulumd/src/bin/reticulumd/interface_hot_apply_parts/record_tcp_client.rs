use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use rns_rpc::InterfaceRecord;
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use serde_json::{json, Map, Value};
use std::io;
use tokio::sync::mpsc::Sender;

const OPTIONS: [&str; 5] =
    ["connect_timeout", "max_reconnect_tries", "prefer_ipv6", "i2p_tunneled", "mtu"];

fn invalid_options() -> io::Error {
    // Schema errors can include submitted values; keep this trust-boundary diagnostic value-free.
    io::Error::new(io::ErrorKind::InvalidInput, "invalid tcp_client startup options")
}

pub(super) fn configuration(record: &InterfaceRecord) -> io::Result<InterfaceConfig> {
    let mut projected = Map::new();
    projected.insert("type".into(), Value::String(record.kind.clone()));
    projected.insert("enabled".into(), Value::Bool(true));
    if let Some(host) = &record.host {
        projected.insert("host".into(), json!(host));
    }
    if let Some(port) = record.port {
        projected.insert("port".into(), json!(port));
    }
    for key in OPTIONS {
        if let Some(value) = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get(key))
            .filter(|value| !value.is_null())
        {
            projected.insert(key.into(), value.clone());
        }
    }
    // DaemonConfig owns normalization and validation; InterfaceConfig deserialize alone does not.
    let config: DaemonConfig = serde_json::from_value(json!({
        "reticulum": {"share_instance": false}, "interfaces": [projected],
    }))
    .map_err(|_| invalid_options())?;
    config.interfaces.into_iter().next().ok_or_else(invalid_options)
}

pub(in crate::interface_hot_apply) fn adapter(
    record: &InterfaceRecord,
    reconnect_events: Option<&Sender<AddressHash>>,
) -> io::Result<TcpClient> {
    let config = configuration(record)?;
    let endpoint = super::record_hot_apply::tcp_endpoint(record).ok_or_else(invalid_options)?;
    Ok(crate::bootstrap::build_tcp_client_adapter(endpoint, &config, reconnect_events.cloned()))
}

pub(super) fn changed(current: &InterfaceRecord, next: &InterfaceRecord) -> bool {
    let signature = |record: &InterfaceRecord| {
        adapter(record, None).map(|adapter| {
            (
                adapter.connect_timeout(),
                adapter.max_reconnect_tries(),
                adapter.prefer_ipv6(),
                adapter.socket_tuning(),
                adapter.mtu_value(),
            )
        })
    };
    match (signature(current), signature(next)) {
        (Ok(current), Ok(next)) => current != next,
        _ => true,
    }
}

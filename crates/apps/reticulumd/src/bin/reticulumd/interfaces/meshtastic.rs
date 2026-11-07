use std::time::Duration;

use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::meshtastic::{
    MeshtasticInterfaceConfig, MeshtasticInterfaceHandle, MeshtasticReceivedFrame,
};

pub(crate) struct MeshtasticDaemonConfig {
    pub(crate) transport: MeshtasticInterfaceConfig,
    pub(crate) simulation_loopback: bool,
    pub(crate) simulation_node_id: u32,
}

pub(crate) fn build_config(iface: &InterfaceConfig) -> MeshtasticDaemonConfig {
    let mut transport =
        iface.modem_preset.map(MeshtasticInterfaceConfig::from_modem_preset).unwrap_or_default();
    if let Some(hop_limit) = iface.hop_limit {
        transport.hop_limit = hop_limit;
    }
    if let Some(bitrate) = iface.bitrate {
        transport.bitrate_bps = bitrate;
    }
    if let Some(max_payload_bytes) = iface.max_payload_bytes {
        transport.max_payload_bytes = usize::from(max_payload_bytes);
    }
    if let Some(send_delay_ms) = iface.send_delay_ms {
        transport.send_delay = Duration::from_millis(send_delay_ms);
    }
    if let Some(destination_cache_size) = iface.destination_cache_size {
        transport.destination_cache_size = destination_cache_size;
    }
    MeshtasticDaemonConfig {
        transport,
        simulation_loopback: iface.simulation_loopback.unwrap_or(false),
        simulation_node_id: iface.simulation_node_id.unwrap_or(1),
    }
}

pub(crate) fn spawn_simulation_loopback(
    handle: MeshtasticInterfaceHandle,
    node_id: u32,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(frame) = handle.recv_transmit().await {
            if handle
                .inject_received(MeshtasticReceivedFrame::new(node_id, &frame.payload))
                .await
                .is_err()
            {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_uses_modem_preset_then_explicit_overrides() {
        let iface = InterfaceConfig {
            kind: "meshtastic".to_string(),
            modem_preset: Some(8),
            hop_limit: Some(5),
            bitrate: Some(1_200),
            max_payload_bytes: Some(180),
            send_delay_ms: Some(25),
            destination_cache_size: Some(9),
            simulation_loopback: Some(true),
            simulation_node_id: Some(42),
            ..InterfaceConfig::default()
        };
        let config = build_config(&iface);
        assert_eq!(config.transport.hop_limit, 5);
        assert_eq!(config.transport.bitrate_bps, 1_200);
        assert_eq!(config.transport.max_payload_bytes, 180);
        assert_eq!(config.transport.send_delay, Duration::from_millis(25));
        assert_eq!(config.transport.destination_cache_size, 9);
        assert!(config.simulation_loopback);
        assert_eq!(config.simulation_node_id, 42);
    }
}

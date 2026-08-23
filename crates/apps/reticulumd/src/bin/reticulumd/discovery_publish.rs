use rns_rpc::InterfaceRecord;
use rns_transport::discovery::announce::DiscoverableInterface;
use rns_transport::hash::AddressHash;
use serde_json::Value as JsonValue;

fn setting<'a>(record: &'a InterfaceRecord, key: &str) -> Option<&'a JsonValue> {
    record.settings.as_ref()?.as_object()?.get(key)
}

fn setting_str<'a>(record: &'a InterfaceRecord, key: &str) -> Option<&'a str> {
    setting(record, key)?.as_str()
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

#[derive(Debug, Clone)]
pub(crate) struct OutboundDiscoveryAnnouncement {
    pub interface: DiscoverableInterface,
    pub stamp_value: u32,
    pub interval_secs: u64,
    pub encrypted: bool,
}

pub(crate) fn configured_discovery_announcements(
    records: &[InterfaceRecord],
    transport_id: AddressHash,
    transport_enabled: bool,
    network_identity_available: bool,
) -> Vec<OutboundDiscoveryAnnouncement> {
    records
        .iter()
        .filter(|record| record.enabled && setting_bool(record, "discoverable") == Some(true))
        .filter_map(|record| configured_announcement(record, transport_id, transport_enabled))
        .filter(|candidate| {
            if candidate.encrypted && !network_identity_available {
                log::warn!(
                    "discovery encryption requested for {} without a configured [reticulum].network_identity; skipping announcement",
                    candidate.interface.name
                );
                false
            } else {
                true
            }
        })
        .collect()
}

fn configured_announcement(
    record: &InterfaceRecord,
    transport_id: AddressHash,
    transport_enabled: bool,
) -> Option<OutboundDiscoveryAnnouncement> {
    let interface_type = match record.kind.as_str() {
        "backbone" => "BackboneInterface",
        "tcp_server" => "TCPServerInterface",
        "tcp_client" => "TCPClientInterface",
        "i2p" => "I2PInterface",
        "rnode" | "rnode_multi" | "lora" => "RNodeInterface",
        "weave" => "WeaveInterface",
        "kiss" | "kiss_tcp" => "KISSInterface",
        unsupported => {
            log::warn!("discovery publishing is unsupported for interface type {unsupported}");
            return None;
        }
    };
    let operator_lxmf_address = setting_str(record, "discovery_lxmf_address")
        .and_then(|value| hex::decode(value).ok())
        .and_then(|bytes| bytes.try_into().ok());
    let publish_ifac = setting_bool(record, "publish_ifac") == Some(true);
    let mut raw_transport_id = [0_u8; 16];
    raw_transport_id.copy_from_slice(transport_id.as_slice());
    let interface = DiscoverableInterface {
        interface_type: interface_type.to_string(),
        transport: transport_enabled,
        transport_id: raw_transport_id,
        name: setting_str(record, "discovery_name")
            .or(record.name.as_deref())
            .unwrap_or(record.kind.as_str())
            .to_string(),
        latitude: setting_f64(record, "latitude"),
        longitude: setting_f64(record, "longitude"),
        height: setting_f64(record, "height"),
        operator_lxmf_address,
        reachable_on: setting_str(record, "reachable_on")
            .or(record.host.as_deref())
            .map(ToOwned::to_owned),
        port: record.port,
        ifac_netname: publish_ifac
            .then(|| setting_str(record, "network_name").or(setting_str(record, "networkname")))
            .flatten()
            .map(ToOwned::to_owned),
        ifac_netkey: publish_ifac
            .then(|| setting_str(record, "passphrase").or(setting_str(record, "pass_phrase")))
            .flatten()
            .map(ToOwned::to_owned),
        frequency: setting_u64(record, "discovery_frequency")
            .or_else(|| setting_u64(record, "frequency")),
        bandwidth: setting_u64(record, "discovery_bandwidth")
            .or_else(|| setting_u64(record, "bandwidth")),
        spreading_factor: setting_u64(record, "spreading_factor")
            .and_then(|value| u8::try_from(value).ok()),
        coding_rate: setting_u64(record, "coding_rate").and_then(|value| u8::try_from(value).ok()),
        modulation: setting_str(record, "modulation")
            .map(ToOwned::to_owned)
            .or_else(|| setting_u64(record, "discovery_modulation").map(|value| value.to_string())),
        channel: setting_u64(record, "channel"),
    };
    Some(OutboundDiscoveryAnnouncement {
        interface,
        stamp_value: setting_u64(record, "discovery_stamp_value")
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(rns_transport::discovery::announce::DEFAULT_STAMP_VALUE),
        interval_secs: setting_u64(record, "announce_interval").unwrap_or(6 * 60 * 60).max(5 * 60),
        encrypted: setting_bool(record, "discovery_encrypt") == Some(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_transport::discovery::announce::{decode_plain_announce, encode_plain_announce};
    use serde_json::json;

    #[test]
    fn rns_1_5_configured_operator_address_reaches_production_discovery_payload() {
        let record = InterfaceRecord {
            kind: "backbone".to_string(),
            enabled: true,
            host: Some("relay.example".to_string()),
            port: Some(4242),
            name: Some("backbone".to_string()),
            settings: Some(json!({
                "discoverable": true,
                "discovery_stamp_value": 1,
                "discovery_lxmf_address": "42424242424242424242424242424242"
            })),
        };
        let configured = configured_discovery_announcements(
            &[record],
            AddressHash::new_from_slice(&[0x11; 16]),
            true,
            false,
        );
        let payload = encode_plain_announce(&configured[0].interface, configured[0].stamp_value)
            .expect("encode configured discovery payload");
        let decoded = decode_plain_announce(&payload, "network", &[], 1, 1.0, 1)
            .expect("decode configured discovery payload");
        assert_eq!(
            decoded.operator_lxmf_address.as_deref(),
            Some("42424242424242424242424242424242")
        );
        assert!(!configured[0].encrypted);
    }

    #[test]
    fn rns_1_5_encrypted_discovery_requires_shared_network_identity() {
        let record = InterfaceRecord {
            kind: "backbone".to_string(),
            enabled: true,
            host: Some("relay.example".to_string()),
            port: Some(4242),
            name: Some("backbone".to_string()),
            settings: Some(json!({
                "discoverable": true,
                "discovery_encrypt": true,
                "discovery_lxmf_address": "42424242424242424242424242424242"
            })),
        };
        let configured = configured_discovery_announcements(
            std::slice::from_ref(&record),
            AddressHash::new_from_slice(&[0x11; 16]),
            true,
            false,
        );
        assert!(configured.is_empty());
        let configured = configured_discovery_announcements(
            &[record],
            AddressHash::new_from_slice(&[0x11; 16]),
            true,
            true,
        );
        assert_eq!(configured.len(), 1);
        assert!(configured[0].encrypted);
    }

    #[test]
    fn rns_1_5_tcp_client_interface_is_publishable_and_wire_valid() {
        let record = InterfaceRecord {
            kind: "tcp_client".to_string(),
            enabled: true,
            host: Some("relay.example".to_string()),
            port: Some(4242),
            name: Some("outbound-relay".to_string()),
            settings: Some(json!({
                "discoverable": true,
                "discovery_stamp_value": 1,
                "reachable_on": "relay.example"
            })),
        };
        let configured = configured_discovery_announcements(
            &[record],
            AddressHash::new_from_slice(&[0x22; 16]),
            true,
            false,
        );
        assert_eq!(configured.len(), 1);
        assert_eq!(configured[0].interface.interface_type, "TCPClientInterface");

        let payload = encode_plain_announce(&configured[0].interface, configured[0].stamp_value)
            .expect("encode TCP client discovery payload");
        let decoded = decode_plain_announce(&payload, "network", &[], 1, 1.0, 1)
            .expect("decode TCP client discovery payload");
        assert_eq!(decoded.interface_type, "TCPClientInterface");
        assert_eq!(decoded.reachable_on.as_deref(), Some("relay.example"));
        assert_eq!(decoded.port, Some(4242));
    }
}

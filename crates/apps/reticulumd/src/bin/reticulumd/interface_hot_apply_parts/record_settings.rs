use rns_rpc::InterfaceRecord;
use rns_transport::iface::InterfaceSharedConfig;
use serde_json::Value as JsonValue;

pub(crate) fn setting<'a>(record: &'a InterfaceRecord, key: &str) -> Option<&'a JsonValue> {
    record.settings.as_ref()?.as_object()?.get(key)
}

pub(crate) fn setting_str<'a>(record: &'a InterfaceRecord, key: &str) -> Option<&'a str> {
    setting(record, key)?.as_str()
}

pub(crate) fn setting_string(record: &InterfaceRecord, key: &str) -> Option<String> {
    setting_str(record, key).map(ToOwned::to_owned)
}

pub(crate) fn setting_bool(record: &InterfaceRecord, key: &str) -> Option<bool> {
    setting(record, key)?.as_bool()
}

pub(crate) fn setting_u64(record: &InterfaceRecord, key: &str) -> Option<u64> {
    setting(record, key)?.as_u64()
}

pub(crate) fn setting_i64(record: &InterfaceRecord, key: &str) -> Option<i64> {
    setting(record, key)?.as_i64()
}

pub(crate) fn setting_f64(record: &InterfaceRecord, key: &str) -> Option<f64> {
    setting(record, key)?.as_f64()
}

pub(crate) fn interface_record_shared_config(record: &InterfaceRecord) -> InterfaceSharedConfig {
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

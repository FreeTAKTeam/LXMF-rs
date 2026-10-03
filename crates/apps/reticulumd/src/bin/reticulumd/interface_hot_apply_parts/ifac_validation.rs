use rns_rpc::{InterfaceMutationFailure, InterfaceRecord};
use rns_transport::iface::InterfaceSharedConfig;
use std::io;

use super::record_settings::{setting_string, setting_u64};

pub(crate) fn validate_hot_apply_ifac_configuration(
    interfaces: &[InterfaceRecord],
) -> Result<(), io::Error> {
    for record in interfaces {
        let config = InterfaceSharedConfig {
            ifac_size: setting_u64(record, "ifac_size"),
            network_name: setting_string(record, "network_name")
                .or_else(|| setting_string(record, "networkname"))
                .or_else(|| setting_string(record, "ifac_netname")),
            passphrase: setting_string(record, "passphrase")
                .or_else(|| setting_string(record, "pass_phrase"))
                .or_else(|| setting_string(record, "ifac_netkey")),
            ..InterfaceSharedConfig::default()
        };
        if config.ifac_context().is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                InterfaceMutationFailure::InvalidIfacConfiguration,
            ));
        }
    }
    Ok(())
}

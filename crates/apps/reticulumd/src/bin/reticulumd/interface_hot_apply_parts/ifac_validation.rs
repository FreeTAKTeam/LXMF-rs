use rns_rpc::{InterfaceMutationFailure, InterfaceRecord};
use std::io;

use super::record_settings::interface_record_shared_config;

pub(crate) fn validate_hot_apply_ifac_configuration(
    interfaces: &[InterfaceRecord],
) -> Result<(), io::Error> {
    for record in interfaces {
        let config = interface_record_shared_config(record);
        if config.ifac_context().is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                InterfaceMutationFailure::InvalidIfacConfiguration,
            ));
        }
    }
    Ok(())
}

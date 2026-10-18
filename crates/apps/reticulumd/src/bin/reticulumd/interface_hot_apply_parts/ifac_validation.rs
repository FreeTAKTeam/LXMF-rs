use rns_rpc::{InterfaceMutationFailure, InterfaceRecord};
use std::io;

use super::record_settings::{
    interface_record_shared_config, IFAC_NETWORK_ALIASES, IFAC_PASSPHRASE_ALIASES,
};

pub(crate) fn validate_hot_apply_ifac_configuration(
    interfaces: &[InterfaceRecord],
) -> Result<(), io::Error> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            InterfaceMutationFailure::InvalidIfacConfiguration,
        )
    };
    for record in interfaces {
        if let Some(settings) = record.settings.as_ref().and_then(serde_json::Value::as_object) {
            let malformed_credential =
                IFAC_NETWORK_ALIASES.iter().chain(IFAC_PASSPHRASE_ALIASES.iter()).any(|key| {
                    settings.get(*key).is_some_and(|value| !value.is_null() && !value.is_string())
                });
            let malformed_size = settings
                .get("ifac_size")
                .is_some_and(|value| !value.is_null() && value.as_i64().is_none());
            if malformed_credential || malformed_size {
                return Err(invalid());
            }
        }
        let config = interface_record_shared_config(record);
        if config.ifac_context().is_err() {
            return Err(invalid());
        }
    }
    Ok(())
}

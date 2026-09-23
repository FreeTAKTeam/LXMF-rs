include!("meshtastic_parts/module_prelude.rs");

include!("meshtastic_parts/packet_handler.rs");

include!("meshtastic_parts/tunnel.rs");

include!("meshtastic_parts/interface.rs");

#[cfg(test)]
#[path = "meshtastic_parts/ifac_runtime_tests.rs"]
mod ifac_runtime_tests;

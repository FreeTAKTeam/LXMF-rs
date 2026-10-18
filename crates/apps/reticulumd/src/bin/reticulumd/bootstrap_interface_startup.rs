include!("bootstrap_interface_startup_parts/module_prelude.rs");

include!("bootstrap_interface_startup_parts/startup_udp.rs");

#[cfg(all(test, unix))]
#[path = "bootstrap_interface_startup_parts/auto_activation_tests.rs"]
mod auto_activation_tests;

include!("bootstrap_interface_startup_parts/startup_vrn76_kiss_ble.rs");

include!("bootstrap_interface_startup_parts/startup_configured.rs");

include!("bootstrap_interface_startup_parts/startup_misc.rs");

include!("bootstrap_interface_startup_parts/startup_local_server.rs");

include!("bootstrap_interface_startup_parts/startup_i2p.rs");

include!("bootstrap_interface_startup_parts/startup_radio_multi.rs");

include!("bootstrap_interface_startup_parts/startup_stream_clients.rs");

include!("bootstrap_interface_startup_parts/startup_local_attach.rs");

include!("bootstrap_interface_startup_parts/startup_serial_kiss.rs");

include!("bootstrap_interface_startup_parts/startup_ble.rs");

include!("bootstrap_interface_startup_parts/startup_runtime_status.rs");

include!("rnode_ble_parts/module_prelude.rs");

include!("rnode_ble_parts/runtime_helpers.rs");

include!("rnode_ble_parts/windows_paired.rs");

include!("rnode_ble_parts/runtime_lifecycle.rs");

include!("rnode_ble_parts/native_connection.rs");

include!("rnode_ble_parts/rnode_peripheral_matches.rs");

include!("rnode_ble_parts/runtime_startup_notification_drain.rs");

include!("rnode_ble_parts/rnodeblecommandmonitor.rs");

#[cfg(all(test, feature = "rnode-ble"))]
include!("rnode_ble_parts/runtime_tests.rs");

#[cfg(all(test, feature = "rnode-ble"))]
include!("rnode_ble_parts/ifac_worker_tests.rs");

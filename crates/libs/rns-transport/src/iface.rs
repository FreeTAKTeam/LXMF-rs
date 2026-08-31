pub mod auto;

pub mod driver;

pub mod hdlc;

pub mod i2p;

pub mod kiss;

pub mod lora;

pub mod meshtastic;

#[cfg(unix)]
pub mod local;

pub mod pipe;

pub mod rnode_multi;

pub mod rnode_ble;

pub mod rnode_bearer;

mod rnode_bearer_interface;
mod rnode_bearer_status;

pub mod rnode_spp;

pub mod serial;

pub mod tcp_client;

pub mod tcp_server;

pub mod udp;

pub mod vrn76_kiss_ble;

pub mod weave;

include!("iface_parts/module_prelude.rs");

include!("iface_types.rs");

include!("iface_parts/txmessagetype.rs");

include!("iface_parts/interface_traffic.rs");

include!("iface_runtime.rs");

include!("iface_parts/interfacemanager.rs");

include!("iface_parts/interfacemanager_management.rs");

include!("iface_parts/interfacemanager_send.rs");

include!("iface_tests.rs");

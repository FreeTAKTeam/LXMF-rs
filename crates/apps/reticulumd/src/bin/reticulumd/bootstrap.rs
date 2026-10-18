include!("bootstrap_parts/module_prelude.rs");

#[path = "bootstrap_transport.rs"]
mod transport_startup;

#[path = "bootstrap_parts/configuration.rs"]
mod configuration;

include!("bootstrap_parts/module_core.rs");

include!("bootstrap_parts/propagation_node_config.rs");

include!("bootstrap_parts/configure_startup_rpc_token_auth.rs");

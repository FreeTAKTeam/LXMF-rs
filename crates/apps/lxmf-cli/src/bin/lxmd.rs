include!("lxmd_parts/module_prelude.rs");

#[path = "lxmd/config.rs"]
mod config;

#[path = "lxmd/config_python.rs"]
mod config_python;

#[path = "lxmd/inbound.rs"]
mod inbound;

#[path = "lxmd/launch.rs"]
mod launch;

#[path = "lxmd/python_compat.rs"]
mod python_compat;

#[path = "lxmd/query.rs"]
mod query;

#[path = "lxmd/rpc_client.rs"]
mod rpc_client;

#[path = "lxmd/types.rs"]
mod types;

#[path = "../version.rs"]
mod version;

include!("lxmd_parts/support_types.rs");

include!("lxmd_parts/compatibility_notes_only_emitted_for.rs");

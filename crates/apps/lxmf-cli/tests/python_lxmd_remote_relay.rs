mod support;

include!("python_lxmd_remote_relay_parts/module_prelude.rs");

include!("python_lxmd_remote_relay_parts/python_to_rust_lxmd_relay_remote_pat.rs");

include!("python_lxmd_remote_relay_parts/ifac_daemon.rs");

include!("python_lxmd_remote_relay_parts/shared_instance_daemon.rs");

include!("python_lxmd_remote_relay_parts/python_router_restart.rs");

include!("python_lxmd_remote_relay_parts/direct_restart.rs");

include!("python_lxmd_remote_relay_parts/inflight_resource_retry.rs");

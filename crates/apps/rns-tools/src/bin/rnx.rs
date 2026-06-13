#![allow(clippy::too_many_arguments)]

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

include!("rnx_parts/module_prelude.rs");

#[path = "rnx/ble.rs"]
mod ble;

#[path = "rnx/ble_camera.rs"]
mod ble_camera;

#[path = "rnx/ble_native.rs"]
mod ble_native;

#[path = "rnx/harness.rs"]
mod harness;

#[path = "rnx/ble_helpers.rs"]
mod helpers;

#[path = "rnx/resource_repro.rs"]
mod resource_repro;

#[path = "rnx/scenario.rs"]
mod scenario;

#[path = "rnx/scenario_mesh.rs"]
mod scenario_mesh;

#[path = "rnx/tcp.rs"]
mod tcp;

#[path = "rnx/tcp_session.rs"]
mod tcp_session;

include!("rnx_parts/cli.rs");

include!("rnx_parts/run.rs");

#[path = "support/python_channel_events.rs"]
mod python_channel_events;

#[path = "support/python_channel_process.rs"]
mod python_channel_process;

#[path = "support/python_channel_protocol.rs"]
mod python_channel_protocol;

include!("python_channel_interop_parts/module_prelude.rs");

include!("python_channel_interop_parts/rust_to_python_raw_resource_roundtri.rs");

include!("python_channel_interop_parts/ifac_python_interop.rs");

include!("python_channel_interop_parts/resource_size_matrix.rs");

include!("python_channel_interop_parts/resource_faults.rs");

include!("python_channel_interop_parts/reader_resource_interop.rs");

include!("python_channel_interop_parts/python_to_rust_link_identify_roundtr.rs");

include!("python_channel_interop_parts/python_multi_hop.rs");

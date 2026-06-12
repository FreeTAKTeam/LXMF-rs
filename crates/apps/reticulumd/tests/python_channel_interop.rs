#[path = "support/python_channel_events.rs"]
mod python_channel_events;

#[path = "support/python_channel_process.rs"]
mod python_channel_process;

#[path = "support/python_channel_protocol.rs"]
mod python_channel_protocol;

include!("python_channel_interop_parts/part_001_part_001.rs");

include!("python_channel_interop_parts/part_002_rust_to_python_raw_resource_roundtri.rs");

include!("python_channel_interop_parts/part_003_python_to_rust_link_identify_roundtr.rs");

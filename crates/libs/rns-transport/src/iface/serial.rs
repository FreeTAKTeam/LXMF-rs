include!("serial_parts/module_prelude.rs");

include!("serial_parts/run_serial_stream.rs");

#[cfg(test)]
#[path = "serial_parts/ifac_runtime_tests.rs"]
mod ifac_runtime_tests;

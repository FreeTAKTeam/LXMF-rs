include!("kiss_parts/module_prelude.rs");

include!("kiss_parts/flush_pending_kiss.rs");

include!("kiss_parts/kisstcpclientinterface.rs");

#[cfg(test)]
#[path = "kiss_parts/ifac_runtime_tests.rs"]
mod ifac_runtime_tests;

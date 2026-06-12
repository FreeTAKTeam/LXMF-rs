include!("mod_parts/module_prelude.rs");

mod app_mode_contract_tests;

mod auth_mode_tests;

mod certification_tests;

mod crypto_agility_tests;

mod key_management_tests;

mod model_tests;

mod operation_runtime_extraction_tests;

mod release_bc_tests;

include!("mod_parts/event_log_overflow_trigger.rs");

include!("mod_parts/rpcharness.rs");

include!("mod_parts/sdk_conformance_group_send_partial_o.rs");

include!("mod_parts/sdk_conformance_delivery_modes_and_p.rs");

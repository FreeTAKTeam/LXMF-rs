include!("tests_parts/module_prelude.rs");

include!("tests_parts/transport_bridge_reports_deferred_stamp_queue.rs");

include!("tests_parts/multi_service_identity_registry.rs");

include!("tests_parts/propagation_link_cache_reuses_same_s.rs");

include!("tests_parts/vrn76_builder_rejects_missing_periph.rs");

include!("tests_parts/bootstrap_best_effort_marks_vrn76_ki.rs");

include!("tests_parts/bootstrap_reports_auto_interface_as.rs");

include!("tests_parts/bootstrap_restores_path_cache.rs");

include!("tests_parts/bootstrap_strict_mode_panics_on_lora.rs");

include!("tests_parts/propagation_node_toml_config.rs");

include!("main_parts/module_prelude.rs");

mod client_codegen;

include!("main_parts/interop_baseline_path.rs");

include!("main_parts/xtaskcommand.rs");

include!("main_parts/run_ci_stage.rs");

include!("main_parts/run_interop_drift_check.rs");

include!("main_parts/run_sdk_api_stability_check.rs");

include!("main_parts/run_release_scorecard_check.rs");

include!("main_parts/run_key_management_check.rs");

include!("main_parts/run_python_impl_bench_report.rs");

include!("main_parts/write_python_impl_compare_report.rs");

include!("main_parts/run_rust_python_impl_benchmark.rs");

include!("main_parts/collect_resource_measurements_for_wo.rs");

include!("main_parts/capture_platform_descriptor.rs");

include!("main_parts/run_embedded_link_check.rs");

include!("main_parts/run_unused_deps.rs");

include!("main_parts/part_001_part_001.rs");

mod client_codegen;

include!("main_parts/part_002_interop_baseline_path.rs");

include!("main_parts/part_003_xtaskcommand.rs");

include!("main_parts/part_004_run_ci_stage.rs");

include!("main_parts/part_005_run_interop_drift_check.rs");

include!("main_parts/part_006_run_sdk_api_stability_check.rs");

include!("main_parts/part_007_run_release_scorecard_check.rs");

include!("main_parts/part_008_run_key_management_check.rs");

include!("main_parts/part_009_run_python_impl_bench_report.rs");

include!("main_parts/part_010_write_python_impl_compare_report.rs");

include!("main_parts/part_011_run_rust_python_impl_benchmark.rs");

include!("main_parts/part_012_collect_resource_measurements_for_wo.rs");

include!("main_parts/part_013_capture_platform_descriptor.rs");

include!("main_parts/part_014_run_embedded_link_check.rs");

include!("main_parts/part_015_run_unused_deps.rs");

include!("client_codegen_parts/module_prelude.rs");

include!("client_codegen_parts/validate_manifest_schema.rs");

include!("client_codegen_parts/project_grouped_legacy_rpc_schema.rs");

include!("client_codegen_parts/extract_methods_from_schema.rs");

include!("client_codegen_parts/generate_openapi_spec.rs");

include!("client_codegen_parts/compare_target_hash_baseline.rs");

include!("client_codegen_parts/run_go_compile_check.rs");

include!("client_codegen_parts/compare_dirs.rs");

include!("client_codegen_parts/workspace_root.rs");

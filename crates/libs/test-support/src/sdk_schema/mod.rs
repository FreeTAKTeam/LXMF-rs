include!("mod_parts/module_prelude.rs");

mod cookbook_tests;

mod failure_matrix_tests;

mod fixtures_contract_tests;

mod interop_corpus_tests;

mod rpc_core_tests;

mod rpc_domain_tests;

include!("mod_parts/workspace_root.rs");

include!("mod_parts/validate_openrpc_document.rs");

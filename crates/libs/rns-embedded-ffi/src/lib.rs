#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

extern crate alloc;

include!("lib_parts/module_prelude.rs");

include!("generated/node_error_codes.rs");

include!("lib_parts/rnsembeddedv1nodeerror.rs");

include!("lib_parts/rns_embedded_node_get_lifecycle_stat.rs");

include!("lib_parts/destination_list.rs");

include!("lib_parts/fixture_path.rs");

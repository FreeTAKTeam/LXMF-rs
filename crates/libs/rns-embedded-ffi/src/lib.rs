#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

extern crate alloc;

include!("lib_parts/part_001_part_001.rs");

include!("generated/node_error_codes.rs");

include!("lib_parts/part_002_rnsembeddedv1nodeerror.rs");

include!("lib_parts/part_003_rns_embedded_node_get_lifecycle_stat.rs");

include!("lib_parts/part_004_destination_list.rs");

include!("lib_parts/part_005_fixture_path.rs");

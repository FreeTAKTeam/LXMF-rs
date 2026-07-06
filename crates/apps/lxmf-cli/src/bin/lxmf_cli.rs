#![allow(clippy::result_large_err)]

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

include!("../main_parts/module_prelude.rs");

#[path = "../version.rs"]
mod version;

include!("../main_parts/cli.rs");

include!("../main_parts/build_start_request.rs");

#![allow(clippy::result_large_err)]

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

include!("main_parts/part_001_part_001.rs");

mod version;

include!("main_parts/part_002_cli.rs");

include!("main_parts/part_003_build_start_request.rs");

use clap::Parser;

use config::load_effective_args;

use launch::{launch_supervised, requires_supervised_launch};

use python_compat::emit_compatibility_notes;

use serde_json::json;

use std::env;

use std::fs;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use std::path::{Path, PathBuf};

use std::process::{Command, ExitCode};

use std::time::Duration;
